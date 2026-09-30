//! Popup geometry in physical pixels: size from content, position next to
//! the hovered item.

use pulsar_core::layout::{Layout, TextMeasure};
use pulsar_core::metric::ItemKind;

use crate::popup::content::{Block, PopupContent};
use crate::taskbar::placement::Rect32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub scale: f32,
    pub width: f32,
    pub pad: f32,
    /// Body text size and line height.
    pub font_px: f32,
    pub line_h: f32,
    pub title_px: f32,
    pub title_h: f32,
    pub graph_h: f32,
    pub bars_h: f32,
    pub block_gap: f32,
}

impl Metrics {
    /// `line_h` is the measured line height of `font_px` body text.
    pub fn new(dpi: u32, font_px: f32, line_h: f32) -> Self {
        let scale = dpi as f32 / 96.0;
        Self {
            scale,
            width: 300.0 * scale,
            pad: 12.0 * scale,
            font_px,
            line_h,
            title_px: font_px * 1.45,
            title_h: line_h * 1.45,
            graph_h: 72.0 * scale,
            bars_h: 28.0 * scale,
            block_gap: 10.0 * scale,
        }
    }
}

fn block_height(block: &Block, m: &Metrics) -> f32 {
    match block {
        Block::Rows { heading, rows } => {
            heading.map_or(0.0, |_| m.line_h) + rows.len() as f32 * m.line_h
        }
        Block::Bars { values, .. } if values.is_empty() => 0.0,
        Block::Bars { .. } => m.line_h + m.bars_h,
    }
}

pub fn height(content: &PopupContent, m: &Metrics) -> f32 {
    let error = content
        .error
        .as_ref()
        .map_or(0.0, |_| m.line_h + m.block_gap);
    let blocks: f32 = content
        .blocks
        .iter()
        .map(|b| block_height(b, m))
        .filter(|&h| h > 0.0)
        .map(|h| h + m.block_gap)
        .sum();
    (2.0 * m.pad + m.title_h + m.block_gap + m.graph_h + m.block_gap + error + blocks - m.block_gap)
        .ceil()
}

/// Centres the popup over the hovered item, just above the taskbar (below
/// it when the taskbar is at the top), kept inside the monitor.
pub fn position(
    anchor: Rect32,
    taskbar: Rect32,
    monitor: Rect32,
    width: i32,
    height: i32,
    scale: f32,
) -> (i32, i32) {
    let gap = (8.0 * scale).round() as i32;
    let centre = (anchor.left + anchor.right) / 2;
    let x = (centre - width / 2).clamp(
        monitor.left + gap,
        (monitor.right - width - gap).max(monitor.left + gap),
    );
    let above = taskbar.top - height - gap;
    let y = if above >= monitor.top {
        above
    } else {
        taskbar.bottom + gap
    };
    (x, y)
}

/// Screen rectangle covering every cell of `kind` in an overlay whose
/// top-left corner is at `origin`.
pub fn item_screen_rect(layout: &Layout, kind: ItemKind, origin: (i32, i32)) -> Option<Rect32> {
    let cells: Vec<_> = layout.cells.iter().filter(|c| c.kind == kind).collect();
    let first = cells.first()?;
    let (mut l, mut t, mut r, mut b) = (
        first.rect.x,
        first.rect.y,
        first.rect.right(),
        first.rect.y + first.rect.h,
    );
    for c in &cells[1..] {
        l = l.min(c.rect.x);
        t = t.min(c.rect.y);
        r = r.max(c.rect.right());
        b = b.max(c.rect.y + c.rect.h);
    }
    Some(Rect32 {
        left: origin.0 + l.floor() as i32,
        top: origin.1 + t.floor() as i32,
        right: origin.0 + r.ceil() as i32,
        bottom: origin.1 + b.ceil() as i32,
    })
}

/// Shortens `s` with a trailing ellipsis until it fits `max_w`.
pub fn ellipsize(s: &str, max_w: f32, font_px: f32, m: &dyn TextMeasure) -> String {
    if m.width(s, font_px) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
            + "…";
        if m.width(&candidate, font_px) <= max_w {
            return candidate;
        }
    }
    "…".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::layout::CellPart;
    use pulsar_core::metric::ItemKind;

    const MONITOR: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const TASKBAR: Rect32 = Rect32 {
        left: 0,
        top: 1032,
        right: 1920,
        bottom: 1080,
    };

    fn content(blocks: Vec<Block>) -> PopupContent {
        PopupContent {
            kind: ItemKind::Cpu,
            title: "CPU",
            value: "1%".into(),
            series: vec![CellPart::Main],
            error: None,
            blocks,
        }
    }

    fn rows(n: usize) -> Block {
        Block::Rows {
            heading: Some("h"),
            rows: vec![(String::new(), String::new()); n],
        }
    }

    struct Fixed;
    impl TextMeasure for Fixed {
        fn width(&self, text: &str, font_px: f32) -> f32 {
            text.chars().count() as f32 * font_px * 0.5
        }
        fn line_height(&self, font_px: f32) -> f32 {
            font_px * 1.25
        }
    }

    #[test]
    fn item_rect_spans_all_of_an_items_cells() {
        use pulsar_core::config::DisplayMode;
        use pulsar_core::layout::{LayoutInput, compute_layout};
        let input = LayoutInput {
            mode: DisplayMode::Text,
            taskbar_height_px: 48.0,
            dpi: 96,
            font_size_pt: 9.0,
            short_labels: false,
        };
        let specs: Vec<_> = [ItemKind::Cpu, ItemKind::Network]
            .into_iter()
            .map(|kind| pulsar_core::layout::ItemSpec {
                kind,
                parts: pulsar_core::layout::default_parts(kind),
            })
            .collect();
        let l = compute_layout(&specs, input, &Fixed);
        let net = item_screen_rect(&l, ItemKind::Network, (1000, 1032)).unwrap();
        let cells: Vec<_> = l
            .cells
            .iter()
            .filter(|c| c.kind == ItemKind::Network)
            .collect();
        assert_eq!(
            net.top,
            1032 + cells[0].rect.y.floor() as i32,
            "starts at the ↓ cell"
        );
        assert_eq!(
            net.bottom,
            1032 + (cells[1].rect.y + cells[1].rect.h).ceil() as i32,
            "ends at the ↑ cell"
        );
        assert!(item_screen_rect(&l, ItemKind::Gpu, (0, 0)).is_none());
    }

    #[test]
    fn ellipsize_keeps_short_text() {
        assert_eq!(ellipsize("Realtek", 100.0, 10.0, &Fixed), "Realtek");
    }

    #[test]
    fn ellipsize_trims_to_fit() {
        // 5 px per char: 40 px fits 8 chars including the ellipsis.
        let s = ellipsize("Realtek Gaming 2.5GbE", 40.0, 10.0, &Fixed);
        assert_eq!(s, "Realtek…");
        assert!(Fixed.width(&s, 10.0) <= 40.0);
    }

    #[test]
    fn ellipsize_degrades_to_ellipsis() {
        assert_eq!(ellipsize("abc", 1.0, 10.0, &Fixed), "…");
    }

    #[test]
    fn height_grows_by_one_line_per_row() {
        let m = Metrics::new(96, 12.0, 16.0);
        let a = height(&content(vec![rows(2)]), &m);
        let b = height(&content(vec![rows(5)]), &m);
        assert_eq!(b - a, 3.0 * 16.0);
    }

    #[test]
    fn empty_bars_take_no_space() {
        let m = Metrics::new(96, 12.0, 16.0);
        let without = height(&content(vec![rows(1)]), &m);
        let with = height(
            &content(vec![
                rows(1),
                Block::Bars {
                    heading: "Cores",
                    values: vec![],
                },
            ]),
            &m,
        );
        assert_eq!(with, without);
    }

    #[test]
    fn error_adds_a_line() {
        let m = Metrics::new(96, 12.0, 16.0);
        let mut c = content(vec![rows(1)]);
        let before = height(&c, &m);
        c.error = Some("GPU counters unavailable".into());
        assert_eq!(height(&c, &m) - before, 16.0 + 10.0);
    }

    #[test]
    fn metrics_scale_with_dpi() {
        let a = Metrics::new(96, 12.0, 16.0);
        let b = Metrics::new(192, 24.0, 32.0);
        assert_eq!(b.width, 2.0 * a.width);
        let (ha, hb) = (
            height(&content(vec![rows(3)]), &a),
            height(&content(vec![rows(3)]), &b),
        );
        assert!(
            (hb - 2.0 * ha).abs() <= 2.0,
            "{ha} vs {hb} (rounded up to whole pixels)"
        );
    }

    #[test]
    fn sits_above_the_taskbar_centred_on_the_item() {
        let anchor = Rect32 {
            left: 1400,
            top: 1036,
            right: 1444,
            bottom: 1076,
        };
        assert_eq!(
            position(anchor, TASKBAR, MONITOR, 300, 200, 1.0),
            (1422 - 150, 1032 - 200 - 8)
        );
    }

    #[test]
    fn stays_inside_the_monitor() {
        let near_right = Rect32 {
            left: 1900,
            top: 1036,
            right: 1916,
            bottom: 1076,
        };
        assert_eq!(
            position(near_right, TASKBAR, MONITOR, 300, 200, 1.0).0,
            1920 - 300 - 8
        );
        let near_left = Rect32 {
            left: 2,
            top: 1036,
            right: 20,
            bottom: 1076,
        };
        assert_eq!(position(near_left, TASKBAR, MONITOR, 300, 200, 1.0).0, 8);
    }

    #[test]
    fn opens_below_a_top_taskbar() {
        let top_bar = Rect32 {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 48,
        };
        let anchor = Rect32 {
            left: 900,
            top: 4,
            right: 944,
            bottom: 44,
        };
        assert_eq!(position(anchor, top_bar, MONITOR, 300, 200, 1.0).1, 48 + 8);
    }

    #[test]
    fn secondary_monitor_coordinates() {
        let monitor = Rect32 {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let taskbar = Rect32 {
            left: -1920,
            top: 1032,
            right: 0,
            bottom: 1080,
        };
        let anchor = Rect32 {
            left: -30,
            top: 1036,
            right: -10,
            bottom: 1076,
        };
        assert_eq!(
            position(anchor, taskbar, monitor, 300, 200, 1.0),
            (-308, 824)
        );
    }
}
