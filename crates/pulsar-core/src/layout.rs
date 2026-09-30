//! Pure layout: turns the enabled items, display mode and taskbar metrics into
//! rectangles. All values are physical pixels.

use crate::config::{Config, DisplayMode, DriveValue, LabelStyle};
use crate::format::widest_value;
use crate::metric::{ItemKind, MetricKey};

pub trait TextMeasure {
    fn width(&self, text: &str, font_px: f32) -> f32;
    fn line_height(&self, font_px: f32) -> f32;
    /// Width of an icon glyph drawn at `font_px`.
    fn icon_width(&self, glyph: &str, font_px: f32) -> f32;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellPart {
    Main,
    Down,
    Up,
    /// Per-drive activity or used space, by drive letter.
    DriveActive(u8),
    DriveUsed(u8),
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
    pub labels: LabelStyle,
    pub icons: bool,
}

/// An enabled item and the cells it draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSpec {
    pub kind: ItemKind,
    pub parts: Vec<CellPart>,
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
        ItemKind::GpuTemp => MetricKey::GpuTempC,
    }
}

pub fn part_metric(kind: ItemKind, part: CellPart) -> MetricKey {
    match part {
        CellPart::Up => MetricKey::NetUpBps,
        CellPart::DriveActive(d) => MetricKey::DriveActivePercent(d),
        CellPart::DriveUsed(d) => MetricKey::DriveUsedPercent(d),
        CellPart::Main | CellPart::Down => primary_metric(kind),
    }
}

/// The text part of a cell's label. Drive letters always show; network
/// arrows give way to the direction icons.
pub fn cell_label(kind: ItemKind, part: CellPart, labels: LabelStyle, icons: bool) -> String {
    match part {
        CellPart::Down | CellPart::Up if icons => String::new(),
        CellPart::Down => "↓".into(),
        CellPart::Up => "↑".into(),
        CellPart::DriveActive(d) | CellPart::DriveUsed(d) => format!("{}:", d as char),
        CellPart::Main => match labels {
            LabelStyle::Full => kind.label().into(),
            LabelStyle::Short => kind.short_label().into(),
            LabelStyle::None => String::new(),
        },
    }
}

/// A Segoe Fluent Icons glyph for the cell.
pub fn cell_icon(kind: ItemKind, part: CellPart) -> &'static str {
    match (kind, part) {
        (_, CellPart::Down) => "\u{E896}",
        (_, CellPart::Up) => "\u{E898}",
        (_, CellPart::DriveActive(_) | CellPart::DriveUsed(_)) => "\u{EDA2}",
        (ItemKind::Cpu, _) => "\u{E950}",
        (ItemKind::Ram, _) => "\u{EEA0}",
        (ItemKind::Gpu, _) => "\u{F211}",
        (ItemKind::GpuTemp, _) => "\u{E9CA}",
        (ItemKind::Disk, _) => "\u{EDA2}",
        (ItemKind::Network, _) => "\u{E968}",
        (ItemKind::Ping, _) => "\u{EC05}",
    }
}

/// Space between an icon and the text after it.
pub fn icon_gap(font_px: f32) -> f32 {
    (font_px * 0.3).round()
}

/// Icon and label text together, as drawn.
pub fn label_width(
    kind: ItemKind,
    part: CellPart,
    labels: LabelStyle,
    icons: bool,
    font_px: f32,
    m: &dyn TextMeasure,
) -> f32 {
    let text = cell_label(kind, part, labels, icons);
    let text_w = if text.is_empty() {
        0.0
    } else {
        m.width(&text, font_px)
    };
    if !icons {
        return text_w;
    }
    let icon_w = m.icon_width(cell_icon(kind, part), font_px);
    if text_w > 0.0 {
        icon_w + icon_gap(font_px) + text_w
    } else {
        icon_w
    }
}

#[derive(Clone, Copy)]
struct Labels {
    style: LabelStyle,
    icons: bool,
}

pub fn default_parts(kind: ItemKind) -> Vec<CellPart> {
    match kind {
        ItemKind::Network => vec![CellPart::Down, CellPart::Up],
        _ => vec![CellPart::Main],
    }
}

/// The enabled items in order, each with the cells it draws.
pub fn item_specs(config: &Config) -> Vec<ItemSpec> {
    let drives = config.drive_letters();
    config
        .enabled_items()
        .into_iter()
        .map(|kind| {
            let parts = if kind == ItemKind::Disk && !drives.is_empty() {
                drives
                    .iter()
                    .map(|&d| match config.disk.value {
                        DriveValue::Activity => CellPart::DriveActive(d),
                        DriveValue::Used => CellPart::DriveUsed(d),
                    })
                    .collect()
            } else {
                default_parts(kind)
            };
            ItemSpec { kind, parts }
        })
        .collect()
}

pub fn compute_layout(items: &[ItemSpec], input: LayoutInput, m: &dyn TextMeasure) -> Layout {
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
    let labels = Labels {
        style: input.labels,
        icons: input.icons,
    };
    match input.mode {
        DisplayMode::Text => text_layout(items, height, scale, font_px, labels, m),
        DisplayMode::Graph => graph_layout(items, height, scale, font_px, labels, m),
    }
}

/// Packs items into columns of `rows` cells, column-major. An item's parts
/// (network ↓ and ↑) are kept together when they fit in a column.
fn columns(items: &[ItemSpec], rows: usize) -> Vec<Vec<(ItemKind, CellPart)>> {
    let mut columns: Vec<Vec<(ItemKind, CellPart)>> = Vec::new();
    for spec in items {
        let free = columns.last().map_or(0, |c| rows - c.len());
        if free == 0 || (spec.parts.len() > free && spec.parts.len() <= rows) {
            columns.push(Vec::with_capacity(rows));
        }
        for &part in &spec.parts {
            if columns.last().is_some_and(|c| c.len() == rows) {
                columns.push(Vec::with_capacity(rows));
            }
            columns.last_mut().unwrap().push((spec.kind, part));
        }
    }
    columns
}

fn text_layout(
    items: &[ItemSpec],
    height: f32,
    scale: f32,
    font_px: f32,
    labels: Labels,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = PADDING_DIP * scale;
    let line_h = m.line_height(font_px);
    let rows = (((height - 2.0 * padding) / line_h).floor() as usize).clamp(1, MAX_TEXT_ROWS);
    let top = ((height - rows as f32 * line_h) / 2.0).max(0.0);

    let mut cells = Vec::new();
    let mut x = padding;
    for column in columns(items, rows) {
        let label_w = column
            .iter()
            .map(|&(k, p)| label_width(k, p, labels.style, labels.icons, font_px, m))
            .fold(0.0, f32::max);
        let value_w = column
            .iter()
            .map(|&(k, p)| m.width(widest_value(part_metric(k, p).unit()), font_px))
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

/// Widest text a graph tile draws: network shows `↓ value` / `↑ value`,
/// other tiles a label line and a value line.
fn graph_text_width(
    kind: ItemKind,
    part: CellPart,
    labels: Labels,
    font_px: f32,
    m: &dyn TextMeasure,
) -> f32 {
    let widest = widest_value(part_metric(kind, part).unit());
    let label = |part| label_width(kind, part, labels.style, labels.icons, font_px, m);
    match kind {
        ItemKind::Network => label(CellPart::Down) + icon_gap(font_px) + m.width(widest, font_px),
        _ => label(part).max(m.width(widest, font_px)),
    }
}

fn graph_layout(
    items: &[ItemSpec],
    height: f32,
    scale: f32,
    font_px: f32,
    labels: Labels,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = PADDING_DIP * scale;
    let tile_h = (height - 2.0 * padding).max(1.0);
    let mut cells = Vec::with_capacity(items.len());
    let mut x = padding;
    for spec in items {
        // Network draws both directions in one tile; other items get a tile per part.
        let tiles = if spec.kind == ItemKind::Network {
            vec![CellPart::Main]
        } else {
            spec.parts.clone()
        };
        for part in tiles {
            let w = (graph_text_width(spec.kind, part, labels, font_px, m) + 2.0 * padding)
                .max(MIN_TILE_DIP * scale);
            cells.push(Cell {
                kind: spec.kind,
                part,
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
    use crate::config::{Config, DriveValue, LabelStyle};

    /// Every character is half the font size wide; lines are 1.25× the font size.
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

    fn input(mode: DisplayMode, taskbar_height_px: f32, dpi: u32) -> LayoutInput {
        LayoutInput {
            mode,
            taskbar_height_px,
            dpi,
            font_size_pt: 9.0,
            labels: LabelStyle::Full,
            icons: false,
        }
    }

    fn specs(kinds: &[ItemKind]) -> Vec<ItemSpec> {
        kinds
            .iter()
            .map(|&kind| ItemSpec {
                kind,
                parts: default_parts(kind),
            })
            .collect()
    }

    const ALL: [ItemKind; 7] = ItemKind::ALL;

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
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Text, 48.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 2);
        assert_no_overlap(&l);
        assert_inside(&l);
    }

    #[test]
    fn text_mode_small_taskbar_uses_one_row() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Text, 32.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn text_mode_rows_are_capped_at_three() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Text, 200.0, 96), &Fixed);
        let rows: std::collections::BTreeSet<i32> =
            l.cells.iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn text_mode_network_takes_two_cells() {
        let l = compute_layout(
            &specs(&[ItemKind::Network]),
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        let parts: Vec<CellPart> = l.cells.iter().map(|c| c.part).collect();
        assert_eq!(parts, vec![CellPart::Down, CellPart::Up]);
    }

    #[test]
    fn text_mode_keeps_an_items_parts_in_one_column() {
        let items = [ItemKind::Cpu, ItemKind::Network, ItemKind::Gpu];
        let l = compute_layout(&specs(&items), input(DisplayMode::Text, 48.0, 96), &Fixed);
        let x_of = |part| l.cells.iter().find(|c| c.part == part).unwrap().rect.x;
        assert_eq!(x_of(CellPart::Down), x_of(CellPart::Up));
        let cpu = l.cells.iter().find(|c| c.kind == ItemKind::Cpu).unwrap();
        assert_ne!(
            cpu.rect.x,
            x_of(CellPart::Down),
            "network starts a new column"
        );
        assert_no_overlap(&l);
    }

    #[test]
    fn text_mode_single_row_still_places_every_part() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Text, 32.0, 96), &Fixed);
        assert_eq!(l.cells.len(), ALL.len() + 1);
        assert_no_overlap(&l);
    }

    #[test]
    fn text_mode_column_width_reserves_widest_value() {
        // One column: labels "CPU","RAM" = 3 chars → 18px; widest "100%" = 4 chars → 24px; gap 4px.
        let l = compute_layout(
            &specs(&[ItemKind::Cpu, ItemKind::Ram]),
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
            &specs(&[ItemKind::Cpu, ItemKind::Ping]),
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        assert_eq!(l.cells[0].value_x, l.cells[1].value_x);
    }

    #[test]
    fn layout_scales_with_dpi() {
        let a = compute_layout(&specs(&ALL), input(DisplayMode::Text, 48.0, 96), &Fixed);
        let b = compute_layout(&specs(&ALL), input(DisplayMode::Text, 72.0, 144), &Fixed);
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
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Graph, 48.0, 96), &Fixed);
        assert_eq!(l.cells.len(), ALL.len());
        for c in &l.cells {
            assert_eq!(c.rect.h, 40.0);
            assert!(c.rect.w >= 44.0);
        }
        assert_no_overlap(&l);
        assert_inside(&l);
    }

    #[test]
    fn graph_tiles_fit_their_widest_text() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Graph, 48.0, 96), &Fixed);
        let net = l
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Network)
            .unwrap();
        // "↓" (6px) + icon gap (4px) + "99.9 MB/s" (54px) + 2 × 4px padding.
        assert_eq!(net.rect.w, 6.0 + 4.0 + 54.0 + 8.0);
        let cpu = l.cells.iter().find(|c| c.kind == ItemKind::Cpu).unwrap();
        assert_eq!(cpu.rect.w, 44.0, "short text keeps the minimum tile width");
    }

    #[test]
    fn no_items_gives_zero_width() {
        let l = compute_layout(&specs(&[]), input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_eq!(l.width, 0.0);
        assert!(l.cells.is_empty());
    }

    #[test]
    fn tiny_taskbar_still_lays_out_one_row() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Text, 10.0, 96), &Fixed);
        assert!(!l.cells.is_empty());
        assert!(l.cells.iter().all(|c| c.rect.y >= 0.0));
    }

    #[test]
    fn hit_test_finds_item() {
        let l = compute_layout(&specs(&ALL), input(DisplayMode::Graph, 48.0, 96), &Fixed);
        let gpu = l.cells.iter().find(|c| c.kind == ItemKind::Gpu).unwrap();
        assert_eq!(
            l.hit_test(gpu.rect.x + 1.0, gpu.rect.y + 1.0),
            Some(ItemKind::Gpu)
        );
        assert_eq!(l.hit_test(-5.0, 0.0), None);
    }
    #[test]
    fn per_drive_disk_has_a_cell_per_drive_in_text_mode() {
        let mut c = Config::default();
        c.disk.drives = vec!["C".into(), "D".into()];
        let disk = item_specs(&c)
            .into_iter()
            .find(|s| s.kind == ItemKind::Disk)
            .unwrap();
        assert_eq!(
            disk.parts,
            [CellPart::DriveActive(b'C'), CellPart::DriveActive(b'D')]
        );
        c.disk.value = DriveValue::Used;
        let disk = item_specs(&c)
            .into_iter()
            .find(|s| s.kind == ItemKind::Disk)
            .unwrap();
        assert_eq!(
            disk.parts,
            [CellPart::DriveUsed(b'C'), CellPart::DriveUsed(b'D')]
        );
        let l = compute_layout(&[disk], input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_eq!(l.cells.len(), 2);
        assert_eq!(
            cell_label(ItemKind::Disk, l.cells[1].part, LabelStyle::Full, false),
            "D:"
        );
    }

    #[test]
    fn per_drive_disk_has_a_tile_per_drive_in_graph_mode() {
        let items = [ItemSpec {
            kind: ItemKind::Disk,
            parts: vec![CellPart::DriveActive(b'C'), CellPart::DriveActive(b'D')],
        }];
        let l = compute_layout(&items, input(DisplayMode::Graph, 48.0, 96), &Fixed);
        assert_eq!(l.cells.len(), 2);
        assert_ne!(l.cells[0].rect.x, l.cells[1].rect.x);
        assert_eq!(
            l.hit_test(l.cells[1].rect.x + 1.0, l.cells[1].rect.y + 1.0),
            Some(ItemKind::Disk)
        );
        assert_no_overlap(&l);
    }

    #[test]
    fn network_stays_one_graph_tile() {
        let l = compute_layout(
            &specs(&[ItemKind::Network]),
            input(DisplayMode::Graph, 48.0, 96),
            &Fixed,
        );
        assert_eq!(l.cells.len(), 1);
    }

    #[test]
    fn short_labels_make_text_mode_narrower() {
        let long = compute_layout(&specs(&ALL), input(DisplayMode::Text, 48.0, 96), &Fixed);
        let mut i = input(DisplayMode::Text, 48.0, 96);
        i.labels = LabelStyle::Short;
        let short = compute_layout(&specs(&ALL), i, &Fixed);
        assert!(
            short.width < long.width * 0.9,
            "{} vs {}",
            short.width,
            long.width
        );
        assert_eq!(
            cell_label(ItemKind::Cpu, CellPart::Main, LabelStyle::Short, false),
            "C"
        );
        assert_eq!(
            cell_label(ItemKind::Network, CellPart::Up, LabelStyle::Short, false),
            "↑"
        );
        assert_eq!(
            cell_label(
                ItemKind::Disk,
                CellPart::DriveActive(b'C'),
                LabelStyle::Short,
                false
            ),
            "C:"
        );
    }

    #[test]
    fn part_metrics_cover_drives_and_network() {
        assert_eq!(
            part_metric(ItemKind::Disk, CellPart::DriveUsed(b'E')),
            MetricKey::DriveUsedPercent(b'E')
        );
        assert_eq!(
            part_metric(ItemKind::Disk, CellPart::DriveActive(b'E')),
            MetricKey::DriveActivePercent(b'E')
        );
        assert_eq!(
            part_metric(ItemKind::Network, CellPart::Up),
            MetricKey::NetUpBps
        );
        assert_eq!(
            part_metric(ItemKind::GpuTemp, CellPart::Main),
            MetricKey::GpuTempC
        );
    }

    #[test]
    fn cell_label_styles() {
        let label = |style| cell_label(ItemKind::Ram, CellPart::Main, style, true);
        assert_eq!(label(LabelStyle::Full), "RAM");
        assert_eq!(label(LabelStyle::Short), "M");
        assert_eq!(label(LabelStyle::None), "");
        assert_eq!(cell_icon(ItemKind::Cpu, CellPart::Main), "\u{E950}");
        assert_eq!(cell_icon(ItemKind::Network, CellPart::Up), "\u{E898}");
        assert_eq!(
            cell_icon(ItemKind::Disk, CellPart::DriveUsed(b'D')),
            "\u{EDA2}"
        );
    }

    #[test]
    fn icons_replace_network_arrows_but_keep_drive_letters() {
        assert_eq!(
            cell_label(ItemKind::Network, CellPart::Down, LabelStyle::Full, true),
            ""
        );
        assert_eq!(
            cell_label(
                ItemKind::Disk,
                CellPart::DriveActive(b'C'),
                LabelStyle::None,
                true
            ),
            "C:"
        );
    }

    #[test]
    fn icons_add_width_and_labels_none_is_narrowest() {
        let width = |labels, icons| {
            let mut i = input(DisplayMode::Text, 48.0, 96);
            i.labels = labels;
            i.icons = icons;
            compute_layout(&specs(&ALL), i, &Fixed).width
        };
        let full = width(LabelStyle::Full, false);
        let full_icons = width(LabelStyle::Full, true);
        let icons_only = width(LabelStyle::None, true);
        assert!(full_icons > full, "{full_icons} vs {full}");
        assert!(icons_only < full, "{icons_only} vs {full}");
        for mode in [DisplayMode::Text, DisplayMode::Graph] {
            let mut i = input(mode, 48.0, 96);
            i.labels = LabelStyle::Short;
            i.icons = true;
            let l = compute_layout(&specs(&ALL), i, &Fixed);
            assert_no_overlap(&l);
            assert_inside(&l);
        }
    }
}
