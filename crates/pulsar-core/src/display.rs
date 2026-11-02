//! Pure mapping from snapshots and histories to what an overlay shows.

use std::borrow::Cow;

use crate::config::NetworkConfig;
use crate::format::format_value;
use crate::history::History;
pub use crate::layout::part_metric as metric_for;
use crate::layout::{CellPart, Rect};
use crate::metric::{ItemKind, MetricKey, Snapshot, Unit};

/// Projects rates below the configured minimums to zero for drawing. Each
/// metric is compared on its own, so a combined rate is judged by its measured
/// sum, whatever its adapters show.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetworkFloor {
    down: f64,
    up: f64,
}

impl NetworkFloor {
    pub fn new(config: &NetworkConfig) -> Self {
        Self {
            down: config.min_down_bps as f64,
            up: config.min_up_bps as f64,
        }
    }

    fn threshold(self, key: MetricKey) -> f64 {
        match key {
            MetricKey::NetDownBps | MetricKey::NetAdapterDownBps(_) => self.down,
            MetricKey::NetUpBps | MetricKey::NetAdapterUpBps(_) => self.up,
            _ => 0.0,
        }
    }

    pub fn value(self, key: MetricKey, value: f64) -> f64 {
        let threshold = self.threshold(key);
        if threshold > 0.0 && value < threshold {
            0.0
        } else {
            value
        }
    }

    /// Copies only when a sample changes.
    pub fn series<'a>(self, key: MetricKey, history: &'a History) -> Cow<'a, History> {
        let threshold = self.threshold(key);
        if threshold > 0.0 && history.iter().any(|v| v < threshold) {
            let samples = history.iter().map(|v| self.value(key, v));
            Cow::Owned(History::from_samples(history.capacity(), samples))
        } else {
            Cow::Borrowed(history)
        }
    }
}

/// The value shown for a cell; a dash when the source failed or has no data yet.
pub fn cell_value(kind: ItemKind, part: CellPart, snapshot: &Snapshot) -> String {
    floored_cell_value(kind, part, snapshot, NetworkFloor::default())
}

pub fn floored_cell_value(
    kind: ItemKind,
    part: CellPart,
    snapshot: &Snapshot,
    floor: NetworkFloor,
) -> String {
    let key = metric_for(kind, part);
    match snapshot.get(key) {
        Some(v) if snapshot.item_error(kind).is_none() => {
            format_value(floor.value(key, v), key.unit())
        }
        _ => format_value(f64::NAN, key.unit()),
    }
}

pub fn drive_temperature(snapshot: &Snapshot, number: u8) -> Option<String> {
    let temp = snapshot
        .get(MetricKey::DiskTempC(number))
        .map(|t| format_value(t, Unit::Celsius));
    let asleep = snapshot.get(MetricKey::DiskAsleep(number)).is_some();
    let minutes = snapshot
        .get(MetricKey::DiskTempAgeSeconds(number))
        .map(|seconds| (seconds / 60.0) as u32);
    Some(match (temp, asleep, minutes) {
        (None, true, _) => crate::tr!("Asleep").to_string(),
        (None, false, _) => return None,
        (Some(t), true, None | Some(0)) => crate::tr!("{} · asleep", t),
        (Some(t), true, Some(m)) => crate::tr!("{} · asleep, read {} min ago", t, m),
        (Some(t), false, None) => t,
        (Some(t), false, Some(0)) => crate::tr!("{} · just now", t),
        (Some(t), false, Some(m)) => crate::tr!("{} · {} min ago", t, m),
    })
}

/// Graph ceiling: percentages are fixed at 100; everything else scales to
/// the recent peak with headroom, never below a floor so idle noise stays flat.
pub fn graph_max(unit: Unit, histories: &[&History]) -> f64 {
    let floor = match unit {
        Unit::Percent | Unit::Celsius => return 100.0,
        Unit::Rpm => 1000.0,
        Unit::Watts => 50.0,
        Unit::Volts => 1.5,
        Unit::BytesPerSec | Unit::NetworkRate => 16.0 * 1024.0,
        Unit::Millis => 100.0,
        Unit::FramesPerSec => 60.0,
        Unit::Bytes
        | Unit::MHz
        | Unit::Ratio
        | Unit::Cycles
        | Unit::WattHours
        | Unit::Seconds
        | Unit::Count => 1.0,
    };
    let peak = histories.iter().filter_map(|h| h.max()).fold(0.0, f64::max);
    (peak * 1.1).max(floor)
}

/// Polygon for an area graph: the newest sample sits at the right edge and
/// each slot is `rect.w / (capacity - 1)` wide. Missing samples draw as zero.
/// Returns the outline from bottom-left, across the samples, to bottom-right.
pub fn area_points(history: &History, rect: Rect, max: f64) -> Vec<(f32, f32)> {
    let n = history.len();
    if n == 0 || max <= 0.0 {
        return Vec::new();
    }
    let step = rect.w / (history.capacity().max(2) - 1) as f32;
    let bottom = rect.y + rect.h;
    let x_of = |i: usize| rect.x + rect.w - (n - 1 - i) as f32 * step;
    let mut points = Vec::with_capacity(n + 2);
    points.push((x_of(0), bottom));
    for (i, v) in history.iter().enumerate() {
        let v = if v.is_nan() {
            0.0
        } else {
            (v / max).clamp(0.0, 1.0)
        };
        points.push((x_of(i), bottom - v as f32 * rect.h));
    }
    points.push((x_of(n - 1), bottom));
    points
}

/// A bar per recent sample, newest at the right edge, each at least `min_w`
/// wide with `gap` between; older samples that do not fit are left out.
/// Missing samples draw no bar. Returns each bar and its value.
pub fn bar_rects(
    history: &History,
    rect: Rect,
    max: f64,
    min_w: f32,
    gap: f32,
) -> Vec<(Rect, f64)> {
    if history.is_empty() || max <= 0.0 || rect.w <= 0.0 {
        return Vec::new();
    }
    let slots = ((rect.w + gap) / (min_w + gap)).floor().max(1.0) as usize;
    let slots = slots.min(history.capacity().max(1));
    let width = (rect.w + gap) / slots as f32 - gap;
    let values: Vec<f64> = history.iter().collect();
    let shown = &values[values.len().saturating_sub(slots)..];
    let bottom = rect.y + rect.h;
    shown
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_nan())
        .map(|(i, &v)| {
            let from_right = (shown.len() - 1 - i) as f32;
            let x = rect.x + rect.w - width - from_right * (width + gap);
            let h = (v / max).clamp(0.0, 1.0) as f32 * rect.h;
            (
                Rect {
                    x,
                    y: bottom - h,
                    w: width,
                    h,
                },
                v,
            )
        })
        .collect()
}

/// How full a level meter is for `value` against `max`, 0–1.
pub fn meter_fraction(value: f64, max: f64) -> f32 {
    if max <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    (value / max).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::SourceId;

    fn floor(down: u64, up: u64) -> NetworkFloor {
        NetworkFloor::new(&NetworkConfig {
            min_down_bps: down,
            min_up_bps: up,
            ..NetworkConfig::default()
        })
    }

    #[test]
    fn a_rate_below_the_floor_shows_as_zero() {
        let floor = floor(10 * 1024, 0);
        for (rate, shown) in [
            (300.0, "0 B/s"),
            (4.0 * 1024.0, "0 B/s"),
            (25.0 * 1024.0, "25.0 KB/s"),
        ] {
            let mut s = Snapshot::default();
            s.set(MetricKey::NetDownBps, rate);
            let value = floored_cell_value(ItemKind::Network, CellPart::Down, &s, floor);
            assert_eq!(value, shown, "{rate}");
        }
    }

    #[test]
    fn a_rate_equal_to_the_floor_shows() {
        let floor = floor(1000, 1000);
        assert_eq!(floor.value(MetricKey::NetDownBps, 1000.0), 1000.0);
        assert_eq!(floor.value(MetricKey::NetDownBps, 999.0), 0.0);
        assert_eq!(floor.value(MetricKey::NetAdapterUpBps(2), 1000.0), 1000.0);
        assert_eq!(floor.value(MetricKey::NetAdapterUpBps(2), 999.9), 0.0);
    }

    #[test]
    fn each_direction_has_its_own_floor() {
        let floor = floor(1000, 100);
        assert_eq!(floor.value(MetricKey::NetDownBps, 500.0), 0.0);
        assert_eq!(floor.value(MetricKey::NetUpBps, 500.0), 500.0);
        assert_eq!(floor.value(MetricKey::NetAdapterDownBps(0), 500.0), 0.0);
        assert_eq!(floor.value(MetricKey::NetAdapterUpBps(0), 500.0), 500.0);
    }

    #[test]
    fn a_combined_rate_is_compared_as_the_sum() {
        let floor = floor(10_000, 0);
        let mut s = Snapshot::default();
        s.set(MetricKey::NetAdapterDownBps(0), 6_000.0);
        s.set(MetricKey::NetAdapterDownBps(1), 6_000.0);
        s.set(MetricKey::NetDownBps, 12_000.0);
        let shown = |part| floored_cell_value(ItemKind::Network, part, &s, floor);
        assert_eq!(shown(CellPart::AdapterDown(0)), "0 B/s");
        assert_eq!(shown(CellPart::AdapterDown(1)), "0 B/s");
        assert_eq!(shown(CellPart::Down), "11.7 KB/s");
    }

    #[test]
    fn a_zero_floor_changes_nothing() {
        let floor = NetworkFloor::default();
        assert_eq!(floor.value(MetricKey::NetDownBps, 0.5), 0.5);
        let h = history(&[0.5, f64::NAN, 2.0], 5);
        assert!(matches!(
            floor.series(MetricKey::NetUpBps, &h),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn only_network_rates_have_a_floor() {
        let floor = floor(1000, 1000);
        assert_eq!(floor.value(MetricKey::CpuTotal, 50.0), 50.0);
        assert_eq!(floor.value(MetricKey::GpuTempC, -5.0), -5.0);
        assert_eq!(floor.value(MetricKey::NetDownTotalBytes, 50.0), 50.0);
        assert_eq!(floor.value(MetricKey::DiskReadBps, 50.0), 50.0);
    }

    #[test]
    fn a_floored_series_keeps_gaps_and_leaves_the_history_raw() {
        let floor = floor(100, 0);
        let h = history(&[50.0, f64::NAN, 100.0, 150.0], 8);
        let floored = floor.series(MetricKey::NetDownBps, &h);
        let values: Vec<f64> = floored.iter().collect();
        assert_eq!(values[0], 0.0);
        assert!(values[1].is_nan());
        assert_eq!(values[2..], [100.0, 150.0]);
        assert_eq!(floored.capacity(), 8);
        assert_eq!(h.iter().next(), Some(50.0));
    }

    #[test]
    fn a_series_with_nothing_below_the_floor_is_not_copied() {
        let h = history(&[100.0, f64::NAN, 200.0], 5);
        assert!(matches!(
            floor(100, 0).series(MetricKey::NetDownBps, &h),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_missing_or_failed_rate_stays_a_dash_under_a_floor() {
        let floor = floor(1000, 0);
        let mut s = Snapshot::default();
        let shown = |s: &Snapshot| floored_cell_value(ItemKind::Network, CellPart::Down, s, floor);
        assert_eq!(shown(&s), "—");
        s.set(MetricKey::NetDownBps, 5.0);
        s.errors
            .insert(SourceId::Network, "counters unavailable".into());
        assert_eq!(shown(&s), "—");
    }

    #[test]
    fn the_history_records_the_rate_as_measured() {
        let mut store = crate::history::HistoryStore::new(5);
        let mut s = Snapshot::default();
        s.set(MetricKey::NetDownBps, 300.0);
        store.record(&s);
        let floor = floor(10_000, 0);
        let drawn = floor.series(
            MetricKey::NetDownBps,
            store.get(MetricKey::NetDownBps).unwrap(),
        );
        assert_eq!(drawn.latest(), Some(0.0));
        assert_eq!(
            store.get(MetricKey::NetDownBps).unwrap().latest(),
            Some(300.0)
        );
        assert_eq!(s.get(MetricKey::NetDownBps), Some(300.0));
    }

    #[test]
    fn a_drive_temperature_says_how_recent_it_is() {
        let text = |temp: Option<f64>, asleep, age: Option<f64>| {
            let mut s = Snapshot::default();
            if let Some(temp) = temp {
                s.set(MetricKey::DiskTempC(0), temp);
            }
            if asleep {
                s.set(MetricKey::DiskAsleep(0), 1.0);
            }
            if let Some(age) = age {
                s.set(MetricKey::DiskTempAgeSeconds(0), age);
            }
            drive_temperature(&s, 0)
        };
        let some = |s: &str| Some(s.to_string());
        assert_eq!(text(Some(45.0), false, None), some("45°C"));
        assert_eq!(text(Some(38.0), false, Some(59.0)), some("38°C · just now"));
        assert_eq!(
            text(Some(38.0), false, Some(240.0)),
            some("38°C · 4 min ago")
        );
        assert_eq!(text(Some(38.0), true, Some(30.0)), some("38°C · asleep"));
        assert_eq!(
            text(Some(38.0), true, Some(720.0)),
            some("38°C · asleep, read 12 min ago")
        );
        assert_eq!(text(None, true, None), some("Asleep"));
        assert_eq!(text(None, false, None), None);
    }

    #[test]
    fn bars_fit_the_newest_samples_at_the_right_edge() {
        let h = history(&[10.0, 20.0, 30.0, 40.0, 100.0], 60);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 11.0,
            h: 10.0,
        };
        // 3 px bars with 1 px gaps: (11 + 1) / 4 = 3 bars.
        let bars = bar_rects(&h, rect, 100.0, 3.0, 1.0);
        let values: Vec<f64> = bars.iter().map(|(_, v)| *v).collect();
        assert_eq!(values, [30.0, 40.0, 100.0]);
        let last = bars[2].0;
        assert_eq!((last.x + last.w, last.y, last.h), (11.0, 0.0, 10.0));
        assert_eq!(bars[0].0.x, 0.0);
        assert_eq!(bars[1].0.h, 4.0);
    }

    #[test]
    fn bars_skip_missing_samples_and_empty_histories() {
        let h = history(&[f64::NAN, 50.0], 10);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 10.0,
        };
        assert_eq!(bar_rects(&h, rect, 100.0, 2.0, 1.0).len(), 1);
        assert!(bar_rects(&History::new(5), rect, 100.0, 2.0, 1.0).is_empty());
    }

    #[test]
    fn meter_fraction_is_clamped() {
        assert_eq!(meter_fraction(50.0, 100.0), 0.5);
        assert_eq!(meter_fraction(500.0, 100.0), 1.0);
        assert_eq!(meter_fraction(f64::NAN, 100.0), 0.0);
        assert_eq!(meter_fraction(5.0, 0.0), 0.0);
    }

    fn history(values: &[f64], capacity: usize) -> History {
        let mut h = History::new(capacity);
        for &v in values {
            h.push(v);
        }
        h
    }

    #[test]
    fn cell_value_formats_the_primary_metric() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 23.4);
        s.set(MetricKey::NetUpBps, 2048.0);
        assert_eq!(cell_value(ItemKind::Cpu, CellPart::Main, &s), "23%");
        assert_eq!(cell_value(ItemKind::Network, CellPart::Up, &s), "2.0 KB/s");
    }

    #[test]
    fn cell_value_is_dash_when_missing_or_failed() {
        let mut s = Snapshot::default();
        assert_eq!(cell_value(ItemKind::Gpu, CellPart::Main, &s), "—");
        s.set(MetricKey::GpuUtil, 5.0);
        s.errors
            .insert(SourceId::Gpu, "GPU counters unavailable".into());
        assert_eq!(cell_value(ItemKind::Gpu, CellPart::Main, &s), "—");
    }

    #[test]
    fn a_fallback_reading_shows_despite_the_service_error() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::SensorService, "not installed".into());
        s.set(MetricKey::CpuPowerW, 42.0);
        assert_eq!(cell_value(ItemKind::CpuPower, CellPart::Main, &s), "42 W");
        assert!(s.item_error(ItemKind::CpuPower).is_none());
        assert!(
            s.item_error(ItemKind::CpuTemp).is_some(),
            "no fallback reading"
        );
        s.set(MetricKey::ChipsetTempC, 50.0);
        assert!(
            s.item_error(ItemKind::ChipsetTemp).is_some(),
            "has no fallback"
        );
    }

    #[test]
    fn percent_graphs_are_fixed_at_100() {
        let h = history(&[5.0], 10);
        assert_eq!(graph_max(Unit::Percent, &[&h]), 100.0);
    }

    #[test]
    fn rate_graphs_autoscale_with_floor() {
        let quiet = history(&[10.0, 20.0], 10);
        assert_eq!(graph_max(Unit::BytesPerSec, &[&quiet]), 16.0 * 1024.0);
        let busy = history(&[1_000_000.0], 10);
        assert_eq!(graph_max(Unit::BytesPerSec, &[&quiet, &busy]), 1_100_000.0);
        assert_eq!(graph_max(Unit::Millis, &[]), 100.0);
    }

    #[test]
    fn area_points_anchor_newest_sample_at_right_edge() {
        let h = history(&[0.0, 50.0, 100.0], 5);
        let rect = Rect {
            x: 10.0,
            y: 0.0,
            w: 40.0,
            h: 20.0,
        };
        let p = area_points(&h, rect, 100.0);
        assert_eq!(p.len(), 5);
        assert_eq!(p[0], (30.0, 20.0), "bottom-left under oldest sample");
        assert_eq!(p[1], (30.0, 20.0));
        assert_eq!(p[2], (40.0, 10.0));
        assert_eq!(p[3], (50.0, 0.0));
        assert_eq!(p[4], (50.0, 20.0), "bottom-right");
    }

    #[test]
    fn area_points_clamp_and_zero_missing() {
        let h = history(&[f64::NAN, 250.0], 2);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let p = area_points(&h, rect, 100.0);
        assert_eq!(p[1], (0.0, 10.0));
        assert_eq!(p[2], (10.0, 0.0));
    }

    #[test]
    fn area_points_empty_history() {
        let h = History::new(5);
        assert!(area_points(&h, Rect::default(), 100.0).is_empty());
    }

    #[test]
    fn a_missing_drive_has_no_value() {
        let s = Snapshot::default();
        assert_eq!(
            cell_value(ItemKind::Disk, CellPart::DriveActive(b'X'), &s),
            "—"
        );
    }
}
