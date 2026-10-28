//! Pure mapping from snapshots and histories to what an overlay shows.

use crate::format::format_value;
use crate::history::History;
pub use crate::layout::part_metric as metric_for;
use crate::layout::{CellPart, Rect};
use crate::metric::{ItemKind, Snapshot, Unit};

/// The value shown for a cell; a dash when the source failed or has no data yet.
pub fn cell_value(kind: ItemKind, part: CellPart, snapshot: &Snapshot) -> String {
    let key = metric_for(kind, part);
    match snapshot.get(key) {
        Some(v) if snapshot.item_error(kind).is_none() => format_value(v, key.unit()),
        _ => format_value(f64::NAN, key.unit()),
    }
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
    use crate::metric::{MetricKey, SourceId};

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
