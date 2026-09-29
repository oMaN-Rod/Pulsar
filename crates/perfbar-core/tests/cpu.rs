mod common;

use common::{percent, sample_twice};
use perfbar_core::metric::MetricKey;
use perfbar_core::sources::CpuSource;

#[test]
fn cpu_reports_total_cores_and_clock() {
    let snap = sample_twice(&mut CpuSource::new().unwrap());
    percent(&snap, MetricKey::CpuTotal);
    percent(&snap, MetricKey::CpuCore(0));
    let mhz = snap.get(MetricKey::CpuClockMhz).expect("clock");
    assert!(mhz > 100.0 && mhz < 10_000.0, "clock {mhz}");
}
