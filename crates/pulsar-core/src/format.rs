use crate::metric::Unit;

const PLACEHOLDER: &str = "—";

/// Formats a metric value for display. Non-finite values render as a dash.
pub fn format_value(value: f64, unit: Unit) -> String {
    if !value.is_finite() {
        return PLACEHOLDER.to_string();
    }
    match unit {
        Unit::Percent => format!("{:.0}%", value.clamp(0.0, 100.0)),
        Unit::Bytes => scaled(value.max(0.0), &["B", "KB", "MB", "GB", "TB"]),
        Unit::BytesPerSec => scaled(value.max(0.0), &["B/s", "KB/s", "MB/s", "GB/s", "TB/s"]),
        Unit::Millis => format!("{:.0} ms", value.clamp(0.0, 999.0)),
        Unit::MHz => format!("{:.2} GHz", value.max(0.0) / 1000.0),
        Unit::Celsius => format!("{:.0}°C", value.clamp(-99.0, 199.0)),
        Unit::Rpm => format!("{:.0} RPM", value.clamp(0.0, 99_999.0)),
    }
}

/// The longest string `format_value` can produce for `unit`, used to reserve
/// a fixed width so the layout does not shift as values change.
pub fn widest_value(unit: Unit) -> &'static str {
    match unit {
        Unit::Percent => "100%",
        Unit::Bytes => "99.9 GB",
        Unit::BytesPerSec => "99.9 MB/s",
        Unit::Millis => "999 ms",
        Unit::MHz => "9.99 GHz",
        Unit::Celsius => "199°C",
        Unit::Rpm => "99999 RPM",
    }
}

/// Binary (1024) scaling. Values move to the next unit at 1000 so the
/// integer part never exceeds three digits; one decimal below 100.
fn scaled(mut value: f64, units: &[&str]) -> String {
    let mut index = 0;
    while value >= 999.5 && index + 1 < units.len() {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 || value >= 99.95 {
        format!("{:.0} {}", value, units[index])
    } else {
        format!("{:.1} {}", value, units[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_and_clamps() {
        assert_eq!(format_value(23.4, Unit::Percent), "23%");
        assert_eq!(format_value(123.0, Unit::Percent), "100%");
        assert_eq!(format_value(-1.0, Unit::Percent), "0%");
    }

    #[test]
    fn bytes_per_sec_scaling() {
        assert_eq!(format_value(0.0, Unit::BytesPerSec), "0 B/s");
        assert_eq!(format_value(512.0, Unit::BytesPerSec), "512 B/s");
        assert_eq!(format_value(1000.0, Unit::BytesPerSec), "1.0 KB/s");
        assert_eq!(format_value(120.0 * 1024.0, Unit::BytesPerSec), "120 KB/s");
        assert_eq!(
            format_value(1.26 * 1024.0 * 1024.0, Unit::BytesPerSec),
            "1.3 MB/s"
        );
    }

    #[test]
    fn bytes_scaling() {
        assert_eq!(
            format_value(16.0 * 1024.0 * 1024.0 * 1024.0, Unit::Bytes),
            "16.0 GB"
        );
        assert_eq!(
            format_value(512.0 * 1024.0 * 1024.0 * 1024.0, Unit::Bytes),
            "512 GB"
        );
    }

    #[test]
    fn millis_and_mhz() {
        assert_eq!(format_value(14.2, Unit::Millis), "14 ms");
        assert_eq!(format_value(1500.0, Unit::Millis), "999 ms");
        assert_eq!(format_value(3920.0, Unit::MHz), "3.92 GHz");
    }

    #[test]
    fn non_finite_is_placeholder() {
        assert_eq!(format_value(f64::NAN, Unit::Percent), "—");
        assert_eq!(format_value(f64::INFINITY, Unit::BytesPerSec), "—");
    }

    #[test]
    fn widest_is_at_least_as_long_as_any_formatted_value() {
        let units = [
            Unit::Percent,
            Unit::Bytes,
            Unit::BytesPerSec,
            Unit::Millis,
            Unit::MHz,
            Unit::Celsius,
            Unit::Rpm,
        ];
        let mut v = 0.0;
        while v < 1e13 {
            for unit in units {
                if unit == Unit::MHz && v >= 10_000.0 {
                    continue;
                }
                let formatted = format_value(v, unit);
                assert!(
                    formatted.chars().count() <= widest_value(unit).chars().count(),
                    "{formatted:?} is wider than {:?}",
                    widest_value(unit)
                );
            }
            v = v * 1.07 + 1.0;
        }
    }
}
