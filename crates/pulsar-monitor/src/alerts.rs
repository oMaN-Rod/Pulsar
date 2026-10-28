//! What an alert says.

use pulsar_core::alerts::Alert;
use pulsar_core::config::AlertsConfig;
use pulsar_core::format::format_value;
use pulsar_core::metric::{MetricKey, Snapshot, Unit};
use pulsar_core::tr;

/// The notification's title and message.
pub fn text(alert: Alert, s: &Snapshot, config: &AlertsConfig) -> (String, String) {
    let value = |key: MetricKey| {
        s.get(key)
            .map_or_else(String::new, |v| format_value(v, key.unit()))
    };
    let limit = |l: u32, unit: Unit| format_value(f64::from(l), unit);
    match alert {
        Alert::CpuHot => (
            tr!("CPU at {}", value(MetricKey::CpuTempC)),
            tr!(
                "Above your limit of {} for half a minute.",
                limit(config.cpu.limit, Unit::Celsius)
            ),
        ),
        Alert::GpuHot => (
            tr!("GPU at {}", value(MetricKey::GpuTempC)),
            tr!(
                "Above your limit of {} for half a minute.",
                limit(config.gpu.limit, Unit::Celsius)
            ),
        ),
        Alert::BatteryLow => (
            tr!("Battery at {}", value(MetricKey::BatteryPercent)),
            tr!("Plug in soon.").to_string(),
        ),
        Alert::DriveLow(letter) => (
            tr!("Drive {}: is nearly full", letter as char),
            tr!("{} free.", value(MetricKey::VolumeFreeBytes(letter))),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_name_the_reading_and_the_limit() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTempC, 93.4);
        s.set(
            MetricKey::VolumeFreeBytes(b'C'),
            12.0 * 1024.0 * 1024.0 * 1024.0,
        );
        let c = AlertsConfig::default();
        assert_eq!(
            text(Alert::CpuHot, &s, &c),
            (
                "CPU at 93°C".to_string(),
                "Above your limit of 90°C for half a minute.".to_string()
            )
        );
        assert_eq!(
            text(Alert::DriveLow(b'C'), &s, &c),
            (
                "Drive C: is nearly full".to_string(),
                "12.0 GB free.".to_string()
            )
        );
    }
}
