use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub struct DiskIoSource {
    query: Query,
    idle: Counter,
    read: Counter,
    write: Counter,
    drives: Counter,
}

/// `C:`-style LogicalDisk instance names; `_Total` and unlettered volumes are skipped.
pub fn drive_letter(instance: &str) -> Option<u8> {
    let b = instance.as_bytes();
    (b.len() == 2 && b[1] == b':' && b[0].is_ascii_uppercase()).then_some(b[0])
}

impl DiskIoSource {
    pub fn new() -> Result<Self, SourceError> {
        let query = Query::new()?;
        let idle = query.add(r"\PhysicalDisk(_Total)\% Idle Time")?;
        let read = query.add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")?;
        let write = query.add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")?;
        let drives = query.add(r"\LogicalDisk(*)\% Idle Time")?;
        query.collect()?;
        Ok(Self {
            query,
            idle,
            read,
            write,
            drives,
        })
    }
}

impl Source for DiskIoSource {
    fn id(&self) -> SourceId {
        SourceId::DiskIo
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        self.query.collect()?;
        out.set(
            MetricKey::DiskActivePercent,
            (100.0 - self.idle.value()?).clamp(0.0, 100.0),
        );
        out.set(MetricKey::DiskReadBps, self.read.value()?.max(0.0));
        out.set(MetricKey::DiskWriteBps, self.write.value()?.max(0.0));
        for (name, idle) in self.drives.array()? {
            if let Some(letter) = drive_letter(&name) {
                out.set(
                    MetricKey::DriveActivePercent(letter),
                    (100.0 - idle).clamp(0.0, 100.0),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::metric::Snapshot;

    #[test]
    fn drive_instances_are_single_letters() {
        assert_eq!(drive_letter("C:"), Some(b'C'));
        assert_eq!(drive_letter("_Total"), None);
        assert_eq!(drive_letter("HarddiskVolume1"), None);
    }

    #[test]
    fn live_drive_activity_includes_the_system_drive() {
        let mut disk = DiskIoSource::new().unwrap();
        disk.sample(&mut Snapshot::default()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut s = Snapshot::default();
        disk.sample(&mut s).unwrap();
        let system = std::env::var("SystemDrive").unwrap().as_bytes()[0];
        assert!((0.0..=100.0).contains(&s.get(MetricKey::DriveActivePercent(system)).unwrap()));
    }
}
