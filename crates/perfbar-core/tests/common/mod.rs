#![allow(dead_code)]

use std::thread::sleep;
use std::time::Duration;

use perfbar_core::metric::{MetricKey, Snapshot, Source};

/// Rate counters need two collections; sample, wait a second, sample again.
pub fn sample_twice(source: &mut dyn Source) -> Snapshot {
    let _ = source.sample(&mut Snapshot::default());
    sleep(Duration::from_millis(1000));
    let mut snap = Snapshot::default();
    source.sample(&mut snap).expect("second sample succeeds");
    snap
}

pub fn percent(snap: &Snapshot, key: MetricKey) -> f64 {
    let v = snap.get(key).unwrap_or_else(|| panic!("{key:?} missing"));
    assert!((0.0..=100.0).contains(&v), "{key:?} = {v}");
    v
}
