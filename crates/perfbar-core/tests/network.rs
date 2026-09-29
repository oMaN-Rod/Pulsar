mod common;

use common::sample_twice;
use perfbar_core::metric::MetricKey;
use perfbar_core::sources::NetworkSource;

#[test]
fn network_reports_rates() {
    let snap = sample_twice(&mut NetworkSource::new().unwrap());
    assert!(snap.get(MetricKey::NetDownBps).unwrap() >= 0.0);
    assert!(snap.get(MetricKey::NetUpBps).unwrap() >= 0.0);
}
