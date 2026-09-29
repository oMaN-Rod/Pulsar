use std::collections::HashMap;

use crate::metric::{GpuEngineKind, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub const ENGINE_COUNTER: &str = r"\GPU Engine(*)\Utilization Percentage";

pub struct GpuSource {
    query: Query,
    engines: Counter,
    dedicated: Counter,
    shared: Counter,
}

impl GpuSource {
    pub fn new() -> Result<Self, SourceError> {
        let unavailable = |_| SourceError::Unavailable("GPU counters unavailable".into());
        let query = Query::new()?;
        let engines = query.add(ENGINE_COUNTER).map_err(unavailable)?;
        let dedicated = query
            .add(r"\GPU Adapter Memory(*)\Dedicated Usage")
            .map_err(unavailable)?;
        let shared = query
            .add(r"\GPU Adapter Memory(*)\Shared Usage")
            .map_err(unavailable)?;
        query.collect()?;
        Ok(Self {
            query,
            engines,
            dedicated,
            shared,
        })
    }
}

impl Source for GpuSource {
    fn id(&self) -> SourceId {
        SourceId::Gpu
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        self.query.collect()?;
        let usage = aggregate_engines(&self.engines.array()?);
        out.set(MetricKey::GpuUtil, usage.util);
        for (kind, value) in usage.per_kind {
            out.set(MetricKey::GpuEngine(kind), value);
        }
        let sum = |c: &Counter| {
            c.array()
                .map(|v| v.iter().map(|(_, b)| b.max(0.0)).sum::<f64>())
        };
        out.set(MetricKey::GpuDedicatedBytes, sum(&self.dedicated)?);
        out.set(MetricKey::GpuSharedBytes, sum(&self.shared)?);
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct GpuUsage {
    /// Busiest engine type, as Task Manager reports overall GPU usage.
    pub util: f64,
    pub per_kind: HashMap<GpuEngineKind, f64>,
    pub per_pid: HashMap<u32, f64>,
}

/// Parses `pid_1234_luid_0x0_0xC5E6_phys_0_eng_3_engtype_VideoDecode`
/// into `(pid, engine id, engine kind)`. The engine id identifies one
/// physical engine across processes.
pub fn parse_engine_instance(name: &str) -> Option<(u32, &str, GpuEngineKind)> {
    let rest = name.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    let (engine, engtype) = rest.split_once("_engtype_")?;
    Some((pid.parse().ok()?, engine, engine_kind(engtype)))
}

fn engine_kind(engtype: &str) -> GpuEngineKind {
    match engtype {
        "3D" => GpuEngineKind::ThreeD,
        "VideoDecode" => GpuEngineKind::VideoDecode,
        "VideoEncode" => GpuEngineKind::VideoEncode,
        "Copy" => GpuEngineKind::Copy,
        t if t.starts_with("Compute") => GpuEngineKind::Compute,
        _ => GpuEngineKind::Other,
    }
}

/// Sums per-process utilisation on each physical engine, then reports the
/// busiest engine per kind, overall, and per process.
pub fn aggregate_engines(samples: &[(String, f64)]) -> GpuUsage {
    let mut per_engine: HashMap<&str, (GpuEngineKind, f64)> = HashMap::new();
    let mut per_pid_engine: HashMap<(u32, &str), f64> = HashMap::new();
    for (name, value) in samples {
        let Some((pid, engine, kind)) = parse_engine_instance(name) else {
            continue;
        };
        let value = value.max(0.0);
        per_engine.entry(engine).or_insert((kind, 0.0)).1 += value;
        *per_pid_engine.entry((pid, engine)).or_default() += value;
    }

    let mut usage = GpuUsage::default();
    for (kind, total) in per_engine.into_values() {
        let slot = usage.per_kind.entry(kind).or_default();
        *slot = slot.max(total.min(100.0));
    }
    usage.util = usage.per_kind.values().copied().fold(0.0, f64::max);
    for ((pid, _), value) in per_pid_engine {
        let slot = usage.per_pid.entry(pid).or_default();
        *slot = slot.max(value.min(100.0));
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, v: f64) -> (String, f64) {
        (name.to_string(), v)
    }

    #[test]
    fn parses_engine_instance_names() {
        assert_eq!(
            parse_engine_instance("pid_1234_luid_0x00000000_0x0000C5E6_phys_0_eng_0_engtype_3D"),
            Some((
                1234,
                "luid_0x00000000_0x0000C5E6_phys_0_eng_0",
                GpuEngineKind::ThreeD
            ))
        );
        assert_eq!(
            parse_engine_instance("pid_8_luid_0x0_0x1_phys_0_eng_5_engtype_Compute_1")
                .unwrap()
                .2,
            GpuEngineKind::Compute
        );
        assert_eq!(
            parse_engine_instance("pid_8_luid_0x0_0x1_phys_0_eng_9_engtype_Security")
                .unwrap()
                .2,
            GpuEngineKind::Other
        );
        assert_eq!(parse_engine_instance("garbage"), None);
        assert_eq!(parse_engine_instance("pid_x_luid_engtype_3D"), None);
    }

    #[test]
    fn util_is_busiest_engine_summed_across_processes() {
        let usage = aggregate_engines(&[
            s("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 30.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 25.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode", 40.0),
        ]);
        assert_eq!(usage.per_kind[&GpuEngineKind::ThreeD], 55.0);
        assert_eq!(usage.per_kind[&GpuEngineKind::VideoDecode], 40.0);
        assert_eq!(usage.util, 55.0);
    }

    #[test]
    fn per_process_is_busiest_engine_for_that_process() {
        let usage = aggregate_engines(&[
            s("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 25.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode", 40.0),
            s("pid_7_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 5.0),
        ]);
        assert_eq!(usage.per_pid[&2], 40.0);
        assert_eq!(usage.per_pid[&7], 5.0);
    }

    #[test]
    fn values_are_clamped_to_0_100() {
        let usage = aggregate_engines(&[
            s("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 90.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 90.0),
            s("pid_3_luid_0x0_0x1_phys_0_eng_1_engtype_Copy", -4.0),
        ]);
        assert_eq!(usage.util, 100.0);
        assert_eq!(usage.per_kind[&GpuEngineKind::Copy], 0.0);
    }

    #[test]
    fn no_samples_means_idle() {
        let usage = aggregate_engines(&[]);
        assert_eq!(usage.util, 0.0);
        assert!(usage.per_kind.is_empty());
    }
}
