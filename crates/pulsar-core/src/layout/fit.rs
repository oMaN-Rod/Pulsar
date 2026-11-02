//! What to show when the taskbar has less room than the items need: every
//! rung of the ladder is narrower than the one before, and past the last one
//! there is no room at all.

use std::time::{Duration, Instant};

use super::{ItemSpec, Layout, LayoutInput, TextMeasure, TextSpacing, compute_layout};
use crate::config::{LabelStyle, RemoveFrom};

pub const GROW_MARGIN_DIP: i32 = 48;
pub const GROW_HOLD: Duration = Duration::from_secs(2);

const COMPACT_FONT_SCALE: f32 = 0.9;
const COMPACT_MIN_FONT_PT: f32 = 6.0;
const COMPACT_ITEM_GAP_DIP: f32 = 4.0;
const COMPACT_LABEL_GAP_DIP: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Full,
    Compact,
    /// Compact, or Full when that is no narrower, less this many items.
    Without(usize),
}

/// How a layout's cells are labelled, which the renderer must match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierStyle {
    pub labels: LabelStyle,
    pub icons: bool,
    pub graph_inline: bool,
}

impl From<&LayoutInput<'_>> for TierStyle {
    fn from(i: &LayoutInput<'_>) -> Self {
        Self {
            labels: i.labels,
            icons: i.icons,
            graph_inline: i.graph_inline,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rung {
    pub tier: Tier,
    pub layout: Layout,
    pub style: TierStyle,
}

impl Rung {
    fn new(tier: Tier, items: &[ItemSpec], input: LayoutInput<'_>, m: &dyn TextMeasure) -> Self {
        Self {
            tier,
            layout: compute_layout(items, input, m),
            style: TierStyle::from(&input),
        }
    }

    pub fn length(&self) -> f32 {
        length(&self.layout)
    }
}

pub fn length(layout: &Layout) -> f32 {
    if layout.vertical {
        layout.height
    } else {
        layout.width
    }
}

fn compact_input(input: LayoutInput<'_>) -> LayoutInput<'_> {
    LayoutInput {
        labels: if input.icons {
            LabelStyle::None
        } else if input.labels == LabelStyle::Full {
            LabelStyle::Short
        } else {
            input.labels
        },
        font_size_pt: (input.font_size_pt * COMPACT_FONT_SCALE)
            .max(COMPACT_MIN_FONT_PT)
            .min(input.font_size_pt),
        text_spacing: TextSpacing {
            item_gap: input.text_spacing.item_gap.min(COMPACT_ITEM_GAP_DIP),
            label_gap: input.text_spacing.label_gap.min(COMPACT_LABEL_GAP_DIP),
            row_gap: input.text_spacing.row_gap,
        },
        ..input
    }
}

/// Stacked tiles are usually narrower than inline ones, but not always.
fn narrowest_compact(items: &[ItemSpec], input: LayoutInput<'_>, m: &dyn TextMeasure) -> Rung {
    let compact = compact_input(input);
    let rung = Rung::new(Tier::Compact, items, compact, m);
    if !input.graph_inline {
        return rung;
    }
    let stacked = LayoutInput {
        graph_inline: false,
        ..compact
    };
    let other = Rung::new(Tier::Compact, items, stacked, m);
    if other.length() < rung.length() {
        other
    } else {
        rung
    }
}

pub fn remaining(items: &[ItemSpec], from: RemoveFrom, count: usize) -> &[ItemSpec] {
    let count = count.min(items.len());
    match from {
        RemoveFrom::Left => &items[count..],
        RemoveFrom::Right => &items[..items.len() - count],
    }
}

/// Full, Compact, then one item fewer at a time. Text mode packs items into
/// columns, so removing one can widen the rest; a step that gains nothing is
/// left out.
pub fn ladder(
    items: &[ItemSpec],
    input: LayoutInput<'_>,
    from: RemoveFrom,
    m: &dyn TextMeasure,
) -> Vec<Rung> {
    let mut rungs = vec![Rung::new(Tier::Full, items, input, m)];
    if items.is_empty() {
        return rungs;
    }
    let compact = narrowest_compact(items, input, m);
    let mut base = input;
    if compact.length() < rungs[0].length() {
        base = LayoutInput {
            labels: compact.style.labels,
            icons: compact.style.icons,
            graph_inline: compact.style.graph_inline,
            ..compact_input(input)
        };
        rungs.push(compact);
    }
    for count in 1..items.len() {
        let rung = Rung::new(Tier::Without(count), remaining(items, from, count), base, m);
        if rung.length() < rungs[rungs.len() - 1].length() {
            rungs.push(rung);
        }
    }
    rungs
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Room {
    /// The taskbar could not be read: stay on the first rung, as if it fits.
    Unknown,
    Known(f32),
}

/// Which rung to show. Shrinks at once; grows only with the next rung's length
/// plus `margin` free for `GROW_HOLD`, so a button appearing and going, or two
/// lengths either side of the room, cannot make it flap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fit {
    rung: usize,
    grow_since: Option<Instant>,
}

impl Fit {
    pub fn rung(&self) -> usize {
        self.rung
    }

    /// A hold has run its course, so `update` may now grow.
    pub fn grow_due(&self, now: Instant) -> bool {
        self.grow_since
            .is_some_and(|since| now.duration_since(since) >= GROW_HOLD)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the rung to show; `lengths.len()` means none fits.
    pub fn update(&mut self, lengths: &[f32], room: Room, margin: f32, now: Instant) -> usize {
        let Room::Known(room) = room else {
            self.reset();
            return 0;
        };
        self.rung = self.rung.min(lengths.len());
        if lengths.get(self.rung).is_some_and(|&l| l > room) {
            self.rung = (self.rung..lengths.len())
                .find(|&i| lengths[i] <= room)
                .unwrap_or(lengths.len());
            self.grow_since = None;
            return self.rung;
        }
        let next = self.rung.checked_sub(1).and_then(|i| lengths.get(i));
        if !next.is_some_and(|&l| l + margin <= room) {
            self.grow_since = None;
            return self.rung;
        }
        let since = *self.grow_since.get_or_insert(now);
        if now.duration_since(since) >= GROW_HOLD {
            self.rung = lengths
                .iter()
                .position(|&l| l + margin <= room)
                .unwrap_or(self.rung);
            self.grow_since = None;
        }
        self.rung
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::config::{Config, DisplayMode};
    use crate::layout::{CellPart, ItemNames, item_specs};
    use crate::metric::ItemKind;

    struct Fixed;
    impl TextMeasure for Fixed {
        fn width(&self, text: &str, font_px: f32) -> f32 {
            text.chars().count() as f32 * font_px * 0.5
        }
        fn line_height(&self, font_px: f32) -> f32 {
            font_px * 1.25
        }
        fn icon_width(&self, _glyph: &str, font_px: f32) -> f32 {
            font_px
        }
    }

    fn names() -> &'static ItemNames {
        static NAMES: LazyLock<ItemNames> =
            LazyLock::new(|| ItemNames::resolve(&Config::default()));
        &NAMES
    }

    fn input(mode: DisplayMode) -> LayoutInput<'static> {
        LayoutInput {
            mode,
            taskbar_height_px: 48.0,
            fit_font: false,
            dpi: 96,
            font_size_pt: 9.0,
            labels: LabelStyle::Full,
            icons: false,
            graph_inline: false,
            vertical_labels: false,
            upload_on_top: false,
            vertical_bar_px: None,
            text_spacing: TextSpacing::default(),
            names: names(),
        }
    }

    fn items() -> Vec<ItemSpec> {
        [
            ItemKind::Cpu,
            ItemKind::Ram,
            ItemKind::Gpu,
            ItemKind::Disk,
            ItemKind::Network,
            ItemKind::Ping,
        ]
        .map(|kind| ItemSpec {
            kind,
            parts: vec![CellPart::Main],
            tile: Default::default(),
            graphic: Default::default(),
        })
        .to_vec()
    }

    fn all_inputs() -> Vec<LayoutInput<'static>> {
        let mut all = Vec::new();
        for mode in [DisplayMode::Graph, DisplayMode::Text] {
            for labels in [LabelStyle::Full, LabelStyle::Short, LabelStyle::None] {
                for icons in [false, true] {
                    for graph_inline in [false, true] {
                        for font_size_pt in [6.0, 9.0, 14.0, 20.0] {
                            for (dpi, bar, vertical) in [
                                (96, 48.0, None),
                                (96, 32.0, None),
                                (144, 72.0, None),
                                (96, 48.0, Some(48.0)),
                                (96, 160.0, Some(160.0)),
                            ] {
                                all.push(LayoutInput {
                                    labels,
                                    icons,
                                    graph_inline,
                                    font_size_pt,
                                    dpi,
                                    taskbar_height_px: bar,
                                    vertical_bar_px: vertical,
                                    ..input(mode)
                                });
                            }
                        }
                    }
                }
            }
        }
        all
    }

    fn kinds(layout: &Layout) -> Vec<ItemKind> {
        let mut kinds: Vec<ItemKind> = Vec::new();
        for c in &layout.cells {
            if kinds.last() != Some(&c.kind) {
                kinds.push(c.kind);
            }
        }
        kinds
    }

    #[test]
    fn lengths_never_increase_down_the_ladder() {
        for from in [RemoveFrom::Left, RemoveFrom::Right] {
            for i in all_inputs() {
                let rungs = ladder(&items(), i, from, &Fixed);
                assert_eq!(rungs[0].tier, Tier::Full);
                for pair in rungs.windows(2) {
                    assert!(
                        pair[1].length() < pair[0].length(),
                        "{:?} {} -> {:?} {} for {i:?}",
                        pair[0].tier,
                        pair[0].length(),
                        pair[1].tier,
                        pair[1].length()
                    );
                }
            }
        }
    }

    #[test]
    fn the_first_rung_is_the_layout_on_its_own() {
        for i in all_inputs() {
            let rungs = ladder(&items(), i, RemoveFrom::Right, &Fixed);
            assert_eq!(rungs[0].layout, compute_layout(&items(), i, &Fixed));
            assert_eq!(rungs[0].style, TierStyle::from(&i));
        }
    }

    #[test]
    fn removes_from_the_right_keeping_the_first_items() {
        let rungs = ladder(
            &items(),
            input(DisplayMode::Graph),
            RemoveFrom::Right,
            &Fixed,
        );
        let last = rungs.last().unwrap();
        assert_eq!(kinds(&last.layout), [ItemKind::Cpu]);
        let without: Vec<Vec<ItemKind>> = rungs
            .iter()
            .filter(|r| matches!(r.tier, Tier::Without(_)))
            .map(|r| kinds(&r.layout))
            .collect();
        for pair in without.windows(2) {
            assert_eq!(pair[0][..pair[0].len() - 1], pair[1][..]);
        }
    }

    #[test]
    fn removes_from_the_left_keeping_the_last_items() {
        let rungs = ladder(
            &items(),
            input(DisplayMode::Graph),
            RemoveFrom::Left,
            &Fixed,
        );
        let last = rungs.last().unwrap();
        assert_eq!(kinds(&last.layout), [ItemKind::Ping]);
        let without: Vec<Vec<ItemKind>> = rungs
            .iter()
            .filter(|r| matches!(r.tier, Tier::Without(_)))
            .map(|r| kinds(&r.layout))
            .collect();
        for pair in without.windows(2) {
            assert_eq!(pair[0][1..], pair[1][..]);
        }
    }

    #[test]
    fn compact_keeps_every_item_and_comes_before_removal() {
        let rungs = ladder(
            &items(),
            input(DisplayMode::Text),
            RemoveFrom::Right,
            &Fixed,
        );
        assert_eq!(rungs[1].tier, Tier::Compact);
        assert_eq!(kinds(&rungs[1].layout), kinds(&rungs[0].layout));
        assert_eq!(rungs[1].style.labels, LabelStyle::Short);
        assert!(matches!(rungs[2].tier, Tier::Without(_)));
    }

    #[test]
    fn compact_drops_label_text_beside_an_icon() {
        let i = LayoutInput {
            icons: true,
            ..input(DisplayMode::Text)
        };
        let rungs = ladder(&items(), i, RemoveFrom::Right, &Fixed);
        assert_eq!(rungs[1].tier, Tier::Compact);
        assert_eq!(rungs[1].style.labels, LabelStyle::None);
        assert!(rungs[1].style.icons);
    }

    #[test]
    fn a_removal_rung_keeps_compact_styling() {
        let rungs = ladder(
            &items(),
            input(DisplayMode::Text),
            RemoveFrom::Right,
            &Fixed,
        );
        assert!(
            rungs[2..]
                .iter()
                .all(|r| r.style.labels == LabelStyle::Short)
        );
    }

    #[test]
    fn inline_tiles_may_stack_when_that_is_narrower() {
        let i = LayoutInput {
            graph_inline: true,
            ..input(DisplayMode::Graph)
        };
        let rungs = ladder(&items(), i, RemoveFrom::Right, &Fixed);
        assert_eq!(rungs[1].tier, Tier::Compact);
        assert!(!rungs[1].style.graph_inline);
        assert_eq!(
            rungs[1].layout,
            compute_layout(
                &items(),
                LayoutInput {
                    graph_inline: false,
                    ..compact_input(i)
                },
                &Fixed
            )
        );
    }

    #[test]
    fn nothing_to_show_is_a_single_empty_rung() {
        let rungs = ladder(&[], input(DisplayMode::Graph), RemoveFrom::Right, &Fixed);
        assert_eq!(rungs.len(), 1);
        assert_eq!(rungs[0].length(), 0.0);
    }

    #[test]
    fn real_config_items_ladder_down_to_one() {
        let config = Config::default();
        let specs = item_specs(&config, &crate::gpu_slots::GpuSlots::new());
        let rungs = ladder(&specs, input(DisplayMode::Graph), RemoveFrom::Right, &Fixed);
        let last = rungs.last().unwrap();
        assert_eq!(last.layout.cells.len(), 1);
        assert!(last.layout.cells[0].part == CellPart::Main);
    }

    const LENGTHS: [f32; 4] = [400.0, 300.0, 200.0, 100.0];
    const MARGIN: f32 = 48.0;

    fn fit_at(rung: usize) -> Fit {
        Fit {
            rung,
            grow_since: None,
        }
    }

    #[test]
    fn exact_room_fits_each_rung() {
        for (rung, &length) in LENGTHS.iter().enumerate() {
            let mut fit = Fit::default();
            let now = Instant::now();
            assert_eq!(
                fit.update(&LENGTHS, Room::Known(length), MARGIN, now),
                rung,
                "{length}"
            );
        }
    }

    #[test]
    fn one_pixel_short_drops_to_the_next_rung() {
        for (rung, &length) in LENGTHS.iter().enumerate() {
            let mut fit = Fit::default();
            let now = Instant::now();
            assert_eq!(
                fit.update(&LENGTHS, Room::Known(length - 1.0), MARGIN, now),
                rung + 1,
                "{length}"
            );
        }
    }

    #[test]
    fn no_rung_fitting_means_no_room() {
        let mut fit = Fit::default();
        let got = fit.update(&LENGTHS, Room::Known(99.0), MARGIN, Instant::now());
        assert_eq!(got, LENGTHS.len());
    }

    #[test]
    fn shrinks_at_once_and_skips_rungs() {
        let mut fit = Fit::default();
        let now = Instant::now();
        assert_eq!(fit.update(&LENGTHS, Room::Known(250.0), MARGIN, now), 2);
    }

    #[test]
    fn grows_only_after_the_margin_has_held() {
        let t0 = Instant::now();
        let mut fit = fit_at(1);
        let room = Room::Known(400.0 + MARGIN);
        assert_eq!(fit.update(&LENGTHS, room, MARGIN, t0), 1);
        assert!(!fit.grow_due(t0 + GROW_HOLD - Duration::from_millis(1)));
        assert_eq!(
            fit.update(&LENGTHS, room, MARGIN, t0 + Duration::from_millis(1999)),
            1
        );
        assert!(fit.grow_due(t0 + GROW_HOLD));
        assert_eq!(fit.update(&LENGTHS, room, MARGIN, t0 + GROW_HOLD), 0);
        assert!(!fit.grow_due(t0 + GROW_HOLD * 2));
    }

    #[test]
    fn one_pixel_under_the_margin_never_grows() {
        let t0 = Instant::now();
        let mut fit = fit_at(1);
        let room = Room::Known(400.0 + MARGIN - 1.0);
        for s in 0..60 {
            let at = t0 + Duration::from_secs(s);
            assert_eq!(fit.update(&LENGTHS, room, MARGIN, at), 1);
        }
    }

    #[test]
    fn losing_the_margin_restarts_the_hold() {
        let t0 = Instant::now();
        let mut fit = fit_at(1);
        let good = Room::Known(400.0 + MARGIN);
        let bad = Room::Known(400.0);
        fit.update(&LENGTHS, good, MARGIN, t0);
        fit.update(&LENGTHS, bad, MARGIN, t0 + Duration::from_secs(1));
        assert_eq!(
            fit.update(&LENGTHS, good, MARGIN, t0 + Duration::from_millis(2500)),
            1
        );
        assert_eq!(
            fit.update(&LENGTHS, good, MARGIN, t0 + Duration::from_millis(4500)),
            0
        );
    }

    #[test]
    fn a_grow_goes_as_far_as_the_room_allows() {
        let t0 = Instant::now();
        let mut fit = fit_at(3);
        let room = Room::Known(300.0 + MARGIN);
        fit.update(&LENGTHS, room, MARGIN, t0);
        assert_eq!(fit.update(&LENGTHS, room, MARGIN, t0 + GROW_HOLD), 1);
    }

    #[test]
    fn room_returning_after_none_fits_grows_back() {
        let t0 = Instant::now();
        let mut fit = Fit::default();
        fit.update(&LENGTHS, Room::Known(10.0), MARGIN, t0);
        assert_eq!(fit.rung(), LENGTHS.len());
        let room = Room::Known(100.0 + MARGIN);
        assert_eq!(fit.update(&LENGTHS, room, MARGIN, t0), LENGTHS.len());
        assert_eq!(fit.update(&LENGTHS, room, MARGIN, t0 + GROW_HOLD), 3);
    }

    #[test]
    fn room_oscillating_around_a_rung_does_not_flap() {
        let t0 = Instant::now();
        let mut fit = Fit::default();
        let mut changes = 0;
        let mut last = fit.rung();
        for tick in 0..240u32 {
            let room = if tick % 2 == 0 { 399.0 } else { 401.0 };
            let at = t0 + Duration::from_millis(u64::from(tick) * 500);
            let rung = fit.update(&LENGTHS, Room::Known(room), MARGIN, at);
            if rung != last {
                changes += 1;
                last = rung;
            }
        }
        assert_eq!(changes, 1);
        assert_eq!(last, 1);
    }

    #[test]
    fn a_button_coming_and_going_slowly_changes_at_most_once_per_hold() {
        let t0 = Instant::now();
        let mut fit = Fit::default();
        let mut changes = 0;
        let mut last = fit.rung();
        for tick in 0..120u32 {
            let room = if tick / 3 % 2 == 0 { 390.0 } else { 460.0 };
            let at = t0 + Duration::from_millis(u64::from(tick) * 500);
            let rung = fit.update(&LENGTHS, Room::Known(room), MARGIN, at);
            if rung != last {
                changes += 1;
                last = rung;
            }
        }
        assert_eq!(changes, 1, "1.5 s of room never completes a 2 s hold");
    }

    #[test]
    fn unknown_room_returns_to_the_first_rung_at_once() {
        let mut fit = fit_at(3);
        assert_eq!(
            fit.update(&LENGTHS, Room::Unknown, MARGIN, Instant::now()),
            0
        );
        assert_eq!(fit.rung(), 0);
    }

    #[test]
    fn a_ladder_that_got_shorter_cannot_leave_the_rung_out_of_range() {
        let t0 = Instant::now();
        let mut fit = fit_at(9);
        assert_eq!(fit.update(&LENGTHS, Room::Known(1000.0), MARGIN, t0), 4);
        assert_eq!(
            fit.update(&LENGTHS, Room::Known(1000.0), MARGIN, t0 + GROW_HOLD),
            0
        );
    }
}
