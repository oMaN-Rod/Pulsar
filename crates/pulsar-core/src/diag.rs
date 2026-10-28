//! A plain-text report of every reading, for bug reports.

use std::fmt::Write;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config::Config;
use crate::format::format_value;
use crate::metric::{ItemKind, Snapshot};
use crate::sampler::Sampler;
use crate::sources::{ProcessSort, top_by};

/// Rate counters need two readings this far apart.
const SETTLE: Duration = Duration::from_secs(1);

pub const ARG: &str = "--diag";
const FILE_NAME: &str = "diagnostics.txt";

/// Writes `report` to `path`, or to the logs folder when `path` is `None`,
/// and returns where it went.
pub fn save(config: &Config, header: &str, path: Option<&Path>) -> io::Result<PathBuf> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => crate::paths::logs_dir()
            .ok_or_else(|| io::Error::other("LOCALAPPDATA is not set"))?
            .join(FILE_NAME),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, report(config, header))?;
    Ok(path)
}

/// Samples every source, whether or not the overlay shows it, and describes
/// what each one returned. Pings only when the ping item is on.
pub fn report(config: &Config, header: &str) -> String {
    let mut all = config.clone();
    for item in &mut all.items {
        item.enabled = item.kind != ItemKind::Ping || config.is_enabled(ItemKind::Ping);
    }
    let mut sampler = Sampler::from_config(&all);
    sampler.processes_flag().store(true, Ordering::Relaxed);
    sampler.sample();
    std::thread::sleep(SETTLE);
    sampler.sample();
    std::thread::sleep(SETTLE);
    describe(&sampler.sample(), header)
}

pub fn describe(snapshot: &Snapshot, header: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{header}\n");
    let mut errors: Vec<_> = snapshot.errors.iter().collect();
    errors.sort_by_key(|(id, _)| format!("{id:?}"));
    let _ = writeln!(out, "Unavailable sources: {}", errors.len());
    for (id, reason) in errors {
        let _ = writeln!(out, "  {id:?}: {reason}");
    }
    let mut metrics: Vec<_> = snapshot.metrics.iter().collect();
    metrics.sort_by_key(|(key, _)| format!("{key:?}"));
    let _ = writeln!(out, "\nReadings: {}", metrics.len());
    for (key, value) in metrics {
        let _ = writeln!(
            out,
            "  {key:?} = {value} ({})",
            format_value(*value, key.unit())
        );
    }
    let _ = writeln!(
        out,
        "\nNetwork adapters with traffic: {}",
        snapshot.adapters.len()
    );
    for a in &snapshot.adapters {
        let _ = writeln!(
            out,
            "  {}: down {} B/s, up {} B/s",
            a.name, a.down_bps, a.up_bps
        );
    }
    let _ = writeln!(out, "\nProcesses: {}", snapshot.processes.len());
    for p in top_by(&snapshot.processes, ProcessSort::Cpu, 10) {
        let _ = writeln!(
            out,
            "  {}: cpu {:.1}%, private {} B, gpu {:.1}%, io {} B/s",
            p.name, p.cpu_percent, p.private_bytes, p.gpu_percent, p.io_bps
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::{MetricKey, SourceId};

    #[test]
    fn describes_readings_and_failures_in_a_stable_order() {
        let mut s = Snapshot::default();
        s.set(MetricKey::MemUsedPercent, 41.0);
        s.set(MetricKey::CpuTotal, 12.0);
        s.errors
            .insert(SourceId::Battery, "This PC has no battery".into());
        let text = describe(&s, "Pulsar 1.0");
        assert!(text.starts_with("Pulsar 1.0\n"));
        assert!(text.contains("  Battery: This PC has no battery"));
        let cpu = text.find("CpuTotal = 12 (12%)").unwrap();
        let mem = text.find("MemUsedPercent = 41 (41%)").unwrap();
        assert!(cpu < mem, "sorted by name");
    }
}
