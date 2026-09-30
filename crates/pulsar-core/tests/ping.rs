use std::thread::sleep;
use std::time::Duration;

use pulsar_core::metric::{MetricKey, Snapshot, Source};
use pulsar_core::sources::PingSource;

#[test]
fn ping_localhost() {
    let mut ping = PingSource::spawn("127.0.0.1".into(), Duration::from_millis(200));
    sleep(Duration::from_millis(700));
    let mut snap = Snapshot::default();
    ping.sample(&mut snap).unwrap();
    assert!(snap.get(MetricKey::PingMs).unwrap() < 100.0);
    assert_eq!(snap.get(MetricKey::PingLossPercent), Some(0.0));
}

#[test]
fn ping_unresolvable_host_is_an_error_not_a_panic() {
    let mut ping = PingSource::spawn(
        "definitely-not-a-host.invalid".into(),
        Duration::from_millis(200),
    );
    sleep(Duration::from_millis(1500));
    let err = ping.sample(&mut Snapshot::default()).unwrap_err();
    assert!(err.to_string().contains("Cannot resolve"), "{err}");
}
