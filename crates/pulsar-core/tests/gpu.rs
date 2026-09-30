mod common;

use common::{percent, sample_twice};
use pulsar_core::metric::{MetricKey, SourceError};
use pulsar_core::sources::GpuSource;

#[test]
fn gpu_reports_or_is_cleanly_unavailable() {
    match GpuSource::new() {
        Ok(mut gpu) => {
            let snap = sample_twice(&mut gpu);
            percent(&snap, MetricKey::GpuUtil);
            assert!(snap.get(MetricKey::GpuDedicatedBytes).unwrap() >= 0.0);
        }
        Err(SourceError::Unavailable(msg)) => assert_eq!(msg, "GPU counters unavailable"),
        Err(other) => panic!("unexpected error: {other}"),
    }
}
