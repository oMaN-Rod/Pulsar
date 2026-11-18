use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{MemoryUnit, RateUnit, TemperatureUnit, Units};
use crate::metric::Unit;

const PLACEHOLDER: &str = "—";

static FAHRENHEIT: AtomicBool = AtomicBool::new(false);
static NETWORK_BITS: AtomicBool = AtomicBool::new(false);
static MEMORY_BYTES: AtomicBool = AtomicBool::new(false);

/// Sets the units every later `format_value` call in this process uses.
pub fn set_units(units: Units) {
    FAHRENHEIT.store(
        units.temperature == TemperatureUnit::Fahrenheit,
        Ordering::Relaxed,
    );
    NETWORK_BITS.store(units.network == RateUnit::Bits, Ordering::Relaxed);
    MEMORY_BYTES.store(units.memory == MemoryUnit::Bytes, Ordering::Relaxed);
}

pub fn temperature(celsius: f64) -> (f64, &'static str) {
    if FAHRENHEIT.load(Ordering::Relaxed) {
        (celsius * 9.0 / 5.0 + 32.0, "°F")
    } else {
        (celsius, "°C")
    }
}

/// The units set by `set_units`.
pub fn units() -> Units {
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
        memory: if MEMORY_BYTES.load(Ordering::Relaxed) {
            MemoryUnit::Bytes
        } else {
            MemoryUnit::Percent
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
        Unit::NetworkRate if units.network == RateUnit::Bits => rate(
            value.max(0.0) * 8.0 / 1000.0,
            1000.0,
            &["Kbps", "Mbps", "Gbps", "Tbps"],
        ),
        Unit::NetworkRate => rate(
            value.max(0.0) / 1024.0,
            1024.0,
            &["KB/s", "MB/s", "GB/s", "TB/s"],
        ),
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
        Unit::Watts if value < 9.95 => format!("{:.1} W", value.max(0.0)),
        Unit::Watts => format!("{:.0} W", value.clamp(0.0, 999.0)),
        Unit::Volts if value < 9.9995 => format!("{:.3} V", value.max(0.0)),
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
        Unit::BytesPerSec => "99.9 MB/s",
        Unit::NetworkRate if units.network == RateUnit::Bits => "999 Mbps",
        Unit::NetworkRate => "999 MB/s",
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
        Unit::FramesPerSec => "9999 fps",
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

/// Network rates start at kilo, so idle and busy readings keep similar
/// widths: one decimal below 10, whole numbers from there.
fn rate(mut value: f64, step: f64, units: &[&str]) -> String {
    let mut index = 0;
    while value >= 999.5 && index + 1 < units.len() {
        value /= step;
        index += 1;
    }
    if value < 9.95 {
        format!("{:.1} {}", value, units[index])
    } else {
        format!("{:.0} {}", value, units[index])
    }
}

/// Rates are drawn right-aligned in their slot so the unit holds still.
pub fn right_aligned(unit: Unit) -> bool {
    matches!(unit, Unit::NetworkRate | Unit::BytesPerSec)
}

/// A piece of formatted text: a reading such as `4.2 GB` or `—`, or the
/// plain text between readings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span<'a> {
    pub text: &'a str,
    pub reading: bool,
    /// The longest the reading can grow to, when its unit is known.
    pub widest: Option<&'static str>,
    /// Keep the reading's unit still by aligning it to the right of its room.
    pub right: bool,
}

/// Suffixes `format_in` writes after a number, with the widest reading of
/// each; longer suffixes first so `Wh` wins over `W`.
const SUFFIXES: &[(&str, &str, bool)] = &[
    ("KB/s", "99.9 MB/s", true),
    ("MB/s", "99.9 MB/s", true),
    ("GB/s", "99.9 MB/s", true),
    ("TB/s", "99.9 MB/s", true),
    ("B/s", "99.9 MB/s", true),
    ("Kbps", "999 Mbps", true),
    ("Mbps", "999 Mbps", true),
    ("Gbps", "999 Mbps", true),
    ("Tbps", "999 Mbps", true),
    ("RPM", "99999 RPM", false),
    ("GHz", "9.99 GHz", false),
    ("fps", "9999 fps", false),
    ("min", "23 h 59 min", false),
    ("°C", "199°C", false),
    ("°F", "390°F", false),
    ("KB", "99.9 GB", false),
    ("MB", "99.9 GB", false),
    ("GB", "99.9 GB", false),
    ("TB", "99.9 GB", false),
    ("ms", "999 ms", false),
    ("Wh", "999.9 Wh", false),
    ("B", "99.9 GB", false),
    ("W", "999 W", false),
    ("V", "9.999 V", false),
    ("%", "100%", false),
    ("h", "23 h 59 min", false),
    ("d", "23 h 59 min", false),
];

/// Splits `text` into readings and the plain text around them, so each
/// reading can keep the room of its widest form as its value changes.
pub fn spans(text: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let found = if rest.starts_with(PLACEHOLDER) {
            Some((PLACEHOLDER.len(), None, false))
        } else if starts_number(text, i) {
            reading_len(rest).map(|(len, widest, right)| (len, Some(widest), right))
        } else {
            None
        };
        match found {
            Some((len, widest, right)) => {
                if plain < i {
                    out.push(Span {
                        text: &text[plain..i],
                        reading: false,
                        widest: None,
                        right: false,
                    });
                }
                out.push(Span {
                    text: &text[i..i + len],
                    reading: true,
                    widest,
                    right,
                });
                i += len;
                plain = i;
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain < text.len() {
        out.push(Span {
            text: &text[plain..],
            reading: false,
            widest: None,
            right: false,
        });
    }
    out
}

/// `text` with every reading at its widest, to measure room that holds
/// still while the readings change.
pub fn widest_like(text: &str) -> String {
    spans(text)
        .iter()
        .map(|s| s.widest.unwrap_or(s.text))
        .collect()
}

/// A number starts at `i`: a digit, or a minus sign before one, not inside
/// a word such as `RTX 4090` would be.
fn starts_number(text: &str, i: usize) -> bool {
    let rest = &text[i..];
    let digit_at = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_digit());
    let begins = digit_at(rest)
        || (rest.starts_with('-') && digit_at(&rest[1..]))
        || (rest.starts_with('\u{2212}') && digit_at(&rest['\u{2212}'.len_utf8()..]));
    let after_word = text[..i]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == ',');
    begins && !after_word
}

/// The length of the number and unit at the start of `s`, with the widest
/// reading of that unit; `None` for a number without a known unit.
fn reading_len(s: &str) -> Option<(usize, &'static str, bool)> {
    let sign = s.chars().next().filter(|c| !c.is_ascii_digit());
    let start = sign.map_or(0, char::len_utf8);
    let digits = s[start..]
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(s.len() - start);
    let mut end = start + digits;
    while s[..end].ends_with(['.', ',']) {
        end -= 1;
    }
    let (len, widest, right) = unit_after(&s[end..])?;
    let mut len = end + len;
    if widest == "23 h 59 min" {
        while let Some(more) = duration_part(&s[len..]) {
            len += more;
        }
    }
    Some((len, widest, right))
}

fn unit_after(s: &str) -> Option<(usize, &'static str, bool)> {
    let gap = usize::from(s.starts_with(' '));
    SUFFIXES.iter().find_map(|&(suffix, widest, right)| {
        let after = s[gap..].strip_prefix(suffix)?;
        let bounded = !after.chars().next().is_some_and(char::is_alphanumeric);
        bounded.then_some((gap + suffix.len(), widest, right))
    })
}

/// ` 05 min` or ` 4 h` continuing a duration.
fn duration_part(s: &str) -> Option<usize> {
    let rest = s.strip_prefix(' ')?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits == 0 {
        return None;
    }
    let (len, widest, _) = unit_after(&rest[digits..])?;
    (widest == "23 h 59 min").then_some(1 + digits + len)
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
        assert_eq!(format_in(0.0, Unit::NetworkRate, metric), "0.0 KB/s");
        assert_eq!(format_in(512.0, Unit::NetworkRate, metric), "0.5 KB/s");
        assert_eq!(
            format_in(20.0 * 1024.0, Unit::NetworkRate, metric),
            "20 KB/s"
        );
        assert_eq!(
            format_in(1_030_000.0, Unit::NetworkRate, metric),
            "1.0 MB/s"
        );
        assert_eq!(format_in(45.0, Unit::Celsius, metric), "45°C");
        let other = Units {
            temperature: TemperatureUnit::Fahrenheit,
            network: RateUnit::Bits,
            memory: MemoryUnit::Bytes,
        };
        assert_eq!(format_in(45.0, Unit::Celsius, other), "113°F");
        assert_eq!(format_in(-40.0, Unit::Celsius, other), "-40°F");
        assert_eq!(format_in(1_000_000.0, Unit::NetworkRate, other), "8.0 Mbps");
        assert_eq!(format_in(0.0, Unit::NetworkRate, other), "0.0 Kbps");
        assert_eq!(format_in(62.5, Unit::NetworkRate, other), "0.5 Kbps");
        assert_eq!(format_in(100.0, Unit::NetworkRate, other), "0.8 Kbps");
        assert_eq!(format_in(2_500.0, Unit::NetworkRate, other), "20 Kbps");
        assert_eq!(format_in(37_500.0, Unit::NetworkRate, other), "300 Kbps");
        assert_eq!(format_in(125_000.0, Unit::NetworkRate, other), "1.0 Mbps");
        assert_eq!(format_in(2_500_000.0, Unit::NetworkRate, other), "20 Mbps");
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
    fn readings_widen_to_their_unit_and_leave_plain_text() {
        assert_eq!(widest_like("0 fps"), "9999 fps");
        assert_eq!(widest_like("4.2 GB of 16 GB"), "99.9 GB of 99.9 GB");
        assert_eq!(
            widest_like("↓ 2.0 KB/s  ↑ 0.5 KB/s"),
            "↓ 99.9 MB/s  ↑ 99.9 MB/s"
        );
        assert_eq!(widest_like("45% · 7.2 GB"), "100% · 99.9 GB");
        assert_eq!(widest_like("2 h 05 min"), "23 h 59 min");
        assert_eq!(widest_like("1 d 4 h"), "23 h 59 min");
        assert_eq!(widest_like("51.3 Wh"), "999.9 Wh");
        assert_eq!(widest_like("-5°F"), "390°F");
        assert_eq!(widest_like("RTX 4090"), "RTX 4090");
        assert_eq!(widest_like("1 · Intel"), "1 · Intel");
        assert_eq!(widest_like("12345"), "12345");
        let s = spans("↓ —  ↑ 1.0 Mbps");
        assert_eq!(
            s.iter().map(|s| (s.text, s.reading)).collect::<Vec<_>>(),
            [
                ("↓ ", false),
                ("—", true),
                ("  ↑ ", false),
                ("1.0 Mbps", true)
            ]
        );
        assert!(s[3].right && !s[1].right);
    }

    #[test]
    fn every_formatted_reading_is_one_span_no_wider_than_its_widest() {
        let units = [
            Unit::Percent,
            Unit::Bytes,
            Unit::BytesPerSec,
            Unit::NetworkRate,
            Unit::Millis,
            Unit::MHz,
            Unit::Celsius,
            Unit::Rpm,
            Unit::Watts,
            Unit::Volts,
            Unit::WattHours,
            Unit::Seconds,
            Unit::FramesPerSec,
        ];
        let mut v: f64 = 0.0;
        while v < 1e9 {
            for unit in units {
                if unit == Unit::MHz && v >= 9_995.0 {
                    continue;
                }
                let text = format_value(v, unit);
                let s = spans(&text);
                assert_eq!(s.len(), 1, "{text:?}");
                let widest = s[0].widest.expect(&text);
                assert!(
                    text.chars().count() <= widest.chars().count(),
                    "{text:?} is wider than {widest:?}"
                );
            }
            v = v * 1.3 + 0.7;
        }
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
            Unit::Volts,
            Unit::FramesPerSec,
        ];
        // Zero, and just below where a reading drops a decimal or gains a digit.
        let edges = [0.0, 9.96, 9.9996, 99.96, 999.6, 9999.6];
        let mut v: f64 = 0.0;
        for v in std::iter::from_fn(|| {
            v = v * 1.07 + 1.0;
            (v < 1e13).then_some(v)
        })
        .chain(edges)
        {
            for unit in units {
                if unit == Unit::MHz && v >= 9_995.0 {
                    continue;
                }
                let formatted = format_value(v, unit);
                assert!(
                    formatted.chars().count() <= widest_value(unit).chars().count(),
                    "{formatted:?} is wider than {:?}",
                    widest_value(unit)
                );
            }
        }
    }
}
