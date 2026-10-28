mod common;

use common::percent;
use pulsar_core::metric::{MetricKey, Snapshot, Source};
use pulsar_core::sources::MemorySource;

#[test]
fn memory_is_consistent() {
    let mut snap = Snapshot::default();
    MemorySource::new().sample(&mut snap).unwrap();
    percent(&snap, MetricKey::MemUsedPercent);
    let total = snap.get(MetricKey::MemTotalBytes).unwrap();
    let used = snap.get(MetricKey::MemUsedBytes).unwrap();
    assert!(total > 0.0 && used > 0.0 && used <= total);
    let commit = snap.get(MetricKey::CommitBytes).unwrap();
    assert!(commit <= snap.get(MetricKey::CommitLimitBytes).unwrap());
}

#[test]
fn memory_lists_fit_inside_physical_memory() {
    let mut snap = Snapshot::default();
    MemorySource::new().sample(&mut snap).unwrap();
    let total = snap.get(MetricKey::MemTotalBytes).unwrap();
    let lists: f64 = [
        MetricKey::MemStandbyBytes,
        MetricKey::MemModifiedBytes,
        MetricKey::MemFreeBytes,
    ]
    .into_iter()
    .map(|key| snap.get(key).expect("memory list counters"))
    .sum();
    assert!(lists > 0.0 && lists <= total * 1.01, "{lists} of {total}");
    assert!(snap.get(MetricKey::MemHardwareReservedBytes).is_some());
}
