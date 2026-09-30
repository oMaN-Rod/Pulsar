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

fn root(letter: u8) -> HSTRING {
    HSTRING::from(format!("{}:\\", letter as char))
}

/// Letters of the fixed (non-removable, non-network) drives.
pub fn fixed_drives() -> Vec<u8> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| b'A' + i)
        .filter(|&letter| unsafe { GetDriveTypeW(&root(letter)) } == DRIVE_FIXED)
        .collect()
}

pub fn used_percent(free: f64, total: f64) -> f64 {
    if total > 0.0 {
        ((total - free) / total * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

impl DiskSpaceSource {
    fn refresh(&mut self) {
        self.cached.clear();
        for letter in fixed_drives() {
            let root = root(letter);
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
            out.set(
                MetricKey::DriveUsedPercent(letter),
                used_percent(free, total),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_percent_is_derived_from_free_and_total() {
        assert_eq!(used_percent(25.0, 100.0), 75.0);
        assert_eq!(used_percent(0.0, 0.0), 0.0);
    }

    #[test]
    fn fixed_drives_include_the_system_drive() {
        let system = std::env::var("SystemDrive").unwrap().as_bytes()[0];
        assert!(fixed_drives().contains(&system));
    }
}
