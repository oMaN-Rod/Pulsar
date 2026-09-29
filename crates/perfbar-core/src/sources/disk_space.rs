use std::time::{Duration, Instant};

use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::core::HSTRING;

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

const DRIVE_FIXED: u32 = 3;
const REFRESH: Duration = Duration::from_secs(30);

/// Free/total space per fixed volume, re-queried every 30 s and cached in between.
#[derive(Default)]
pub struct DiskSpaceSource {
    cached: Vec<(u8, f64, f64)>,
    refreshed: Option<Instant>,
}

impl DiskSpaceSource {
    fn refresh(&mut self) {
        self.cached.clear();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26u8 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = b'A' + i;
            let root = HSTRING::from(format!("{}:\\", letter as char));
            if unsafe { GetDriveTypeW(&root) } != DRIVE_FIXED {
                continue;
            }
            let (mut free, mut total) = (0u64, 0u64);
            let ok = unsafe { GetDiskFreeSpaceExW(&root, Some(&mut free), Some(&mut total), None) };
            if ok.is_ok() {
                self.cached.push((letter, free as f64, total as f64));
            }
        }
        self.refreshed = Some(Instant::now());
    }
}

impl Source for DiskSpaceSource {
    fn id(&self) -> SourceId {
        SourceId::DiskSpace
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if self.refreshed.is_none_or(|t| t.elapsed() >= REFRESH) {
            self.refresh();
        }
        for &(letter, free, total) in &self.cached {
            out.set(MetricKey::VolumeFreeBytes(letter), free);
            out.set(MetricKey::VolumeTotalBytes(letter), total);
        }
        Ok(())
    }
}
