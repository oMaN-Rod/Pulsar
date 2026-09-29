//! Pure layout: turns the enabled items, display mode and taskbar metrics into
//! rectangles. All values are physical pixels.

use crate::config::DisplayMode;
use crate::format::widest_value;
use crate::metric::{ItemKind, MetricKey};

pub trait TextMeasure {
    fn width(&self, text: &str, font_px: f32) -> f32;
    fn line_height(&self, font_px: f32) -> f32;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellPart {
    Main,
    Down,
    Up,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub kind: ItemKind,
    pub part: CellPart,
    pub rect: Rect,
    /// Text mode: x where the value starts (labels are left-aligned before it).
    pub value_x: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub font_px: f32,
    pub cells: Vec<Cell>,
}

impl Layout {
    /// The item under a point, for hover and click handling.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<ItemKind> {
        self.cells
            .iter()
            .find(|c| c.rect.contains(x, y))
            .map(|c| c.kind)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutInput {
    pub mode: DisplayMode,
    pub taskbar_height_px: f32,
    pub dpi: u32,
    pub font_size_pt: f32,
}

const PADDING_DIP: f32 = 4.0;
const COLUMN_GAP_DIP: f32 = 10.0;
const LABEL_GAP_DIP: f32 = 4.0;
const TILE_GAP_DIP: f32 = 4.0;
const MIN_TILE_DIP: f32 = 44.0;
const MAX_TEXT_ROWS: usize = 3;

pub fn primary_metric(kind: ItemKind) -> MetricKey {
    match kind {
        ItemKind::Cpu => MetricKey::CpuTotal,
        ItemKind::Ram => MetricKey::MemUsedPercent,
        ItemKind::Disk => MetricKey::DiskActivePercent,
        ItemKind::Network => MetricKey::NetDownBps,
        ItemKind::Gpu => MetricKey::GpuUtil,
        ItemKind::Ping => MetricKey::PingMs,
    }
}

pub fn cell_label(kind: ItemKind, part: CellPart) -> &'static str {
    match part {
        CellPart::Main => kind.label(),
        CellPart::Down => "↓",
        CellPart::Up => "↑",
    }
}

fn parts(kind: ItemKind) -> &'static [CellPart] {
    match kind {
        ItemKind::Network => &[CellPart::Down, CellPart::Up],
        _ => &[CellPart::Main],
    }
}

pub fn compute_layout(items: &[ItemKind], input: LayoutInput, m: &dyn TextMeasure) -> Layout {
    let scale = input.dpi as f32 / 96.0;
    let font_px = input.font_size_pt * input.dpi as f32 / 72.0;
    let height = input.taskbar_height_px;
    if items.is_empty() {
        return Layout {
            width: 0.0,
            height,
            font_px,
            cells: Vec::new(),
        };
    }
    match input.mode {
        DisplayMode::Text => text_layout(items, height, scale, font_px, m),
        DisplayMode::Graph => graph_layout(items, height, scale, font_px, m),
    }
}

fn text_layout(
    items: &[ItemKind],
    height: f32,
    scale: f32,
    font_px: f32,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = PADDING_DIP * scale;
    let line_h = m.line_height(font_px);
    let rows = (((height - 2.0 * padding) / line_h).floor() as usize).clamp(1, MAX_TEXT_ROWS);
    let top = ((height - rows as f32 * line_h) / 2.0).max(0.0);

    let slots: Vec<(ItemKind, CellPart)> = items
        .iter()
        .flat_map(|&k| parts(k).iter().map(move |&p| (k, p)))
        .collect();

    let mut cells = Vec::with_capacity(slots.len());
    let mut x = padding;
    for column in slots.chunks(rows) {
        let label_w = column
            .iter()
            .map(|&(k, p)| m.width(cell_label(k, p), font_px))
            .fold(0.0, f32::max);
        let value_w = column
            .iter()
            .map(|&(k, _)| m.width(widest_value(primary_metric(k).unit()), font_px))
            .fold(0.0, f32::max);
        let col_w = label_w + LABEL_GAP_DIP * scale + value_w;
        for (row, &(kind, part)) in column.iter().enumerate() {
            cells.push(Cell {
                kind,
                part,
                rect: Rect {
                    x,
                    y: top + row as f32 * line_h,
                    w: col_w,
                    h: line_h,
                },
                value_x: x + label_w + LABEL_GAP_DIP * scale,
            });
        }
        x += col_w + COLUMN_GAP_DIP * scale;
    }
    let width = x - COLUMN_GAP_DIP * scale + padding;
    Layout {
        width,
        height,
        font_px,
        cells,
    }
}

fn graph_layout(
    items: &[ItemKind],
    height: f32,
    scale: f32,
    font_px: f32,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = PADDING_DIP * scale;
    let tile_h = (height - 2.0 * padding).max(1.0);
    let mut cells = Vec::with_capacity(items.len());
    let mut x = padding;
    for &kind in items {
        let value_w = m.width(widest_value(primary_metric(kind).unit()), font_px);
        let w = (value_w + 2.0 * padding).max(MIN_TILE_DIP * scale);
        cells.push(Cell {
            kind,
            part: CellPart::Main,
            rect: Rect {
                x,
                y: padding,
                w,
                h: tile_h,
            },
            value_x: x + padding,
        });
        x += w + TILE_GAP_DIP * scale;
    }
    let width = x - TILE_GAP_DIP * scale + padding;
    Layout {
        width,
        height,
        font_px,
        cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is half the font size wide; lines are 1.25× the font size.
    struct Fixed;
    impl TextMeasure for Fixed {
        fn width(&self, text: &str, font_px: f32) -> f32 {
            text.chars().count() as f32 * font_px * 0.5
        }
        fn line_height(&self, font_px: f32) -> f32 {
            font_px * 1.25
        }
    }

    fn input(mode: DisplayMode, taskbar_height_px: f32, dpi: u32) -> LayoutInput {
        LayoutInput {
            mode,
            taskbar_height_px,
            dpi,
            font_size_pt: 9.0,
        }
    }

    const ALL: [ItemKind; 6] = ItemKind::ALL;

    fn assert_no_overlap(layout: &Layout) {
        for (i, a) in layout.cells.iter().enumerate() {
            for b in &layout.cells[i + 1..] {
                let overlap_x = a.rect.x < b.rect.right() && b.rect.x < a.rect.right();
                let overlap_y = a.rect.y < b.rect.y + b.rect.h && b.rect.y < a.rect.y + a.rect.h;
                assert!(!(overlap_x && overlap_y), "{a:?} overlaps {b:?}");
            }
        }
    }

    fn assert_inside(layout: &Layout) {
        for c in &layout.cells {
            assert!(
                c.rect.x >= 0.0 && c.rect.right() <= layout.width + 0.01,
                "{c:?}"
            );
            assert!(
                c.rect.y >= 0.0 && c.rect.y + c.rect.h <= layout.height + 0.01,
                "{c:?}"
            );
        }
    }

    #[test]
    fn text_mode_standard_taskbar_uses_two_rows() {
        // 9pt @ 96 dpi = 12px font, 15px lines; (48 - 8) / 15 = 2 rows.
        let l = compute_layout(&ALL, input(DisplayMode::Text, 48.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 2);
        assert_no_overlap(&l);
        assert_inside(&l);
    }

    #[test]
    fn text_mode_small_taskbar_uses_one_row() {
        let l = compute_layout(&ALL, input(DisplayMode::Text, 32.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn text_mode_rows_are_capped_at_three() {
        let l = compute_layout(&ALL, input(DisplayMode::Text, 200.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn text_mode_network_takes_two_cells() {
        let l = compute_layout(
            &[ItemKind::Network],
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        let parts: Vec<CellPart> = l.cells.iter().map(|c| c.part).collect();
        assert_eq!(parts, vec![CellPart::Down, CellPart::Up]);
    }

    #[test]
    fn text_mode_column_width_reserves_widest_value() {
        // One column: labels "CPU","RAM" = 3 chars → 18px; widest "100%" = 4 chars → 24px; gap 4px.
        let l = compute_layout(
            &[ItemKind::Cpu, ItemKind::Ram],
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        assert_eq!(l.cells[0].rect.w, 18.0 + 4.0 + 24.0);
        assert_eq!(l.cells[0].value_x, l.cells[0].rect.x + 18.0 + 4.0);
        assert_eq!(l.cells[0].rect.x, l.cells[1].rect.x, "same column");
    }

    #[test]
    fn text_mode_labels_align_within_a_column() {
        // "PING" is wider than "CPU"; both values start at the same x.
        let l = compute_layout(
            &[ItemKind::Cpu, ItemKind::Ping],
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        assert_eq!(l.cells[0].value_x, l.cells[1].value_x);
    }

    #[test]
    fn layout_scales_with_dpi() {
        let a = compute_layout(&ALL, input(DisplayMode::Text, 48.0, 96), &Fixed);
        let b = compute_layout(&ALL, input(DisplayMode::Text, 72.0, 144), &Fixed);
        assert!(
            (b.width / a.width - 1.5).abs() < 0.01,
            "{} vs {}",
            a.width,
            b.width
        );
        assert_eq!(b.cells.len(), a.cells.len());
    }

    #[test]
    fn graph_mode_one_tile_per_item_full_height() {
        let l = compute_layout(&ALL, input(DisplayMode::Graph, 48.0, 96), &Fixed);
        assert_eq!(l.cells.len(), ALL.len());
        for c in &l.cells {
            assert_eq!(c.rect.h, 40.0);
            assert!(c.rect.w >= 44.0);
        }
        assert_no_overlap(&l);
        assert_inside(&l);
    }

    #[test]
    fn no_items_gives_zero_width() {
        let l = compute_layout(&[], input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_eq!(l.width, 0.0);
        assert!(l.cells.is_empty());
    }

    #[test]
    fn tiny_taskbar_still_lays_out_one_row() {
        let l = compute_layout(&ALL, input(DisplayMode::Text, 10.0, 96), &Fixed);
        assert!(!l.cells.is_empty());
        assert!(l.cells.iter().all(|c| c.rect.y >= 0.0));
    }

    #[test]
    fn hit_test_finds_item() {
        let l = compute_layout(&ALL, input(DisplayMode::Graph, 48.0, 96), &Fixed);
        let gpu = l.cells.iter().find(|c| c.kind == ItemKind::Gpu).unwrap();
        assert_eq!(
            l.hit_test(gpu.rect.x + 1.0, gpu.rect.y + 1.0),
            Some(ItemKind::Gpu)
        );
        assert_eq!(l.hit_test(-5.0, 0.0), None);
    }
}
