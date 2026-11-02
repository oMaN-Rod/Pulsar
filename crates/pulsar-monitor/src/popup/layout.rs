//! Popup geometry in physical pixels: size from content, position next to
//! the hovered item.

use std::borrow::Cow;

use pulsar_core::config::POPUP_GRAPH_HEIGHTS;
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
    /// A composition's stacked bar.
    pub stack_h: f32,
    pub block_gap: f32,
}

impl Metrics {
    #[cfg(test)]
    pub fn new(dpi: u32, font_px: f32, line_h: f32) -> Self {
        let d = pulsar_core::config::PopupConfig::default();
        Self::with_size(dpi, font_px, line_h, d.width, d.graph_height)
    }

    /// `line_h` is the measured line height of `font_px` body text; `width`
    /// and `graph_h` are logical pixels.
    pub fn with_size(dpi: u32, font_px: f32, line_h: f32, width: u16, graph_h: u16) -> Self {
        let scale = dpi as f32 / 96.0;
        Self {
            scale,
            width: f32::from(width) * scale,
            pad: 12.0 * scale,
            font_px,
            line_h,
            title_px: font_px * 1.45,
            title_h: line_h * 1.45,
            graph_h: f32::from(graph_h) * scale,
            bars_h: 28.0 * scale,
            stack_h: 10.0 * scale,
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
        Block::Composition { parts, .. } => {
            m.line_h + m.stack_h + m.block_gap / 2.0 + parts.len() as f32 * m.line_h
        }
    }
}

pub fn height(content: &PopupContent, m: &Metrics) -> f32 {
    let error = content
        .error
        .as_ref()
        .map_or(0.0, |_| m.line_h + m.block_gap);
    let graph = if content.graph {
        m.graph_h + m.block_gap
    } else {
        0.0
    };
    let blocks: f32 = content
        .blocks
        .iter()
        .map(|b| block_height(b, m))
        .filter(|&h| h > 0.0)
        .map(|h| h + m.block_gap)
        .sum();
    (2.0 * m.pad + m.title_h + m.block_gap + graph + error + blocks - m.block_gap).ceil()
}

fn edge_gap(scale: f32) -> i32 {
    (8.0 * scale).round() as i32
}

#[derive(Debug)]
pub struct Fitted<'a> {
    pub content: Cow<'a, PopupContent>,
    pub metrics: Metrics,
    pub height: i32,
}

/// Narrows the popup to the monitor, then makes it short enough: the graph
/// shrinks to its minimum, then goes, then rows are cut with a last row
/// saying `more`.
pub fn fit<'a>(
    content: &'a PopupContent,
    mut m: Metrics,
    monitor: Rect32,
    more: &str,
) -> Fitted<'a> {
    let gap = edge_gap(m.scale);
    let room_w = (monitor.right - monitor.left - 2 * gap).max(1);
    let room_h = (monitor.bottom - monitor.top - 2 * gap).max(1);
    m.width = m.width.min(room_w as f32);
    let room = room_h as f32;
    let mut fitted = Cow::Borrowed(content);
    let over = height(content, &m) - room;
    if over > 0.0 && content.graph {
        let least = f32::from(*POPUP_GRAPH_HEIGHTS.start()) * m.scale;
        if m.graph_h - over >= least {
            m.graph_h -= over;
        } else {
            fitted.to_mut().graph = false;
        }
    }
    if height(&fitted, &m) > room {
        cap_rows(fitted.to_mut(), &m, room, more);
    }
    let height = (height(&fitted, &m) as i32).min(room_h);
    Fitted {
        content: fitted,
        metrics: m,
        height,
    }
}

/// Keeps the blocks that fit `room` in order, cutting the first one that
/// does not if it is plain rows, and ends with a row saying `more`.
fn cap_rows(content: &mut PopupContent, m: &Metrics, room: f32, more: &str) {
    let last = Block::Rows {
        heading: None,
        rows: vec![(more.to_string(), String::new())],
    };
    let blocks = std::mem::replace(&mut content.blocks, vec![last.clone()]);
    let mut left = room - height(content, m);
    let mut kept = Vec::new();
    for block in blocks {
        let cost = block_height(&block, m);
        if cost == 0.0 {
            continue;
        }
        if cost + m.block_gap <= left {
            left -= cost + m.block_gap;
            kept.push(block);
            continue;
        }
        if let Block::Rows { heading, mut rows } = block {
            let head = heading.map_or(0.0, |_| m.line_h);
            let fits = ((left - m.block_gap - head) / m.line_h).floor();
            if fits >= 1.0 {
                rows.truncate(fits as usize);
                kept.push(Block::Rows { heading, rows });
            }
        }
        break;
    }
    kept.push(last);
    content.blocks = kept;
}

/// Centres the popup on the hovered item, just outside the taskbar: above or
/// below a horizontal one, beside a vertical one; kept inside the monitor.
pub fn position(
    anchor: Rect32,
    taskbar: Rect32,
    monitor: Rect32,
    width: i32,
    height: i32,
    scale: f32,
) -> (i32, i32) {
    let gap = edge_gap(scale);
    if taskbar.is_vertical() {
        let centre = (anchor.top + anchor.bottom) / 2;
        let y = (centre - height / 2).clamp(
            monitor.top + gap,
            (monitor.bottom - height - gap).max(monitor.top + gap),
        );
        let right = taskbar.right + gap;
        let left = taskbar.left - gap - width;
        let x = if right + width <= monitor.right {
            right
        } else if left >= monitor.left {
            left
        } else {
            monitor.left
        };
        return (x, y);
    }
    let centre = (anchor.left + anchor.right) / 2;
    let x = (centre - width / 2).clamp(
        monitor.left + gap,
        (monitor.right - width - gap).max(monitor.left + gap),
    );
    let above = taskbar.top - height - gap;
    let below = taskbar.bottom + gap;
    let y = if above >= monitor.top {
        above
    } else if below + height <= monitor.bottom {
        below
    } else {
        monitor.top
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
            graph: true,
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
        fn icon_width(&self, _glyph: &str, font_px: f32) -> f32 {
            font_px
        }
    }

    #[test]
    fn item_rect_spans_all_of_an_items_cells() {
        use pulsar_core::config::Config;
        use pulsar_core::config::DisplayMode;
        use pulsar_core::layout::{ItemNames, LayoutInput, TextSpacing, compute_layout};
        let names = ItemNames::resolve(&Config::default());
        let input = LayoutInput {
            mode: DisplayMode::Text,
            taskbar_height_px: 48.0,
            fit_font: false,
            dpi: 96,
            font_size_pt: 9.0,
            labels: pulsar_core::config::LabelStyle::Full,
            icons: false,
            graph_inline: false,
            upload_on_top: false,
            vertical_bar_px: None,
            text_spacing: TextSpacing::default(),
            names: &names,
        };
        let specs: Vec<_> = [ItemKind::Cpu, ItemKind::Network]
            .into_iter()
            .map(|kind| pulsar_core::layout::ItemSpec {
                kind,
                parts: vec![CellPart::Main],
                tile: Default::default(),
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
    fn the_popup_anchor_covers_every_network_cell_in_each_show_mode() {
        use pulsar_core::config::{Config, DisplayMode, LabelStyle, NetworkShow};
        use pulsar_core::layout::{
            ItemNames, LayoutInput, TextSpacing, compute_layout, item_specs,
        };
        let names = ItemNames::resolve(&Config::default());
        for show in [
            NetworkShow::Both,
            NetworkShow::Download,
            NetworkShow::Upload,
        ] {
            for mode in [DisplayMode::Graph, DisplayMode::Text] {
                for adapters in [0, 2] {
                    for bar in [None, Some(80.0)] {
                        let mut c = Config::default();
                        c.network.show = show;
                        c.display.mode = mode;
                        c.network.adapters = (0..adapters).map(|i| format!("a{i}")).collect();
                        let input = LayoutInput {
                            mode,
                            taskbar_height_px: 48.0,
                            fit_font: false,
                            dpi: 96,
                            font_size_pt: 9.0,
                            labels: LabelStyle::Full,
                            icons: false,
                            graph_inline: false,
                            upload_on_top: false,
                            vertical_bar_px: bar,
                            text_spacing: TextSpacing::default(),
                            names: &names,
                        };
                        let l = compute_layout(&item_specs(&c, &Default::default()), input, &Fixed);
                        let net = item_screen_rect(&l, ItemKind::Network, (0, 0)).unwrap();
                        let cells = l.cells.iter().filter(|c| c.kind == ItemKind::Network);
                        for c in cells {
                            assert_eq!(l.hit_test(c.rect.x + 1.0, c.rect.y + 1.0), Some(c.kind));
                            assert!(net.left <= c.rect.x as i32 && net.top <= c.rect.y as i32);
                            assert!(net.right >= c.rect.right() as i32);
                            assert!(net.bottom >= (c.rect.y + c.rect.h) as i32);
                        }
                    }
                }
            }
        }
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
    fn hiding_the_graph_saves_its_height() {
        let m = Metrics::new(96, 12.0, 16.0);
        let mut c = content(vec![rows(1)]);
        let with = height(&c, &m);
        c.graph = false;
        assert_eq!(with - height(&c, &m), m.graph_h + m.block_gap);
    }

    #[test]
    fn a_composition_takes_its_bar_and_a_line_per_part() {
        let m = Metrics::new(96, 12.0, 16.0);
        let without = height(&content(vec![rows(1)]), &m);
        let parts = vec![(String::new(), 1.0, String::new()); 4];
        let with = height(
            &content(vec![
                rows(1),
                Block::Composition {
                    heading: "h",
                    parts,
                },
            ]),
            &m,
        );
        assert_eq!(with - without, m.block_gap + 16.0 + 10.0 + 5.0 + 4.0 * 16.0);
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
    fn default_size_matches_the_plain_metrics() {
        let d = pulsar_core::config::PopupConfig::default();
        let sized = Metrics::with_size(144, 18.0, 24.0, d.width, d.graph_height);
        assert_eq!(sized, Metrics::new(144, 18.0, 24.0));
        assert_eq!((sized.width, sized.graph_h), (450.0, 108.0));
    }

    #[test]
    fn with_size_scales_and_sets_the_graph_height() {
        let small = Metrics::with_size(96, 12.0, 16.0, 300, 72);
        let tall = Metrics::with_size(96, 12.0, 16.0, 300, 172);
        let c = content(vec![rows(2)]);
        assert_eq!(height(&c, &tall) - height(&c, &small), 100.0);
        let wide = Metrics::with_size(192, 24.0, 32.0, 500, 100);
        assert_eq!((wide.width, wide.graph_h), (1000.0, 200.0));
    }

    fn monitor(w: i32, h: i32) -> Rect32 {
        Rect32 {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        }
    }

    const MORE: &str = "More in the details window";

    fn ends_with_more(c: &PopupContent) -> bool {
        matches!(
            c.blocks.last(),
            Some(Block::Rows { heading: None, rows })
                if rows.len() == 1 && rows[0].0 == MORE && rows[0].1.is_empty()
        )
    }

    #[test]
    fn content_that_fits_is_not_touched() {
        let c = content(vec![rows(5)]);
        let m = Metrics::new(96, 12.0, 16.0);
        let f = fit(&c, m, MONITOR, MORE);
        assert!(matches!(f.content, Cow::Borrowed(_)));
        assert_eq!(f.metrics, m);
        assert_eq!(f.height, height(&c, &m) as i32);
    }

    #[test]
    fn width_clamps_to_the_monitor() {
        let m = Metrics::with_size(192, 24.0, 32.0, 800, 72);
        let c = content(vec![rows(1)]);
        let f = fit(&c, m, monitor(1280, 720), MORE);
        assert_eq!(f.metrics.width, 1280.0 - 32.0);
        let (x, _) = position(
            Rect32 {
                left: 1200,
                top: 680,
                right: 1240,
                bottom: 716,
            },
            Rect32 {
                left: 0,
                top: 672,
                right: 1280,
                bottom: 720,
            },
            monitor(1280, 720),
            f.metrics.width as i32,
            f.height,
            2.0,
        );
        assert_eq!(x, 16);
    }

    #[test]
    fn the_graph_shrinks_before_any_row_goes() {
        let c = content(vec![rows(10)]);
        let m = Metrics::with_size(96, 12.0, 16.0, 300, 400);
        let full = height(&c, &m) as i32;
        let room = full - 100;
        let f = fit(&c, m, monitor(1920, room + 16), MORE);
        assert_eq!(f.height, room);
        assert_eq!(f.metrics.graph_h, 300.0);
        assert!(f.content.graph);
        assert_eq!(f.content.blocks, c.blocks, "every row stays");
    }

    #[test]
    fn the_graph_goes_when_its_minimum_does_not_fit() {
        let c = content(vec![rows(10)]);
        let m = Metrics::with_size(96, 12.0, 16.0, 300, 72);
        let mut text = c.clone();
        text.graph = false;
        let text_h = height(&text, &m) as i32;

        let f = fit(&c, m, monitor(1920, text_h + 34 + 16), MORE);
        assert!(f.content.graph, "24 px and its gap still fit");
        assert_eq!(f.metrics.graph_h, 24.0);

        let f = fit(&c, m, monitor(1920, text_h + 33 + 16), MORE);
        assert!(!f.content.graph);
        assert_eq!(f.content.blocks, c.blocks, "text alone fits");
        assert_eq!(f.height, text_h);
    }

    #[test]
    fn text_taller_than_the_monitor_caps_rows_and_points_to_details() {
        let c = content(vec![rows(4), rows(100), rows(4)]);
        let m = Metrics::new(96, 12.0, 16.0);
        let f = fit(&c, m, monitor(1920, 400), MORE);
        assert!(f.height <= 400 - 16);
        assert!(!f.content.graph);
        assert!(ends_with_more(&f.content));
        assert_eq!(f.content.blocks.len(), 3, "the third block is cut off");
        assert_eq!(f.content.blocks[0], c.blocks[0]);
        let Block::Rows { rows: cut, .. } = &f.content.blocks[1] else {
            panic!("the second block keeps its rows");
        };
        assert!(!cut.is_empty() && cut.len() < 100);
        assert!(
            f.height + 16 > 400 - 16,
            "no room left for another row: {}",
            f.height
        );
    }

    #[test]
    fn many_devices_at_high_dpi_stay_on_the_monitor() {
        let devices: Vec<Block> = (0..12)
            .map(|_| Block::Rows {
                heading: Some("Device"),
                rows: vec![(String::new(), String::new()); 8],
            })
            .chain([Block::Composition {
                heading: "Shares",
                parts: vec![(String::new(), 1.0, String::new()); 6],
            }])
            .collect();
        for dpi in [96, 144, 240, 288] {
            let scale = dpi as f32 / 96.0;
            let (font, line) = (12.0 * scale, 16.0 * scale);
            let c = content(devices.clone());
            let m = Metrics::with_size(dpi, font, line, 800, 400);
            let f = fit(&c, m, monitor(1920, 1080), MORE);
            let gap = edge_gap(scale);
            assert!(f.height <= 1080 - 2 * gap, "{dpi}: {}", f.height);
            assert!(f.metrics.width <= (1920 - 2 * gap) as f32);
            assert_eq!(f.height as f32, height(&f.content, &f.metrics), "{dpi}");
            assert!(ends_with_more(&f.content), "{dpi}");
            assert!(!f.content.graph, "{dpi}");
        }
    }

    #[test]
    fn blocks_that_cannot_be_cut_go_whole() {
        let bars = Block::Bars {
            heading: "Cores",
            values: vec![1.0; 8],
        };
        let c = content(vec![
            rows(2),
            Block::Composition {
                heading: "Shares",
                parts: vec![(String::new(), 1.0, String::new()); 40],
            },
            bars,
        ]);
        let f = fit(&c, Metrics::new(96, 12.0, 16.0), monitor(1920, 300), MORE);
        assert_eq!(f.content.blocks.len(), 2);
        assert_eq!(f.content.blocks[0], c.blocks[0]);
        assert!(ends_with_more(&f.content));
    }

    #[test]
    fn a_monitor_shorter_than_the_title_still_bounds_the_popup() {
        let mut c = content(vec![rows(3)]);
        c.error = Some("failed".into());
        let f = fit(&c, Metrics::new(96, 12.0, 16.0), monitor(100, 40), MORE);
        assert_eq!(f.height, 24);
        assert_eq!(f.metrics.width, 84.0);
        assert!(ends_with_more(&f.content));
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
    fn opens_beside_a_vertical_taskbar() {
        let anchor = Rect32 {
            left: 0,
            top: 500,
            right: 48,
            bottom: 540,
        };
        let left_bar = Rect32 {
            left: 0,
            top: 0,
            right: 48,
            bottom: 1080,
        };
        assert_eq!(
            position(anchor, left_bar, MONITOR, 300, 200, 1.0),
            (48 + 8, 520 - 100)
        );
        let right_bar = Rect32 {
            left: 1872,
            right: 1920,
            ..left_bar
        };
        assert_eq!(
            position(anchor, right_bar, MONITOR, 300, 200, 1.0).0,
            1872 - 8 - 300
        );
        let near_end = Rect32 {
            top: 1040,
            bottom: 1076,
            ..anchor
        };
        assert_eq!(
            position(near_end, left_bar, MONITOR, 300, 200, 1.0).1,
            1080 - 200 - 8
        );
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
