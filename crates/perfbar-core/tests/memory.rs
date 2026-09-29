mod common;

use common::percent;
use perfbar_core::metric::{MetricKey, Snapshot, Source};
use perfbar_core::sources::MemorySource;

#[test]
fn memory_is_consistent() {
    let mut snap = Snapshot::default();
    MemorySource.sample(&mut snap).unwrap();
    percent(&snap, MetricKey::MemUsedPercent);
    let total = snap.get(MetricKey::MemTotalBytes).unwrap();
    let used = snap.get(MetricKey::MemUsedBytes).unwrap();
    assert!(total > 0.0 && used > 0.0 && used <= total);
    let commit = snap.get(MetricKey::CommitBytes).unwrap();
    assert!(commit <= snap.get(MetricKey::CommitLimitBytes).unwrap());
}
