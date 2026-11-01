use std::collections::HashMap;

use crate::metric::{GpuEngineKind, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, PdhError, Query};

pub const ENGINE_COUNTER: &str = r"\GPU Engine(*)\Utilization Percentage";

/// PDH reports "no data" when the GPU objects exist but have no instances
/// (VMs, RDP sessions, basic display drivers).
fn unavailable(_: PdhError) -> SourceError {
    SourceError::Unavailable(crate::tr!("GPU counters unavailable").into())
}

pub struct GpuSource {
    query: Query,
    engines: Counter,
    dedicated: Counter,
    shared: Counter,
}

impl GpuSource {
    pub fn new() -> Result<Self, SourceError> {
        Self::with_paths(
            ENGINE_COUNTER,
            r"\GPU Adapter Memory(*)\Dedicated Usage",
            r"\GPU Adapter Memory(*)\Shared Usage",
        )
    }

    fn with_paths(engines: &str, dedicated: &str, shared: &str) -> Result<Self, SourceError> {
        let query = Query::new()?;
        let engines = query.add(engines).map_err(unavailable)?;
        let dedicated = query.add(dedicated).map_err(unavailable)?;
        let shared = query.add(shared).map_err(unavailable)?;
        query.collect().map_err(unavailable)?;
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
        self.sample_counters(out).map_err(unavailable)
    }
}

impl GpuSource {
    fn sample_counters(&self, out: &mut Snapshot) -> Result<(), PdhError> {
        self.query.collect()?;
        let usage = aggregate_engines(&self.engines.array()?);
        out.set(MetricKey::GpuUtil, usage.util);
        for (kind, value) in usage.per_kind {
            out.set(MetricKey::GpuEngine(kind), value);
        }
        let dedicated = per_adapter(&self.dedicated.array()?);
        let shared = per_adapter(&self.shared.array()?);
        out.set(MetricKey::GpuDedicatedBytes, dedicated.values().sum());
        out.set(MetricKey::GpuSharedBytes, shared.values().sum());
        let readings = &mut out.gpu_readings;
        readings.seen.extend(dedicated.keys().chain(shared.keys()));
        readings.load = Some(usage.per_adapter);
        readings.dedicated = dedicated;
        readings.shared = shared;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct GpuUsage {
    /// Busiest engine type, as Task Manager reports overall GPU usage.
    pub util: f64,
    pub per_kind: HashMap<GpuEngineKind, f64>,
    pub per_pid: HashMap<u32, f64>,
    /// Busiest engine on each adapter, by LUID.
    pub per_adapter: HashMap<u64, f64>,
}

/// The adapter LUID in an instance name such as
/// `pid_8_luid_0x00000000_0x0000C5E6_phys_0_eng_3_engtype_3D` or
/// `luid_0x00000000_0x0000C5E6_phys_0`, high part first.
pub fn parse_luid(name: &str) -> Option<u64> {
    let (_, rest) = name.split_once("luid_0x")?;
    let (high, rest) = rest.split_once("_0x")?;
    let low = rest.split('_').next()?;
    let high = u32::from_str_radix(high, 16).ok()?;
    let low = u32::from_str_radix(low, 16).ok()?;
    Some((u64::from(high) << 32) | u64::from(low))
}

/// Sums `GPU Adapter Memory` instances by adapter; a linked adapter lists
/// one instance per physical part.
fn per_adapter(samples: &[(String, f64)]) -> HashMap<u64, f64> {
    let mut out = HashMap::new();
    for (name, value) in samples {
        if let Some(luid) = parse_luid(name) {
            *out.entry(luid).or_default() += value.max(0.0);
        }
    }
    out
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
/// busiest engine per kind, overall, per process and per adapter.
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
    for (engine, (kind, total)) in per_engine {
        let total = total.min(100.0);
        let slot = usage.per_kind.entry(kind).or_default();
        *slot = slot.max(total);
        if let Some(luid) = parse_luid(engine) {
            let slot = usage.per_adapter.entry(luid).or_default();
            *slot = slot.max(total);
        }
    }
    usage.util = usage.per_kind.values().copied().fold(0.0, f64::max);
    for ((pid, _), value) in per_pid_engine {
        let slot = usage.per_pid.entry(pid).or_default();
        *slot = slot.max(value.min(100.0));
    }
    usage
}

#[cfg(test)]
pub(crate) mod tests {
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

    /// Registered but instance-less, like GPU counters on a VM or RDP session.
    pub(crate) const EMPTY_OBJECT: &str = r"\Hyper-V Dynamic Memory VM(*)\Guest Available Memory";

    #[test]
    fn object_without_instances_is_reported_as_unavailable() {
        let result = GpuSource::with_paths(EMPTY_OBJECT, EMPTY_OBJECT, EMPTY_OBJECT)
            .and_then(|mut gpu| gpu.sample(&mut Snapshot::default()));
        match result {
            Err(SourceError::Unavailable(msg)) => assert_eq!(msg, "GPU counters unavailable"),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn luids_parse_from_engine_and_memory_instances() {
        assert_eq!(
            parse_luid("pid_8_luid_0x00000001_0x0000C5E6_phys_0_eng_3_engtype_3D"),
            Some(0x1_0000_C5E6)
        );
        assert_eq!(
            parse_luid("luid_0x00000000_0x0001a2b3_phys_1"),
            Some(0x1_A2B3)
        );
        assert_eq!(parse_luid("pid_8_eng_3"), None);
    }

    #[test]
    fn each_adapter_reports_its_own_busiest_engine() {
        let usage = aggregate_engines(&[
            s("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 30.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 25.0),
            s("pid_3_luid_0x0_0x2_phys_0_eng_5_engtype_Compute_0", 80.0),
            s("pid_3_luid_0x0_0x2_phys_0_eng_0_engtype_3D", 10.0),
        ]);
        assert_eq!(usage.per_adapter[&1], 55.0);
        assert_eq!(usage.per_adapter[&2], 80.0);
        assert_eq!(usage.util, 80.0);
    }

    #[test]
    fn memory_is_summed_per_adapter() {
        let memory = per_adapter(&[
            s("luid_0x0_0x1_phys_0", 100.0),
            s("luid_0x0_0x2_phys_0", 7.0),
            s("luid_0x0_0x2_phys_1", 3.0),
        ]);
        assert_eq!(memory[&1], 100.0);
        assert_eq!(memory[&2], 10.0);
    }

    #[test]
    fn no_samples_means_idle() {
        let usage = aggregate_engines(&[]);
        assert_eq!(usage.util, 0.0);
        assert!(usage.per_kind.is_empty());
    }
}
