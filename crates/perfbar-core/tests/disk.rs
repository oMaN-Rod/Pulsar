mod common;

use common::{percent, sample_twice};
use perfbar_core::metric::{MetricKey, Snapshot, Source};
use perfbar_core::sources::{DiskIoSource, DiskSpaceSource};

#[test]
fn disk_io_reports_activity_and_rates() {
    let snap = sample_twice(&mut DiskIoSource::new().unwrap());
    percent(&snap, MetricKey::DiskActivePercent);
    assert!(snap.get(MetricKey::DiskReadBps).unwrap() >= 0.0);
    assert!(snap.get(MetricKey::DiskWriteBps).unwrap() >= 0.0);
}

#[test]
fn disk_space_includes_system_drive() {
    let mut snap = Snapshot::default();
    DiskSpaceSource::default().sample(&mut snap).unwrap();
    let drive = std::env::var("SystemDrive").unwrap().as_bytes()[0];
    let free = snap
        .get(MetricKey::VolumeFreeBytes(drive))
        .expect("system drive");
    assert!(free <= snap.get(MetricKey::VolumeTotalBytes(drive)).unwrap());
}
