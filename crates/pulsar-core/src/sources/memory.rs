use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

pub struct MemorySource;

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
        Ok(())
    }
}
