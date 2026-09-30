use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub struct DiskIoSource {
    query: Query,
    idle: Counter,
    read: Counter,
    write: Counter,
}

impl DiskIoSource {
    pub fn new() -> Result<Self, SourceError> {
        let query = Query::new()?;
        let idle = query.add(r"\PhysicalDisk(_Total)\% Idle Time")?;
        let read = query.add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")?;
        let write = query.add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")?;
        query.collect()?;
        Ok(Self {
            query,
            idle,
            read,
            write,
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
        Ok(())
    }
}
