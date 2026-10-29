use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{RateUnit, TemperatureUnit, Units};
use crate::metric::Unit;

const PLACEHOLDER: &str = "—";

static FAHRENHEIT: AtomicBool = AtomicBool::new(false);
static NETWORK_BITS: AtomicBool = AtomicBool::new(false);

/// Sets the units every later `format_value` call in this process uses.
pub fn set_units(units: Units) {
    FAHRENHEIT.store(
        units.temperature == TemperatureUnit::Fahrenheit,
        Ordering::Relaxed,
    );
    NETWORK_BITS.store(units.network == RateUnit::Bits, Ordering::Relaxed);
}

pub fn temperature(celsius: f64) -> (f64, &'static str) {
    if FAHRENHEIT.load(Ordering::Relaxed) {
        (celsius * 9.0 / 5.0 + 32.0, "°F")
    } else {
        (celsius, "°C")
    }
}

fn units() -> Units {
    Units {
        temperature: if FAHRENHEIT.load(Ordering::Relaxed) {
            TemperatureUnit::Fahrenheit
        } else {
            TemperatureUnit::Celsius
        },
        network: if NETWORK_BITS.load(Ordering::Relaxed) {
            RateUnit::Bits
        } else {
            RateUnit::Bytes
        },
    }
}

/// Formats a metric value for display in the units set by `set_units`.
/// Non-finite values render as a dash.
pub fn format_value(value: f64, unit: Unit) -> String {
    format_in(value, unit, units())
}

pub fn format_in(value: f64, unit: Unit, units: Units) -> String {
    if !value.is_finite() {
        return PLACEHOLDER.to_string();
    }
    let fahrenheit = units.temperature == TemperatureUnit::Fahrenheit;
    match unit {
        Unit::Percent => format!("{:.0}%", value.clamp(0.0, 100.0)),
        Unit::Bytes => scaled(value.max(0.0), &["B", "KB", "MB", "GB", "TB"]),
        Unit::BytesPerSec => scaled(value.max(0.0), &["B/s", "KB/s", "MB/s", "GB/s", "TB/s"]),
        Unit::NetworkRate if units.network == RateUnit::Bits => decimal(
            value.max(0.0) * 8.0,
            &["bps", "Kbps", "Mbps", "Gbps", "Tbps"],
        ),
        Unit::NetworkRate => scaled(value.max(0.0), &["B/s", "KB/s", "MB/s", "GB/s", "TB/s"]),
        Unit::Millis if value < 9.95 => format!("{:.1} ms", value.max(0.0)),
        Unit::Millis => format!("{:.0} ms", value.clamp(0.0, 999.0)),
        Unit::MHz => format!("{:.2} GHz", value.max(0.0) / 1000.0),
        Unit::Celsius if fahrenheit => {
            format!(
                "{:.0}°F",
                (value.clamp(-99.0, 199.0) * 9.0 / 5.0 + 32.0).max(-99.0)
            )
        }
        Unit::Celsius => format!("{:.0}°C", value.clamp(-99.0, 199.0)),
        Unit::Rpm => format!("{:.0} RPM", value.clamp(0.0, 99_999.0)),
        Unit::Watts if value < 10.0 => format!("{:.1} W", value.max(0.0)),
        Unit::Watts => format!("{:.0} W", value.clamp(0.0, 999.0)),
        Unit::Volts if value < 10.0 => format!("{:.3} V", value.max(0.0)),
        Unit::Volts => format!("{:.2} V", value.min(99.99)),
        Unit::Ratio if value.fract() == 0.0 => format!("×{:.0}", value.clamp(0.0, 999.0)),
        Unit::Ratio => format!("×{:.2}", value.clamp(0.0, 999.0)),
        Unit::Cycles => format!("{:.0}", value.clamp(0.0, 9999.0)),
        Unit::WattHours => format!("{:.1} Wh", value.clamp(0.0, 999.0)),
        Unit::Seconds => duration(value.max(0.0)),
        Unit::Count => format!("{:.0}", value.clamp(0.0, 9_999_999.0)),
        Unit::FramesPerSec => format!("{:.0} fps", value.clamp(0.0, 9999.0)),
    }
}

/// `"45 min"`, `"2 h 05 min"`, or `"3 d 4 h"` past a day.
fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, h) => format!("{h} h {minutes:02} min"),
        (d, h) => format!("{d} d {h} h"),
    }
}

/// The longest string `format_value` can produce for `unit`, used to reserve
/// a fixed width so the layout does not shift as values change.
pub fn widest_value(unit: Unit) -> &'static str {
    widest_in(unit, units())
}

pub fn widest_in(unit: Unit, units: Units) -> &'static str {
    match unit {
        Unit::Percent => "100%",
        Unit::Bytes => "99.9 GB",
        Unit::BytesPerSec | Unit::NetworkRate => "99.9 MB/s",
        Unit::Millis => "999 ms",
        Unit::MHz => "9.99 GHz",
        Unit::Celsius if units.temperature == TemperatureUnit::Fahrenheit => "390°F",
        Unit::Celsius => "199°C",
        Unit::Rpm => "99999 RPM",
        Unit::Watts => "999 W",
        Unit::Volts => "9.999 V",
        Unit::Ratio => "×99.75",
        Unit::Cycles => "9999",
        Unit::WattHours => "999.9 Wh",
        Unit::Seconds => "23 h 59 min",
        Unit::Count => "9999999",
        Unit::FramesPerSec => "999 fps",
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

/// Decimal (1000) scaling, otherwise as `scaled`.
fn decimal(mut value: f64, units: &[&str]) -> String {
    let mut index = 0;
    while value >= 999.5 && index + 1 < units.len() {
        value /= 1000.0;
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
        assert_eq!(format_value(6.94, Unit::Millis), "6.9 ms");
        assert_eq!(format_value(143.6, Unit::FramesPerSec), "144 fps");
        assert_eq!(format_value(1500.0, Unit::Millis), "999 ms");
        assert_eq!(format_value(3920.0, Unit::MHz), "3.92 GHz");
        assert_eq!(format_value(86.6, Unit::Watts), "87 W");
    }

    #[test]
    fn fahrenheit_and_bits_follow_the_chosen_units() {
        let metric = Units::default();
        assert_eq!(
            format_in(1_000_000.0, Unit::NetworkRate, metric),
            "977 KB/s"
        );
        assert_eq!(format_in(45.0, Unit::Celsius, metric), "45°C");
        let other = Units {
            temperature: TemperatureUnit::Fahrenheit,
            network: RateUnit::Bits,
        };
        assert_eq!(format_in(45.0, Unit::Celsius, other), "113°F");
        assert_eq!(format_in(-40.0, Unit::Celsius, other), "-40°F");
        assert_eq!(format_in(1_000_000.0, Unit::NetworkRate, other), "8.0 Mbps");
        assert_eq!(format_in(100.0, Unit::NetworkRate, other), "800 bps");
        assert_eq!(format_in(125_000.0, Unit::NetworkRate, other), "1.0 Mbps");
        assert_eq!(
            format_in(1_000_000.0, Unit::BytesPerSec, other),
            "977 KB/s",
            "disk rates stay in bytes"
        );
        let mut v: f64 = 0.0;
        while v < 1e13 {
            for unit in [Unit::NetworkRate, Unit::Celsius] {
                let value = if unit == Unit::Celsius {
                    v.min(400.0) - 150.0
                } else {
                    v
                };
                for units in [metric, other] {
                    let formatted = format_in(value, unit, units);
                    let widest = widest_in(unit, units);
                    assert!(
                        formatted.chars().count() <= widest.chars().count(),
                        "{formatted:?} is wider than {widest:?}"
                    );
                }
            }
            v = v * 1.07 + 1.0;
        }
    }

    #[test]
    fn durations_read_as_hours_and_minutes() {
        assert_eq!(format_value(0.0, Unit::Seconds), "0 min");
        assert_eq!(format_value(45.0 * 60.0, Unit::Seconds), "45 min");
        assert_eq!(
            format_value(2.0 * 3600.0 + 300.0, Unit::Seconds),
            "2 h 05 min"
        );
        assert_eq!(format_value(28.0 * 3600.0, Unit::Seconds), "1 d 4 h");
        assert_eq!(format_value(51.3, Unit::WattHours), "51.3 Wh");
        assert_eq!(format_value(12345.0, Unit::Count), "12345");
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
            Unit::Watts,
        ];
        let mut v: f64 = 0.0;
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
