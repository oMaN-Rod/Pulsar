//! Pure mapping from snapshots and histories to what an overlay shows.

use std::borrow::Cow;

use crate::config::NetworkConfig;
use crate::format::format_value;
use crate::history::{History, HistoryStore};
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
pub fn heat_strip_cells(history: &History, rect: Rect, w: f32, gap: f32) -> Vec<(Rect, f64)> {
    bar_rects(history, rect, 1.0, w, gap)
        .into_iter()
        .map(|(bar, value)| {
            let cell = Rect {
                y: rect.y,
                h: rect.h,
                ..bar
            };
            (cell, value)
        })
        .collect()
}

/// The lit cells with their rows from the bottom, and how many rows there are.
pub fn dot_cells(
    history: &History,
    rect: Rect,
    max: f64,
    pitch: f32,
    cell: f32,
    max_rows: usize,
) -> (Vec<(Rect, usize)>, usize) {
    if history.is_empty() || max <= 0.0 || rect.w < cell || rect.h < cell {
        return (Vec::new(), 0);
    }
    let gap = pitch - cell;
    let columns = (((rect.w + gap) / pitch).floor() as usize).min(history.capacity().max(1));
    let rows = (((rect.h + gap) / pitch).floor() as usize).clamp(1, max_rows.max(1));
    let values: Vec<f64> = history.iter().collect();
    let shown = &values[values.len().saturating_sub(columns)..];
    let (right, bottom) = ((rect.x + rect.w).round(), (rect.y + rect.h).round());
    let mut cells = Vec::with_capacity(shown.len() * rows);
    for (from_right, &value) in shown.iter().rev().enumerate() {
        let lit = (meter_fraction(value, max) * rows as f32).round() as usize;
        for row in 0..lit {
            let area = Rect {
                x: right - cell - from_right as f32 * pitch,
                y: bottom - cell - row as f32 * pitch,
                w: cell,
                h: cell,
            };
            cells.push((area, row));
        }
    }
    (cells, rows)
}

pub fn history_range(history: &History) -> Option<(f64, f64)> {
    history
        .iter()
        .filter(|v| !v.is_nan())
        .fold(None, |range, v| match range {
            None => Some((v, v)),
            Some((low, high)) => Some((f64::min(low, v), f64::max(high, v))),
        })
}

pub fn meter_fraction(value: f64, max: f64) -> f32 {
    if max <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    (value / max).clamp(0.0, 1.0) as f32
}

/// Clockwise from twelve o'clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingArc {
    pub start: (f32, f32),
    pub end: (f32, f32),
    pub large: bool,
}

/// `None` when empty. A full ring stops just short, as an arc can't close.
pub fn ring_arc(center: (f32, f32), radius: f32, fraction: f32) -> Option<RingArc> {
    if fraction <= 0.0 {
        return None;
    }
    let fraction = fraction.min(0.999);
    let angle = fraction * std::f32::consts::TAU;
    Some(RingArc {
        start: (center.0, center.1 - radius),
        end: (
            center.0 + radius * angle.sin(),
            center.1 - radius * angle.cos(),
        ),
        large: fraction > 0.5,
    })
}

/// The item whose reading a ring shows inside the item's own.
pub fn ring_companion(kind: ItemKind) -> Option<ItemKind> {
    match kind {
        ItemKind::Cpu => Some(ItemKind::CpuTemp),
        ItemKind::Gpu => Some(ItemKind::GpuTemp),
        _ => None,
    }
}

/// Samples a core's peak mark holds on to.
pub const PEAK_SAMPLES: usize = 10;
/// A peak this close above the load, in percent, adds nothing to the bar.
const PEAK_MARGIN: f64 = 2.0;

/// One core's latest load and its recent peak, 0–100.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreLoad {
    pub load: f64,
    pub peak: f64,
}

pub fn core_loads(snapshot: &Snapshot, history: &HistoryStore) -> Vec<CoreLoad> {
    (0..=u16::MAX)
        .map_while(|i| {
            let key = MetricKey::CpuCore(i);
            let load = snapshot.get(key)?;
            let peak = history
                .get(key)
                .and_then(|h| h.recent_max(PEAK_SAMPLES))
                .unwrap_or(load);
            Some(CoreLoad {
                load,
                peak: peak.max(load),
            })
        })
        .collect()
}

/// One core's bar and track, with where its peak mark goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreBar {
    pub track: Rect,
    pub fill: Rect,
    pub load: f64,
    pub peak_y: Option<f32>,
    pub efficient: bool,
}

fn busiest(loads: &[CoreLoad]) -> CoreLoad {
    loads.iter().fold(
        CoreLoad {
            load: 0.0,
            peak: 0.0,
        },
        |most, l| CoreLoad {
            load: most.load.max(l.load),
            peak: most.peak.max(l.peak),
        },
    )
}

/// A whole-pixel bar per core, or per group of cores with the busiest shown
/// when `area` can't give each its own. Keeps a sliver at idle. Cores from
/// `split` on are efficient ones, set apart by an empty slot.
pub fn core_bars(
    loads: &[CoreLoad],
    area: Rect,
    split: Option<usize>,
    gap: f32,
    min_fill: f32,
) -> Vec<CoreBar> {
    if loads.is_empty() || area.w < 1.0 || area.h < 1.0 {
        return Vec::new();
    }
    let n = loads.len();
    let gap = gap.round().max(1.0);
    let room = ((area.w + gap) / (1.0 + gap)).floor() as usize;
    let split = split.filter(|&s| s > 0 && s < n && room >= 3);
    let spacer = usize::from(split.is_some());
    let bars = room.saturating_sub(spacer).clamp(1, n);
    let slots = bars + spacer;
    let pitch = ((area.w + gap) / slots as f32).floor().max(1.0 + gap);
    let width = pitch - gap;
    let used = slots as f32 * pitch - gap;
    let left = area.x + ((area.w - used) / 2.0).max(0.0).floor();
    let bottom = area.y + area.h;
    // Each type is grouped on its own, so a bar never mixes the two.
    let first_efficient = split.map(|s| ((s * bars + n / 2) / n).clamp(1, bars - 1));
    let group = |bar: usize| match (first_efficient, split) {
        (Some(first), Some(s)) if bar < first => (bar * s / first)..((bar + 1) * s / first),
        (Some(first), Some(s)) => {
            let (i, count) = (bar - first, bars - first);
            (s + i * (n - s) / count)..(s + (i + 1) * (n - s) / count)
        }
        _ => (bar * n / bars)..((bar + 1) * n / bars),
    };
    (0..bars)
        .map(|bar| {
            let core = busiest(&loads[group(bar)]);
            let efficient = first_efficient.is_some_and(|first| bar >= first);
            let x = left + (bar + usize::from(efficient)) as f32 * pitch;
            let h = (meter_fraction(core.load, 100.0) * area.h)
                .round()
                .clamp(min_fill.min(area.h), area.h);
            let peak_y = (core.peak >= core.load + PEAK_MARGIN).then(|| {
                let peak = (meter_fraction(core.peak, 100.0) * area.h).round();
                (bottom - peak).clamp(area.y, bottom - 1.0)
            });
            CoreBar {
                track: Rect {
                    x,
                    y: area.y,
                    w: width,
                    h: area.h,
                },
                fill: Rect {
                    x,
                    y: bottom - h,
                    w: width,
                    h,
                },
                load: core.load,
                peak_y,
                efficient,
            }
        })
        .collect()
}

/// A cell per core, filled row by row, or per group of cores with the busiest
/// shown when `area` can't hold them all.
pub fn heat_cells(loads: &[f64], area: Rect, rows: usize, gap: f32) -> Vec<(Rect, f64)> {
    if loads.is_empty() || rows == 0 || area.w < 1.0 || area.h < 1.0 {
        return Vec::new();
    }
    let n = loads.len();
    let gap = gap.round().max(1.0);
    let most_columns = (((area.w + gap) / (2.0 + gap)).floor() as usize).max(1);
    let cells = n.min(most_columns * rows);
    let columns = cells.div_ceil(rows);
    let pitch_x = ((area.w + gap) / columns as f32).floor();
    let pitch_y = ((area.h + gap) / rows as f32).floor();
    let used = columns as f32 * pitch_x - gap;
    let left = area.x + ((area.w - used) / 2.0).max(0.0).floor();
    (0..cells)
        .map(|cell| {
            let group = &loads[cell * n / cells..(cell + 1) * n / cells];
            let rect = Rect {
                x: left + (cell % columns) as f32 * pitch_x,
                y: area.y + (cell / columns) as f32 * pitch_y,
                w: (pitch_x - gap).max(1.0),
                h: (pitch_y - gap).max(1.0),
            };
            (rect, group.iter().copied().fold(0.0, f64::max))
        })
        .collect()
}

/// Whole-pixel segments across `area`, at most `most` of them.
pub fn segment_rects(area: Rect, most: usize, gap: f32) -> Vec<Rect> {
    if area.w < 1.0 || area.h < 1.0 || most == 0 {
        return Vec::new();
    }
    let gap = gap.round().max(1.0);
    let count = (((area.w + gap) / (2.0 + gap)).floor() as usize).clamp(1, most);
    let pitch = ((area.w + gap) / count as f32).floor();
    let used = count as f32 * pitch - gap;
    let left = area.x + ((area.w - used) / 2.0).max(0.0).floor();
    (0..count)
        .map(|i| Rect {
            x: left + i as f32 * pitch,
            w: pitch - gap,
            ..area
        })
        .collect()
}

/// How many of `count` segments are lit; any reading above zero lights one.
pub fn lit_segments(fraction: f32, count: usize) -> usize {
    if fraction <= 0.0 {
        return 0;
    }
    ((fraction * count as f32).round() as usize).clamp(1, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::SourceId;

    const AREA: Rect = Rect {
        x: 10.0,
        y: 5.0,
        w: 71.0,
        h: 30.0,
    };

    fn steady(loads: &[f64]) -> Vec<CoreLoad> {
        loads
            .iter()
            .map(|&load| CoreLoad { load, peak: load })
            .collect()
    }

    #[test]
    fn a_core_gets_a_bar_of_its_own_while_they_fit() {
        let loads = steady(&[50.0; 24]);
        let bars = core_bars(&loads, AREA, None, 1.0, 1.0);
        assert_eq!(bars.len(), 24);
        assert!(bars.iter().all(|b| b.track.w == 2.0));
        assert_eq!(bars[1].track.x - bars[0].track.x, 3.0);
        assert_eq!(bars[23].track.x + bars[23].track.w, AREA.right());
        assert!(bars.iter().all(|b| b.fill.h == 15.0));
    }

    #[test]
    fn too_many_cores_share_bars_and_show_the_busiest() {
        let mut loads = steady(&[0.0; 256]);
        loads[255].load = 90.0;
        loads[255].peak = 90.0;
        let bars = core_bars(&loads, AREA, None, 1.0, 1.0);
        assert_eq!(bars.len(), 36);
        assert_eq!(bars.last().unwrap().load, 90.0);
        assert_eq!(bars[0].load, 0.0);
        assert!(bars.last().unwrap().track.right() <= AREA.right());
    }

    #[test]
    fn a_few_cores_stay_inside_the_area_and_idle_keeps_a_sliver() {
        let loads = steady(&[0.0, 100.0, f64::NAN, 400.0]);
        let bars = core_bars(&loads, AREA, None, 1.0, 1.0);
        assert_eq!(bars.len(), 4);
        assert_eq!(bars[0].fill.h, 1.0);
        assert_eq!(bars[1].fill.h, AREA.h);
        assert_eq!(bars[2].fill.h, 1.0);
        assert_eq!(bars[3].fill.h, AREA.h);
        assert!(bars.iter().all(|b| b.fill.y + b.fill.h == 35.0));
        assert!(bars[0].track.x >= AREA.x && bars[3].track.right() <= AREA.right());
    }

    #[test]
    fn no_cores_or_no_room_draws_nothing() {
        assert!(core_bars(&[], AREA, None, 1.0, 1.0).is_empty());
        let none = Rect { w: 0.0, ..AREA };
        assert!(core_bars(&steady(&[50.0]), none, None, 1.0, 1.0).is_empty());
    }

    #[test]
    fn efficient_cores_sit_apart_after_an_empty_slot() {
        let area = Rect { w: 74.0, ..AREA };
        let bars = core_bars(&steady(&[50.0; 24]), area, Some(16), 1.0, 1.0);
        assert_eq!(bars.len(), 24);
        assert!(!bars[15].efficient && bars[16].efficient);
        assert_eq!(bars[1].track.x - bars[0].track.x, 3.0);
        assert_eq!(bars[16].track.x - bars[15].track.x, 6.0);
        assert!(bars[23].track.right() <= area.right());
    }

    #[test]
    fn a_split_that_means_nothing_adds_no_slot() {
        for split in [None, Some(0), Some(24), Some(99)] {
            let bars = core_bars(&steady(&[50.0; 24]), AREA, split, 1.0, 1.0);
            assert!(bars.iter().all(|b| !b.efficient), "{split:?}");
            assert_eq!(bars[1].track.x - bars[0].track.x, 3.0);
        }
    }

    #[test]
    fn grouped_cores_keep_the_split_and_stay_in_the_area() {
        let bars = core_bars(&steady(&[50.0; 256]), AREA, Some(192), 1.0, 1.0);
        assert!(bars.len() < 256);
        assert!(bars.iter().any(|b| b.efficient));
        assert!(bars.iter().any(|b| !b.efficient));
        assert!(bars.iter().all(|b| b.track.right() <= AREA.right()));
    }

    #[test]
    fn a_grouped_bar_never_mixes_core_types() {
        let area = Rect { w: 41.0, ..AREA };
        let mut loads = steady(&[0.0; 24]);
        loads[16].load = 90.0;
        loads[16].peak = 90.0;
        let bars = core_bars(&loads, area, Some(16), 1.0, 1.0);
        assert!(bars.len() < 24);
        assert!(bars.iter().filter(|b| b.load > 0.0).all(|b| b.efficient));
        let mut loads = steady(&[0.0; 24]);
        loads[15].load = 90.0;
        loads[15].peak = 90.0;
        let bars = core_bars(&loads, area, Some(16), 1.0, 1.0);
        assert!(bars.iter().filter(|b| b.load > 0.0).all(|b| !b.efficient));
    }

    #[test]
    fn a_peak_marks_its_height_only_when_above_the_load() {
        let loads = [
            CoreLoad {
                load: 20.0,
                peak: 80.0,
            },
            CoreLoad {
                load: 50.0,
                peak: 51.0,
            },
            CoreLoad {
                load: 0.0,
                peak: 100.0,
            },
        ];
        let bars = core_bars(&loads, AREA, None, 1.0, 1.0);
        assert_eq!(bars[0].peak_y, Some(AREA.y + 6.0));
        assert_eq!(bars[1].peak_y, None);
        assert_eq!(bars[2].peak_y, Some(AREA.y));
    }

    #[test]
    fn peaks_come_from_recent_samples_only() {
        let mut store = HistoryStore::new(60);
        for v in [
            90.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 20.0, 30.0,
        ] {
            let mut s = Snapshot::default();
            s.set(MetricKey::CpuCore(0), v);
            store.record(&s);
        }
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuCore(0), 5.0);
        let loads = core_loads(&s, &store);
        assert_eq!(loads[0].load, 5.0);
        assert_eq!(loads[0].peak, 30.0);
    }

    #[test]
    fn a_core_without_history_peaks_at_its_load() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuCore(0), 42.0);
        let loads = core_loads(&s, &HistoryStore::new(60));
        assert_eq!(loads[0].peak, 42.0);
    }

    #[test]
    fn heat_cells_fill_rows_and_group_what_will_not_fit() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 30.0,
        };
        let cells = heat_cells(&[10.0; 24], area, 3, 1.0);
        assert_eq!(cells.len(), 24);
        assert_eq!(cells[8].0.y, cells[0].0.y + 10.0);
        assert_eq!(cells[8].0.x, cells[0].0.x);
        assert!(
            cells
                .iter()
                .all(|(r, _)| r.right() <= 80.0 && r.y + r.h <= 30.0)
        );
        let mut many = vec![0.0; 2000];
        many[1999] = 70.0;
        let grouped = heat_cells(&many, area, 3, 1.0);
        assert!(grouped.len() < 2000);
        assert_eq!(grouped.last().unwrap().1, 70.0);
        assert!(heat_cells(&[], area, 3, 1.0).is_empty());
        assert!(heat_cells(&[1.0], area, 0, 1.0).is_empty());
    }

    #[test]
    fn segments_snap_to_pixels_and_light_in_steps() {
        let rects = segment_rects(AREA, 10, 1.0);
        assert_eq!(rects.len(), 10);
        assert!(
            rects
                .iter()
                .all(|r| r.w == rects[0].w && r.w.fract() == 0.0)
        );
        assert!(rects[9].right() <= AREA.right());
        let narrow = Rect { w: 8.0, ..AREA };
        assert_eq!(segment_rects(narrow, 10, 1.0).len(), 3);
        assert_eq!(lit_segments(0.0, 10), 0);
        assert_eq!(lit_segments(0.001, 10), 1);
        assert_eq!(lit_segments(0.38, 10), 4);
        assert_eq!(lit_segments(1.0, 10), 10);
    }

    #[test]
    fn a_ring_runs_clockwise_from_the_top() {
        let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() + (a.1 - b.1).abs() < 0.01;
        assert_eq!(ring_arc((10.0, 10.0), 5.0, 0.0), None);
        let quarter = ring_arc((10.0, 10.0), 5.0, 0.25).unwrap();
        assert!(close(quarter.start, (10.0, 5.0)));
        assert!(close(quarter.end, (15.0, 10.0)));
        assert!(!quarter.large);
        let most = ring_arc((10.0, 10.0), 5.0, 0.75).unwrap();
        assert!(close(most.end, (5.0, 10.0)));
        assert!(most.large);
        let full = ring_arc((10.0, 10.0), 5.0, 1.0).unwrap();
        assert!(full.large && full.end != full.start);
    }

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

    fn history_of(samples: &[f64]) -> History {
        History::from_samples(samples.len().max(1), samples.iter().copied())
    }

    const DOT_AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 15.0,
        h: 15.0,
    };

    #[test]
    fn a_heat_strip_has_a_full_height_cell_per_reading() {
        let area = Rect {
            x: 0.0,
            y: 10.0,
            w: 8.0,
            h: 6.0,
        };
        let cells = heat_strip_cells(&history_of(&[5.0, f64::NAN, 90.0]), area, 2.0, 1.0);
        assert_eq!(cells.len(), 2);
        for (cell, _) in &cells {
            assert_eq!((cell.y, cell.h), (10.0, 6.0));
        }
        assert_eq!(cells[1].1, 90.0);
    }

    #[test]
    fn a_dot_matrix_lights_only_as_many_cells_as_the_load_needs() {
        let history = history_of(&[0.0, 50.0, 100.0]);
        let (cells, rows) = dot_cells(&history, DOT_AREA, 100.0, 4.0, 3.0, 8);
        assert_eq!(rows, 4);
        assert_eq!(cells.len(), 2 + 4);
        let newest = cells[0].0;
        assert_eq!((newest.x, newest.y), (12.0, 12.0));
    }

    #[test]
    fn a_dot_matrix_stops_at_its_row_limit_and_skips_gaps() {
        let (cells, rows) = dot_cells(&history_of(&[100.0]), DOT_AREA, 100.0, 4.0, 3.0, 2);
        assert_eq!((cells.len(), rows), (2, 2));
        let (cells, _) = dot_cells(&history_of(&[f64::NAN]), DOT_AREA, 100.0, 4.0, 3.0, 8);
        assert!(cells.is_empty());
    }

    #[test]
    fn history_range_ignores_gaps() {
        assert_eq!(
            history_range(&history_of(&[40.0, f64::NAN, 10.0, 70.0])),
            Some((10.0, 70.0))
        );
        assert_eq!(history_range(&history_of(&[f64::NAN])), None);
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
