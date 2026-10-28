use windows::Win32::System::SystemInformation::{
    GetPhysicallyInstalledSystemMemory, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub struct MemorySource {
    /// Where the memory goes; the totals do without it.
    lists: Option<Lists>,
    installed_bytes: Option<f64>,
}

struct Lists {
    query: Query,
    standby: [Counter; 3],
    modified: Counter,
    free: Counter,
    paged: Counter,
    non_paged: Counter,
}

impl Lists {
    fn new() -> Option<Self> {
        let query = Query::new().ok()?;
        let add = |name: &str| query.add(&format!(r"\Memory\{name}")).ok();
        let lists = Self {
            standby: [
                add("Standby Cache Core Bytes")?,
                add("Standby Cache Normal Priority Bytes")?,
                add("Standby Cache Reserve Bytes")?,
            ],
            modified: add("Modified Page List Bytes")?,
            free: add("Free & Zero Page List Bytes")?,
            paged: add("Pool Paged Bytes")?,
            non_paged: add("Pool Nonpaged Bytes")?,
            query,
        };
        lists.query.collect().ok()?;
        Some(lists)
    }

    fn sample(&self, out: &mut Snapshot) {
        if self.query.collect().is_err() {
            return;
        }
        let standby: Option<f64> = self.standby.iter().map(|c| c.value().ok()).sum();
        for (key, value) in [
            (MetricKey::MemStandbyBytes, standby),
            (MetricKey::MemModifiedBytes, self.modified.value().ok()),
            (MetricKey::MemFreeBytes, self.free.value().ok()),
            (MetricKey::MemPagedPoolBytes, self.paged.value().ok()),
            (MetricKey::MemNonPagedPoolBytes, self.non_paged.value().ok()),
        ] {
            if let Some(v) = value {
                out.set(key, v.max(0.0));
            }
        }
    }
}

impl MemorySource {
    pub fn new() -> Self {
        let mut kb = 0u64;
        let installed_bytes = unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }
            .ok()
            .map(|()| kb as f64 * 1024.0);
        Self {
            lists: Lists::new(),
            installed_bytes,
        }
    }
}

impl Default for MemorySource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for MemorySource {
    fn id(&self) -> SourceId {
        SourceId::Memory
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe { GlobalMemoryStatusEx(&mut status)? };

        let total = status.ullTotalPhys as f64;
        let available = status.ullAvailPhys as f64;
        let used = total - available;
        out.set(MetricKey::MemTotalBytes, total);
        out.set(MetricKey::MemAvailableBytes, available);
        out.set(MetricKey::MemUsedBytes, used);
        if total > 0.0 {
            out.set(MetricKey::MemUsedPercent, used / total * 100.0);
        }
        let commit_limit = status.ullTotalPageFile as f64;
        out.set(MetricKey::CommitLimitBytes, commit_limit);
        out.set(
            MetricKey::CommitBytes,
            commit_limit - status.ullAvailPageFile as f64,
        );
        if let Some(installed) = self.installed_bytes {
            out.set(
                MetricKey::MemHardwareReservedBytes,
                (installed - total).max(0.0),
            );
        }
        if let Some(lists) = &self.lists {
            lists.sample(out);
        }
        Ok(())
    }
}
