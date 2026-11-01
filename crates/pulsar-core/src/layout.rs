//! Pure layout: turns the enabled items, display mode and taskbar metrics into
//! rectangles. All values are physical pixels.

use crate::config::{Config, Display, DisplayMode, DriveValue, LabelStyle};
use crate::format::widest_value;
use crate::gpu_slots::GpuSlots;
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
    /// One GPU's load or temperature, by its slot in `GpuSlots`.
    Gpu(u8),
    /// One adapter's traffic, by its place in `NetworkConfig::adapters`.
    AdapterDown(u8),
    AdapterUp(u8),
}

impl CellPart {
    /// The part of a combined item this one shares its colour with.
    pub fn base(self) -> CellPart {
        match self {
            CellPart::DriveActive(_) | CellPart::DriveUsed(_) | CellPart::Gpu(_) => CellPart::Main,
            CellPart::AdapterDown(_) => CellPart::Down,
            CellPart::AdapterUp(_) => CellPart::Up,
            p => p,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub kind: ItemKind,
    pub part: CellPart,
    pub rect: Rect,
    /// Text mode: where the value starts (labels are left-aligned before it,
    /// or sit on the line above when a vertical taskbar is too narrow for both).
    pub value_x: f32,
    pub value_y: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub font_px: f32,
    pub cells: Vec<Cell>,
    pub vertical: bool,
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
    /// Not limited by a taskbar: grow taller than it so the font fits.
    pub fit_font: bool,
    pub dpi: u32,
    pub font_size_pt: f32,
    pub labels: LabelStyle,
    pub icons: bool,
    pub graph_inline: bool,
    pub upload_on_top: bool,
    /// The width of a vertical taskbar; items then stack from the top.
    pub vertical_bar_px: Option<f32>,
    pub text_spacing: TextSpacing,
}

/// Text mode gaps, in DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSpacing {
    pub item_gap: f32,
    pub label_gap: f32,
    pub row_gap: f32,
}

impl From<&Display> for TextSpacing {
    fn from(d: &Display) -> Self {
        Self {
            item_gap: d.text_item_gap as f32,
            label_gap: d.text_label_gap as f32,
            row_gap: d.text_row_gap as f32,
        }
    }
}

impl Default for TextSpacing {
    fn default() -> Self {
        Self::from(&Display::default())
    }
}

/// An enabled item and the cells it draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSpec {
    pub kind: ItemKind,
    pub parts: Vec<CellPart>,
}

const PADDING_DIP: f32 = 4.0;
const INLINE_GAP_DIP: f32 = 4.0;
const TILE_GAP_DIP: f32 = 4.0;
const MIN_TILE_DIP: f32 = 44.0;
const MAX_TEXT_ROWS: usize = 3;
const STACKED_PADDING_DIP: f32 = 2.0;
const STACKED_TILE_GAP_DIP: f32 = 2.0;
const MIN_STACKED_TILE_DIP: f32 = 36.0;
/// Graph tiles draw their text at this fraction of the font size.
pub const GRAPH_FONT_SCALE: f32 = 0.85;
/// A stacked tile draws two lines, each 1.3 × the text size, inside this inset.
const STACKED_LINES: f32 = 2.6;
const STACKED_INSET_DIP: f32 = 4.0;
/// The size at which a floating overlay keeps the rows its taskbar would give it.
const REFERENCE_FONT_PT: f32 = 9.0;

pub fn primary_metric(kind: ItemKind) -> MetricKey {
    match kind {
        ItemKind::Cpu => MetricKey::CpuTotal,
        ItemKind::Ram => MetricKey::MemUsedPercent,
        ItemKind::Disk => MetricKey::DiskActivePercent,
        ItemKind::Network => MetricKey::NetDownBps,
        ItemKind::Gpu => MetricKey::GpuUtil,
        ItemKind::Ping => MetricKey::PingMs,
        ItemKind::GpuTemp => MetricKey::GpuTempC,
        ItemKind::CpuTemp => MetricKey::CpuTempC,
        ItemKind::CpuPower => MetricKey::CpuPowerW,
        ItemKind::CpuClock => MetricKey::CpuClockMhz,
        ItemKind::DiskTemp => MetricKey::DiskTempMaxC,
        ItemKind::ChipsetTemp => MetricKey::ChipsetTempC,
        ItemKind::Battery => MetricKey::BatteryPercent,
        ItemKind::Fps => MetricKey::FrameRate,
    }
}

pub fn part_metric(kind: ItemKind, part: CellPart) -> MetricKey {
    match part {
        CellPart::Up => MetricKey::NetUpBps,
        CellPart::DriveActive(d) => MetricKey::DriveActivePercent(d),
        CellPart::DriveUsed(d) => MetricKey::DriveUsedPercent(d),
        CellPart::Gpu(i) if kind == ItemKind::GpuTemp => MetricKey::GpuAdapterTempC(i),
        CellPart::Gpu(i) => MetricKey::GpuAdapterUtil(i),
        CellPart::AdapterDown(i) => MetricKey::NetAdapterDownBps(i),
        CellPart::AdapterUp(i) => MetricKey::NetAdapterUpBps(i),
        CellPart::Main | CellPart::Down => primary_metric(kind),
    }
}

/// The text part of a cell's label. Drive letters and GPU and adapter
/// numbers always show; network arrows give way to the direction icons.
pub fn cell_label(kind: ItemKind, part: CellPart, labels: LabelStyle, icons: bool) -> String {
    let number = |i: u8| u32::from(i) + 1;
    match part {
        CellPart::Down | CellPart::Up if icons => String::new(),
        CellPart::Down => "↓".into(),
        CellPart::Up => "↑".into(),
        CellPart::AdapterDown(i) | CellPart::AdapterUp(i) if icons => number(i).to_string(),
        CellPart::AdapterDown(i) => format!("{}↓", number(i)),
        CellPart::AdapterUp(i) => format!("{}↑", number(i)),
        CellPart::DriveActive(d) | CellPart::DriveUsed(d) => format!("{}:", d as char),
        CellPart::Gpu(i) => {
            let name = match labels {
                LabelStyle::Full => kind.label(),
                LabelStyle::Short => kind.short_label(),
                LabelStyle::None => "",
            };
            format!("{name}{}", number(i))
        }
        CellPart::Main => match labels {
            LabelStyle::Full => kind.label().into(),
            LabelStyle::Short => kind.short_label().into(),
            LabelStyle::None => String::new(),
        },
    }
}

/// A Segoe Fluent Icons glyph for the cell.
pub fn cell_icon(kind: ItemKind, part: CellPart) -> &'static str {
    match (kind, part.base()) {
        (_, CellPart::Down) => "\u{E896}",
        (_, CellPart::Up) => "\u{E898}",
        (ItemKind::Cpu, _) => "\u{E950}",
        (ItemKind::Ram, _) => "\u{EEA0}",
        (ItemKind::Gpu, _) => "\u{F211}",
        (ItemKind::GpuTemp | ItemKind::CpuTemp | ItemKind::DiskTemp | ItemKind::ChipsetTemp, _) => {
            "\u{E9CA}"
        }
        (ItemKind::CpuPower, _) => "\u{E945}",
        (ItemKind::CpuClock, _) => "\u{EC4A}",
        (ItemKind::Disk, _) => "\u{EDA2}",
        (ItemKind::Network, _) => "\u{E968}",
        (ItemKind::Ping, _) => "\u{EC05}",
        (ItemKind::Battery, _) => "\u{E83F}",
        (ItemKind::Fps, _) => "\u{E7FC}",
    }
}

fn stacked_fit_px(tile_h: f32, scale: f32) -> f32 {
    ((tile_h - STACKED_INSET_DIP * scale) / STACKED_LINES).max(1.0)
}

pub fn stacked_text_px(font_px: f32, tile_h: f32, scale: f32) -> f32 {
    (font_px * GRAPH_FONT_SCALE).min(stacked_fit_px(tile_h, scale))
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

/// The network's two directions in the order they are shown.
pub fn network_parts(upload_on_top: bool) -> [CellPart; 2] {
    if upload_on_top {
        [CellPart::Up, CellPart::Down]
    } else {
        [CellPart::Down, CellPart::Up]
    }
}

/// The two directions a network cell or tile shows, in the order they are
/// shown: one adapter's, or the combined ones.
pub fn network_pair(part: CellPart, upload_on_top: bool) -> [CellPart; 2] {
    match part {
        CellPart::AdapterDown(i) | CellPart::AdapterUp(i) => {
            let (down, up) = (CellPart::AdapterDown(i), CellPart::AdapterUp(i));
            if upload_on_top {
                [up, down]
            } else {
                [down, up]
            }
        }
        _ => network_parts(upload_on_top),
    }
}

/// The enabled items in order, each with the cells it draws.
pub fn item_specs(config: &Config, gpus: &GpuSlots) -> Vec<ItemSpec> {
    specs_with(config, &config.gpu_indexes(gpus))
}

/// `item_specs` with `gpus`, the GPUs with cells of their own, already known.
fn specs_with(config: &Config, gpus: &[u8]) -> Vec<ItemSpec> {
    let kinds = config.enabled_items();
    let is_gpu = |k: &ItemKind| matches!(k, ItemKind::Gpu | ItemKind::GpuTemp);
    // With both on, each GPU's load and temperature sit side by side, where
    // the first of the two items is.
    let paired = !gpus.is_empty() && kinds.iter().filter(|k| is_gpu(k)).count() == 2;
    let mut specs = Vec::with_capacity(kinds.len());
    for &kind in &kinds {
        if !(paired && is_gpu(&kind)) {
            specs.push(ItemSpec {
                kind,
                parts: item_parts(kind, config, gpus),
            });
        } else if !specs.iter().any(|s| is_gpu(&s.kind)) {
            let order: Vec<ItemKind> = kinds.iter().copied().filter(is_gpu).collect();
            for &i in gpus {
                specs.extend(order.iter().map(|&kind| ItemSpec {
                    kind,
                    parts: vec![CellPart::Gpu(i)],
                }));
            }
        }
    }
    specs
}

/// One GPU's load and temperature, which share a text column.
fn gpu_pair(a: &[(ItemKind, CellPart)], b: &[(ItemKind, CellPart)]) -> bool {
    matches!(
        (a, b),
        (&[(ka, pa @ CellPart::Gpu(_))], &[(kb, pb)])
            if pa == pb && ka != kb
                && [ka, kb].iter().all(|k| matches!(k, ItemKind::Gpu | ItemKind::GpuTemp))
    )
}

/// The cells `kind` draws; `gpus` are the GPUs with cells of their own.
fn item_parts(kind: ItemKind, config: &Config, gpus: &[u8]) -> Vec<CellPart> {
    let drives = config.drive_letters();
    let upload_on_top = config.display.upload_on_top;
    match kind {
        ItemKind::Disk if !drives.is_empty() => drives
            .iter()
            .map(|&d| match config.disk.value {
                DriveValue::Activity => CellPart::DriveActive(d),
                DriveValue::Used => CellPart::DriveUsed(d),
            })
            .collect(),
        ItemKind::Gpu | ItemKind::GpuTemp if !gpus.is_empty() => {
            gpus.iter().map(|&i| CellPart::Gpu(i)).collect()
        }
        // One chosen adapter takes the place of the combined reading.
        ItemKind::Network if config.network.adapters.len() > 1 => (0..=u8::MAX)
            .take(config.network.adapters.len())
            .flat_map(|i| network_pair(CellPart::AdapterDown(i), upload_on_top))
            .collect(),
        ItemKind::Network => network_parts(upload_on_top).to_vec(),
        _ => default_parts(kind),
    }
}

pub fn compute_layout(items: &[ItemSpec], input: LayoutInput, m: &dyn TextMeasure) -> Layout {
    let scale = input.dpi as f32 / 96.0;
    let font_px = input.font_size_pt * input.dpi as f32 / 72.0;
    let fit = input.fit_font && input.vertical_bar_px.is_none();
    let height = if fit {
        input
            .taskbar_height_px
            .max(fitted_height(input, scale, font_px, m))
    } else {
        input.taskbar_height_px
    };
    let rows = text_rows(
        height,
        fit,
        input.text_spacing.row_gap * scale,
        PADDING_DIP * scale,
        m.line_height(font_px),
    );
    let height = match input.mode {
        DisplayMode::Text => rows.height,
        DisplayMode::Graph => height,
    };
    if items.is_empty() {
        return Layout {
            width: 0.0,
            height,
            font_px,
            cells: Vec::new(),
            vertical: false,
        };
    }
    let labels = Labels {
        style: input.labels,
        icons: input.icons,
    };
    if let Some(bar_w) = input.vertical_bar_px {
        return match input.mode {
            DisplayMode::Text => {
                stacked_text(items, bar_w, scale, font_px, labels, input.text_spacing, m)
            }
            DisplayMode::Graph => stacked_tiles(items, bar_w, scale, font_px, labels, m),
        };
    }
    match input.mode {
        DisplayMode::Text => {
            text_layout(items, rows, scale, font_px, labels, input.text_spacing, m)
        }
        DisplayMode::Graph => graph_layout(items, height, scale, font_px, labels, input, m),
    }
}

/// The height at which the font fits uncut. Text keeps the rows the taskbar
/// gives the default font, rather than reflowing to one long row.
fn fitted_height(input: LayoutInput, scale: f32, font_px: f32, m: &dyn TextMeasure) -> f32 {
    let padding = PADDING_DIP * scale;
    let content = match input.mode {
        DisplayMode::Text => {
            let reference = m.line_height(REFERENCE_FONT_PT * input.dpi as f32 / 72.0);
            let rows = ((input.taskbar_height_px - 2.0 * padding) / reference).floor();
            rows.clamp(1.0, MAX_TEXT_ROWS as f32) * m.line_height(font_px)
        }
        DisplayMode::Graph if input.graph_inline => m.line_height(font_px * GRAPH_FONT_SCALE),
        DisplayMode::Graph => {
            font_px * GRAPH_FONT_SCALE * STACKED_LINES + STACKED_INSET_DIP * scale
        }
    };
    (content + 2.0 * padding).ceil()
}

struct TextRows {
    height: f32,
    count: usize,
    gap: f32,
}

/// Rows are counted before the row gap, so a gap can't drop one, and a height
/// grown to hold the gaps can't add one. The gap then shrinks to what fits.
fn text_rows(height: f32, grow: bool, gap: f32, padding: f32, line_h: f32) -> TextRows {
    // A height fitted to whole rows must not lose one to float rounding.
    let count =
        (((height - 2.0 * padding) / line_h + 1e-3).floor() as usize).clamp(1, MAX_TEXT_ROWS);
    let spare = |height: f32| (height - 2.0 * padding - count as f32 * line_h).max(0.0);
    let gaps = (count - 1) as f32;
    let missing = gaps * gap - spare(height);
    let height = if grow && missing > 0.0 {
        (height + missing).ceil()
    } else {
        height
    };
    let gap = if count > 1 {
        gap.min(spare(height) / gaps)
    } else {
        0.0
    };
    TextRows { height, count, gap }
}

/// Packs items into columns of `rows` cells, column-major. An item's parts
/// (network ↓ and ↑) are kept together when they fit in a column; with
/// several adapters, each adapter's pair is, and so is each GPU's load and
/// temperature.
fn columns(items: &[ItemSpec], rows: usize) -> Vec<Vec<(ItemKind, CellPart)>> {
    let mut groups: Vec<Vec<(ItemKind, CellPart)>> = Vec::new();
    for spec in items {
        let together = if spec.kind == ItemKind::Network {
            2
        } else {
            spec.parts.len()
        };
        for parts in spec.parts.chunks(together.max(1)) {
            let group: Vec<_> = parts.iter().map(|&p| (spec.kind, p)).collect();
            match groups.last_mut() {
                Some(last) if gpu_pair(last, &group) => last.extend(group),
                _ => groups.push(group),
            }
        }
    }
    let mut columns: Vec<Vec<(ItemKind, CellPart)>> = Vec::new();
    for group in groups {
        let free = columns.last().map_or(0, |c| rows - c.len());
        if free == 0 || (group.len() > free && group.len() <= rows) {
            columns.push(Vec::with_capacity(rows));
        }
        for cell in group {
            if columns.last().is_some_and(|c| c.len() == rows) {
                columns.push(Vec::with_capacity(rows));
            }
            columns.last_mut().unwrap().push(cell);
        }
    }
    columns
}

fn text_layout(
    items: &[ItemSpec],
    rows: TextRows,
    scale: f32,
    font_px: f32,
    labels: Labels,
    spacing: TextSpacing,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = PADDING_DIP * scale;
    let line_h = m.line_height(font_px);
    let label_gap = spacing.label_gap * scale;
    let item_gap = spacing.item_gap * scale;
    let TextRows {
        height,
        count: rows,
        gap,
    } = rows;
    let top = ((height - rows as f32 * line_h - (rows - 1) as f32 * gap) / 2.0).max(0.0);

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
        let col_w = label_w + label_gap + value_w;
        for (row, &(kind, part)) in column.iter().enumerate() {
            let y = top + row as f32 * (line_h + gap);
            cells.push(Cell {
                kind,
                part,
                rect: Rect {
                    x,
                    y,
                    w: col_w,
                    h: line_h,
                },
                value_x: x + label_w + label_gap,
                value_y: y,
            });
        }
        x += col_w + item_gap;
    }
    let width = x - item_gap + padding;
    Layout {
        width,
        height,
        font_px,
        cells,
        vertical: false,
    }
}

/// One column for a vertical taskbar: a label beside its value when the bar
/// is wide enough, otherwise above it, in a font small enough to fit the width.
fn stacked_text(
    items: &[ItemSpec],
    bar_w: f32,
    scale: f32,
    font_px: f32,
    labels: Labels,
    spacing: TextSpacing,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = STACKED_PADDING_DIP * scale;
    let available = (bar_w - 2.0 * padding).max(1.0);
    let gap = spacing.label_gap * scale;
    let row_gap = spacing.row_gap * scale;
    let item_gap = spacing.item_gap * 3.0 / 5.0 * scale;
    let widest_label = |px: f32| {
        items
            .iter()
            .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
            .map(|(k, p)| label_width(k, p, labels.style, labels.icons, px, m))
            .fold(0.0, f32::max)
    };
    let widest_text = |px: f32| {
        items
            .iter()
            .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
            .map(|(k, p)| m.width(widest_value(part_metric(k, p).unit()), px))
            .fold(0.0, f32::max)
    };
    let (label_w, value_w) = (widest_label(font_px), widest_text(font_px));
    let labelled = label_w > 0.0;
    let beside = !labelled || label_w + gap + value_w <= available;
    let needed = if !labelled {
        value_w
    } else if beside {
        available
    } else {
        label_w.max(value_w)
    };
    let font_px = font_px * (available / needed.max(1.0)).min(1.0);
    let label_w = widest_label(font_px);
    let line_h = m.line_height(font_px);

    let mut cells = Vec::new();
    let mut y = padding;
    for spec in items {
        for (i, &part) in spec.parts.iter().enumerate() {
            if i > 0 {
                y += row_gap;
            }
            let (value_x, value_y, h) = if !labelled {
                (padding, y, line_h)
            } else if beside {
                (padding + label_w + gap, y, line_h)
            } else {
                (padding, y + line_h, 2.0 * line_h)
            };
            cells.push(Cell {
                kind: spec.kind,
                part,
                rect: Rect {
                    x: padding,
                    y,
                    w: available,
                    h,
                },
                value_x,
                value_y,
            });
            y += h;
        }
        y += item_gap;
    }
    Layout {
        width: bar_w,
        height: y - item_gap + padding,
        font_px,
        cells,
        vertical: true,
    }
}

/// A full-width graph tile per part for a vertical taskbar; network splits
/// into one tile per direction because both will not fit on one line.
fn stacked_tiles(
    items: &[ItemSpec],
    bar_w: f32,
    scale: f32,
    font_px: f32,
    labels: Labels,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = STACKED_PADDING_DIP * scale;
    let tile_w = (bar_w - 2.0 * padding).max(1.0);
    let available = (tile_w - (PADDING_DIP + 1.0) * scale).max(1.0);
    let small_px = font_px * GRAPH_FONT_SCALE;
    let widest = items
        .iter()
        .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
        .map(|(k, p)| {
            label_width(k, p, labels.style, labels.icons, small_px, m)
                .max(m.width(widest_value(part_metric(k, p).unit()), small_px))
        })
        .fold(0.0, f32::max);
    let small_px = small_px * (available / widest.max(1.0)).min(1.0);
    let tile_h =
        (MIN_STACKED_TILE_DIP * scale).max(small_px * STACKED_LINES + STACKED_INSET_DIP * scale);

    let mut cells = Vec::new();
    let mut y = padding;
    for spec in items {
        for &part in &spec.parts {
            cells.push(Cell {
                kind: spec.kind,
                part,
                rect: Rect {
                    x: padding,
                    y,
                    w: tile_w,
                    h: tile_h,
                },
                value_x: padding + PADDING_DIP * scale,
                value_y: y,
            });
            y += tile_h + STACKED_TILE_GAP_DIP * scale;
        }
    }
    Layout {
        width: bar_w,
        height: y - STACKED_TILE_GAP_DIP * scale + padding,
        font_px: small_px / GRAPH_FONT_SCALE,
        cells,
        vertical: true,
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
        ItemKind::Network => {
            label(network_pair(part, false)[0]) + icon_gap(font_px) + m.width(widest, font_px)
        }
        _ => label(part).max(m.width(widest, font_px)),
    }
}

/// A label and its value on an inline graph tile's single line; x is
/// relative to the tile's text origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineSlot {
    pub part: CellPart,
    pub label_x: f32,
    pub value_x: f32,
}

/// How a tile's text is labelled.
#[derive(Clone, Copy, Debug)]
pub struct TileText {
    pub labels: LabelStyle,
    pub icons: bool,
    pub upload_on_top: bool,
}

/// Where an inline graph tile draws each of its parts, and the width of the
/// whole line. `font_px` is the size the text is drawn at.
pub fn inline_slots(
    kind: ItemKind,
    part: CellPart,
    text: TileText,
    font_px: f32,
    scale: f32,
    m: &dyn TextMeasure,
) -> (Vec<InlineSlot>, f32) {
    let TileText { labels, icons, .. } = text;
    let parts = match kind {
        ItemKind::Network => network_pair(part, text.upload_on_top).to_vec(),
        _ => vec![part],
    };
    let gap = INLINE_GAP_DIP * scale;
    let mut slots = Vec::with_capacity(parts.len());
    let mut x = 0.0;
    for part in parts {
        if !slots.is_empty() {
            x += 2.0 * gap;
        }
        let value_x = x + label_width(kind, part, labels, icons, font_px, m) + gap;
        slots.push(InlineSlot {
            part,
            label_x: x,
            value_x,
        });
        x = value_x + m.width(widest_value(part_metric(kind, part).unit()), font_px);
    }
    (slots, x)
}

fn graph_layout(
    items: &[ItemSpec],
    height: f32,
    scale: f32,
    font_px: f32,
    labels: Labels,
    input: LayoutInput,
    m: &dyn TextMeasure,
) -> Layout {
    let inline = input.graph_inline;
    let padding = PADDING_DIP * scale;
    let tile_h = (height - 2.0 * padding).max(1.0);
    // A short tile stops widening once its text stops growing.
    let stacked_px = font_px.min(stacked_fit_px(tile_h, scale) / GRAPH_FONT_SCALE);
    let mut cells = Vec::with_capacity(items.len());
    let mut x = padding;
    for spec in items {
        // Network draws both directions in one tile, one per adapter when
        // there are several; other items get a tile per part.
        let tiles = if spec.kind == ItemKind::Network {
            let adapters: Vec<CellPart> = spec
                .parts
                .iter()
                .filter(|p| matches!(p, CellPart::AdapterDown(_)))
                .copied()
                .collect();
            if adapters.is_empty() {
                vec![CellPart::Main]
            } else {
                adapters
            }
        } else {
            spec.parts.clone()
        };
        for part in tiles {
            let text_w = if inline {
                let text_px = font_px * GRAPH_FONT_SCALE;
                let text = TileText {
                    labels: labels.style,
                    icons: labels.icons,
                    upload_on_top: input.upload_on_top,
                };
                inline_slots(spec.kind, part, text, text_px, scale, m).1
            } else {
                graph_text_width(spec.kind, part, labels, stacked_px, m)
            };
            let w = (text_w + 2.0 * padding).max(MIN_TILE_DIP * scale);
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
                value_y: padding,
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
        vertical: false,
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
            fit_font: false,
            dpi,
            font_size_pt: 9.0,
            labels: LabelStyle::Full,
            icons: false,
            graph_inline: false,
            upload_on_top: false,
            vertical_bar_px: None,
            text_spacing: TextSpacing::default(),
        }
    }

    fn vertical(mode: DisplayMode, bar_w: f32) -> LayoutInput {
        LayoutInput {
            vertical_bar_px: Some(bar_w),
            ..input(mode, bar_w, 96)
        }
    }

    #[test]
    fn narrow_vertical_bar_puts_values_under_labels_in_a_fitting_font() {
        let l = compute_layout(&specs(&ALL), vertical(DisplayMode::Text, 48.0), &Fixed);
        assert!(l.vertical);
        assert_eq!(l.width, 48.0);
        assert!(
            l.font_px < 12.0,
            "99.9 MB/s must shrink to fit: {}",
            l.font_px
        );
        assert!(Fixed.width("99.9 MB/s", l.font_px) <= 44.0 + 0.01);
        assert_no_overlap(&l);
        assert_inside(&l);
        for c in &l.cells {
            assert_eq!(c.value_x, c.rect.x);
            assert_eq!(c.value_y, c.rect.y + l.font_px * 1.25);
        }
    }

    #[test]
    fn wide_vertical_bar_keeps_labels_beside_values_at_full_size() {
        let l = compute_layout(&specs(&ALL), vertical(DisplayMode::Text, 200.0), &Fixed);
        assert_eq!(l.font_px, 12.0);
        assert_no_overlap(&l);
        for c in &l.cells {
            assert!(c.value_x > c.rect.x);
            assert_eq!(c.value_y, c.rect.y);
        }
    }

    #[test]
    fn unlabelled_vertical_bar_takes_one_line_per_cell() {
        let i = LayoutInput {
            labels: LabelStyle::None,
            ..vertical(DisplayMode::Text, 48.0)
        };
        let l = compute_layout(&specs(&[ItemKind::Cpu, ItemKind::Ram]), i, &Fixed);
        assert_eq!(l.font_px, 12.0);
        for c in &l.cells {
            assert_eq!((c.value_x, c.value_y), (c.rect.x, c.rect.y));
        }
    }

    #[test]
    fn vertical_graph_has_full_width_tiles_with_network_split() {
        let l = compute_layout(&specs(&ALL), vertical(DisplayMode::Graph, 48.0), &Fixed);
        assert_no_overlap(&l);
        assert_inside(&l);
        let net: Vec<_> = l
            .cells
            .iter()
            .filter(|c| c.kind == ItemKind::Network)
            .map(|c| c.part)
            .collect();
        assert_eq!(net, [CellPart::Down, CellPart::Up]);
        assert!(l.cells.iter().all(|c| c.rect.w == 44.0));
        assert!(Fixed.width("99.9 MB/s", l.font_px * GRAPH_FONT_SCALE) <= 39.0 + 0.01);
    }

    #[test]
    fn empty_vertical_bar_has_no_width() {
        let l = compute_layout(&specs(&[]), vertical(DisplayMode::Text, 48.0), &Fixed);
        assert_eq!(l.width, 0.0);
    }

    fn sized(mode: DisplayMode, inline: bool, fit_font: bool, pt: f32) -> Layout {
        let mut i = input(mode, 48.0, 96);
        i.graph_inline = inline;
        i.fit_font = fit_font;
        i.font_size_pt = pt;
        compute_layout(&specs(&ALL), i, &Fixed)
    }

    fn rows(l: &Layout) -> usize {
        l.cells
            .iter()
            .map(|c| c.rect.y as i32)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }

    #[test]
    fn docked_graph_tiles_stop_widening_when_their_text_stops_growing() {
        // 40px tile: (40 - 4) / 2.6 = 13.85px of text, reached past 12pt.
        let at = |pt| sized(DisplayMode::Graph, false, false, pt);
        assert_eq!(at(20.0).height, 48.0);
        assert_eq!(at(20.0).width, at(14.0).width);
        assert!(at(12.0).width > at(9.0).width);
    }

    #[test]
    fn docked_height_is_always_the_taskbars() {
        for mode in [DisplayMode::Graph, DisplayMode::Text] {
            for inline in [false, true] {
                for (bar, dpi) in [(32.0, 96), (48.0, 96), (60.0, 120), (72.0, 144)] {
                    for pt in 6..=20 {
                        let mut i = input(mode, bar, dpi);
                        i.graph_inline = inline;
                        i.font_size_pt = pt as f32;
                        assert_eq!(compute_layout(&specs(&ALL), i, &Fixed).height, bar);
                    }
                }
            }
        }
    }

    #[test]
    fn fit_font_leaves_the_default_size_alone() {
        for (mode, inline) in [
            (DisplayMode::Graph, false),
            (DisplayMode::Graph, true),
            (DisplayMode::Text, false),
        ] {
            assert_eq!(
                sized(mode, inline, true, 9.0),
                sized(mode, inline, false, 9.0)
            );
        }
    }

    #[test]
    fn fit_font_grows_stacked_tiles_so_the_text_keeps_growing() {
        let mut last = 0.0;
        for pt in [12.0, 13.0, 14.0, 16.0, 18.0, 20.0] {
            let l = sized(DisplayMode::Graph, false, true, pt);
            let tile = l.cells[0].rect.h;
            let want = l.font_px * GRAPH_FONT_SCALE;
            assert!(
                (stacked_text_px(l.font_px, tile, 1.0) - want).abs() < 0.01,
                "{pt}"
            );
            assert!(l.width > last, "{pt}");
            last = l.width;
            assert_no_overlap(&l);
            assert_inside(&l);
        }
    }

    #[test]
    fn fit_font_keeps_text_rows_and_grows_to_hold_them() {
        let l = sized(DisplayMode::Text, false, true, 20.0);
        assert_eq!(rows(&l), 2);
        assert!(l.height > 48.0);
        assert_inside(&l);
        assert_eq!(rows(&sized(DisplayMode::Text, false, false, 20.0)), 1);
    }

    #[test]
    fn fit_font_keeps_text_rows_at_every_size_and_scale() {
        for (bar, dpi) in [
            (32.0, 96),
            (48.0, 96),
            (76.0, 96),
            (60.0, 120),
            (72.0, 144),
            (84.0, 168),
        ] {
            let at = |pt: i32| {
                let mut i = input(DisplayMode::Text, bar, dpi);
                i.fit_font = true;
                i.font_size_pt = pt as f32;
                compute_layout(&specs(&ALL), i, &Fixed)
            };
            let want = rows(&at(9));
            for pt in 9..=20 {
                let l = at(pt);
                assert_eq!(rows(&l), want, "{bar} {dpi} {pt}");
                assert_inside(&l);
            }
        }
    }

    #[test]
    fn fit_font_does_not_change_a_vertical_bar() {
        for mode in [DisplayMode::Graph, DisplayMode::Text] {
            let mut i = vertical(mode, 76.0);
            i.font_size_pt = 20.0;
            let fitted = LayoutInput {
                fit_font: true,
                ..i
            };
            assert_eq!(
                compute_layout(&specs(&ALL), fitted, &Fixed),
                compute_layout(&specs(&ALL), i, &Fixed)
            );
        }
    }

    const WIDEST: TextSpacing = TextSpacing {
        item_gap: 40.0,
        label_gap: 16.0,
        row_gap: 16.0,
    };
    const TIGHTEST: TextSpacing = TextSpacing {
        item_gap: 0.0,
        label_gap: 0.0,
        row_gap: 0.0,
    };
    const BARS: [(f32, u32); 6] = [
        (32.0, 96),
        (48.0, 96),
        (76.0, 96),
        (60.0, 120),
        (72.0, 144),
        (84.0, 168),
    ];

    fn spaced(mut i: LayoutInput, spacing: TextSpacing) -> Layout {
        i.text_spacing = spacing;
        compute_layout(&specs(&ALL), i, &Fixed)
    }

    #[test]
    fn default_spacing_matches_the_config_defaults() {
        let d = Config::default().display;
        assert_eq!(
            (d.text_item_gap, d.text_label_gap, d.text_row_gap),
            (10, 4, 0)
        );
        assert_eq!(
            TextSpacing::default(),
            TextSpacing {
                item_gap: 10.0,
                label_gap: 4.0,
                row_gap: 0.0
            }
        );
    }

    #[test]
    fn default_spacing_keeps_todays_text_positions() {
        // Labels 18px, values 24px, 4px between them: 46px columns, 10px apart.
        let items = [ItemKind::Cpu, ItemKind::Ram, ItemKind::Gpu];
        let l = compute_layout(&specs(&items), input(DisplayMode::Text, 48.0, 96), &Fixed);
        let at: Vec<_> = l.cells.iter().map(|c| (c.rect.x, c.rect.y)).collect();
        assert_eq!(at, [(4.0, 9.0), (4.0, 24.0), (60.0, 9.0)]);
        assert_eq!(l.cells[0].value_x, 26.0);
        assert_eq!((l.width, l.height), (110.0, 48.0));
    }

    #[test]
    fn default_spacing_keeps_todays_vertical_positions() {
        // One 15px line per cell, 6px between items, and none inside one.
        let items = [ItemKind::Cpu, ItemKind::Network, ItemKind::Ram];
        let mut i = vertical(DisplayMode::Text, 200.0);
        let at = |i| {
            let l = compute_layout(&specs(&items), i, &Fixed);
            let ys: Vec<_> = l.cells.iter().map(|c| c.rect.y).collect();
            (ys, l.height)
        };
        assert_eq!(at(i), (vec![2.0, 23.0, 38.0, 59.0], 76.0));
        i.dpi = 144;
        i.font_size_pt = 6.0;
        assert_eq!(at(i), (vec![3.0, 27.0, 42.0, 66.0], 84.0));
    }

    #[test]
    fn spacing_leaves_graphs_alone() {
        for (bar, dpi) in BARS {
            for inline in [false, true] {
                for fit_font in [false, true] {
                    let mut i = input(DisplayMode::Graph, bar, dpi);
                    i.graph_inline = inline;
                    i.fit_font = fit_font;
                    assert_eq!(spaced(i, WIDEST), spaced(i, TextSpacing::default()));
                }
            }
        }
        for bar in [48.0, 76.0, 200.0] {
            let i = vertical(DisplayMode::Graph, bar);
            assert_eq!(spaced(i, WIDEST), spaced(i, TextSpacing::default()));
        }
    }

    #[test]
    fn item_and_label_gaps_widen_columns_at_any_scale() {
        for (bar, dpi) in BARS {
            let i = input(DisplayMode::Text, bar, dpi);
            let (base, wide) = (spaced(i, TextSpacing::default()), spaced(i, WIDEST));
            assert!(wide.width > base.width, "{bar} {dpi}");
            let scale = dpi as f32 / 96.0;
            let c = |l: &Layout| l.cells[0].value_x - l.cells[0].rect.x;
            assert!((c(&wide) - c(&base) - 12.0 * scale).abs() < 0.01);
        }
    }

    #[test]
    fn no_overlap_at_the_extremes() {
        let cases = BARS.into_iter().flat_map(|(bar, dpi)| {
            [
                input(DisplayMode::Text, bar, dpi),
                LayoutInput {
                    fit_font: true,
                    ..input(DisplayMode::Text, bar, dpi)
                },
                LayoutInput {
                    dpi,
                    ..vertical(DisplayMode::Text, bar)
                },
                LayoutInput {
                    dpi,
                    ..vertical(DisplayMode::Text, 3.0 * bar)
                },
            ]
        });
        for mut i in cases {
            for pt in [6.0, 9.0, 13.0, 20.0] {
                i.font_size_pt = pt;
                for spacing in [WIDEST, TIGHTEST] {
                    let l = spaced(i, spacing);
                    assert_eq!(l.cells.len(), ALL.len() + 1);
                    assert_no_overlap(&l);
                    if Fixed.line_height(l.font_px) <= l.height {
                        assert_inside(&l);
                    }
                }
            }
        }
    }

    #[test]
    fn the_row_gap_never_changes_the_docked_row_count() {
        for (bar, dpi) in BARS {
            for pt in 6..=20 {
                let mut i = input(DisplayMode::Text, bar, dpi);
                i.font_size_pt = pt as f32;
                let (base, wide) = (spaced(i, TextSpacing::default()), spaced(i, WIDEST));
                assert_eq!(rows(&wide), rows(&base), "{bar} {dpi} {pt}");
                assert_eq!(wide.height, bar);
                assert_rows_stay_within_padding(&wide, dpi);
            }
        }
    }

    #[test]
    fn a_docked_row_gap_shrinks_to_what_fits() {
        // Two 15px rows on a 48px bar leave 10px inside the padding.
        let i = input(DisplayMode::Text, 48.0, 96);
        let row_y = |gap| {
            let l = spaced(
                i,
                TextSpacing {
                    row_gap: gap,
                    ..TextSpacing::default()
                },
            );
            (l.cells[0].rect.y, l.cells[1].rect.y)
        };
        assert_eq!(row_y(0.0), (9.0, 24.0));
        assert_eq!(row_y(4.0), (7.0, 26.0));
        assert_eq!(row_y(10.0), (4.0, 29.0));
        assert_eq!(row_y(16.0), (4.0, 29.0));
    }

    #[test]
    fn a_floating_row_gap_grows_the_height_and_keeps_the_rows() {
        for (bar, dpi) in BARS {
            let scale = dpi as f32 / 96.0;
            for pt in 6..=20 {
                let mut i = input(DisplayMode::Text, bar, dpi);
                i.fit_font = true;
                i.font_size_pt = pt as f32;
                let (base, wide) = (spaced(i, TextSpacing::default()), spaced(i, WIDEST));
                assert_eq!(rows(&wide), rows(&base), "{bar} {dpi} {pt}");
                assert_eq!(wide.height, wide.height.ceil());
                assert_inside(&wide);
                assert_rows_stay_within_padding(&wide, dpi);
                let ys = wide.cells.iter().map(|c| c.rect.y);
                let drawn = ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min);
                let full =
                    (rows(&wide) - 1) as f32 * (Fixed.line_height(wide.font_px) + 16.0 * scale);
                assert!((drawn - full).abs() < 0.01, "{bar} {dpi} {pt}");
            }
        }
    }

    #[test]
    fn a_floating_row_gap_that_fits_does_not_grow_the_overlay() {
        // At 6pt, three 10px rows leave 10px inside a 48px bar's padding.
        let mut i = input(DisplayMode::Text, 48.0, 96);
        i.fit_font = true;
        i.font_size_pt = 6.0;
        let gap = |row_gap| TextSpacing {
            row_gap,
            ..TextSpacing::default()
        };
        assert_eq!(spaced(i, gap(5.0)).height, 48.0);
        assert_eq!(spaced(i, gap(6.0)).height, 50.0);
    }

    #[test]
    fn a_vertical_bar_keeps_its_rows_and_font_at_the_widest_gaps() {
        for bar in [48.0, 76.0, 200.0, 400.0] {
            for pt in [6.0, 9.0, 20.0] {
                let mut i = vertical(DisplayMode::Text, bar);
                i.font_size_pt = pt;
                let (base, wide) = (spaced(i, TextSpacing::default()), spaced(i, WIDEST));
                assert_eq!(wide.cells.len(), base.cells.len());
                assert_eq!(rows(&wide), rows(&base), "{bar} {pt}");
                assert_eq!(wide.font_px, base.font_px, "{bar} {pt}");
                assert_eq!(wide.width, bar);
                assert!(wide.height > base.height);
            }
        }
    }

    #[test]
    fn a_vertical_bar_spaces_items_by_three_fifths_of_the_item_gap() {
        let items = [ItemKind::Cpu, ItemKind::Network];
        let mut i = vertical(DisplayMode::Text, 200.0);
        i.text_spacing = TextSpacing {
            item_gap: 40.0,
            label_gap: 4.0,
            row_gap: 3.0,
        };
        let l = compute_layout(&specs(&items), i, &Fixed);
        let ys: Vec<_> = l.cells.iter().map(|c| c.rect.y).collect();
        assert_eq!(ys, [2.0, 2.0 + 15.0 + 24.0, 2.0 + 15.0 + 24.0 + 15.0 + 3.0]);
    }

    fn assert_rows_stay_within_padding(l: &Layout, dpi: u32) {
        let padding = PADDING_DIP * dpi as f32 / 96.0;
        let bottom = l
            .cells
            .iter()
            .map(|c| c.rect.y + c.rect.h)
            .fold(0.0, f32::max);
        if rows(l) > 1 {
            assert!(l.cells[0].rect.y >= padding - 0.01, "{:?}", l.cells[0]);
            assert!(bottom <= l.height - padding + 0.01, "{bottom} {}", l.height);
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

    const ALL: [ItemKind; 14] = ItemKind::ALL;

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
    fn inline_graph_tiles_put_label_and_value_side_by_side() {
        let mut i = input(DisplayMode::Graph, 32.0, 96);
        i.graph_inline = true;
        let l = compute_layout(&specs(&ALL), i, &Fixed);
        assert_eq!(l.cells.len(), ALL.len());
        assert_no_overlap(&l);
        assert_inside(&l);
        // Text is 12px × 0.85 = 10.2px, so 5.1px a character:
        // "CPU" + 4px gap + "100%" + 2 × 4px padding.
        let cpu = l.cells.iter().find(|c| c.kind == ItemKind::Cpu).unwrap();
        assert!((cpu.rect.w - (15.3 + 4.0 + 20.4 + 8.0)).abs() < 0.01);
        let stacked = compute_layout(&specs(&ALL), input(DisplayMode::Graph, 32.0, 96), &Fixed);
        assert!(l.width > stacked.width, "one line needs wider tiles");
    }

    #[test]
    fn inline_slots_follow_each_other_without_overlap() {
        let text = TileText {
            labels: LabelStyle::Full,
            icons: true,
            upload_on_top: false,
        };
        let slots = |kind, part| inline_slots(kind, part, text, 10.0, 1.0, &Fixed);
        let (cpu, cpu_w) = slots(ItemKind::Cpu, CellPart::Main);
        assert_eq!(cpu.len(), 1);
        // Icon 10px + icon gap 3px + "CPU" 15px, then a 4px gap.
        assert_eq!((cpu[0].label_x, cpu[0].value_x), (0.0, 32.0));
        assert_eq!(cpu_w, 32.0 + 20.0);

        let (net, net_w) = slots(ItemKind::Network, CellPart::Main);
        let parts: Vec<CellPart> = net.iter().map(|s| s.part).collect();
        assert_eq!(parts, [CellPart::Down, CellPart::Up]);
        // "99.9 MB/s" is 45px wide; the second part starts 8px after it.
        assert_eq!(net[1].label_x, net[0].value_x + 45.0 + 8.0);
        assert_eq!(net_w, net[1].value_x + 45.0);

        let up_first = TileText {
            upload_on_top: true,
            ..text
        };
        let (net, _) = inline_slots(
            ItemKind::Network,
            CellPart::Main,
            up_first,
            10.0,
            1.0,
            &Fixed,
        );
        assert_eq!(net[0].part, CellPart::Up);
    }

    #[test]
    fn upload_on_top_puts_the_upload_cell_on_top() {
        let mut c = Config::default();
        c.display.upload_on_top = true;
        let net = item_specs(&c, &GpuSlots::new())
            .into_iter()
            .find(|s| s.kind == ItemKind::Network)
            .unwrap();
        assert_eq!(net.parts, [CellPart::Up, CellPart::Down]);
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
        let disk = item_specs(&c, &GpuSlots::new())
            .into_iter()
            .find(|s| s.kind == ItemKind::Disk)
            .unwrap();
        assert_eq!(
            disk.parts,
            [CellPart::DriveActive(b'C'), CellPart::DriveActive(b'D')]
        );
        c.disk.value = DriveValue::Used;
        let disk = item_specs(&c, &GpuSlots::new())
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
    fn chosen_gpus_get_a_cell_each_for_load_and_temperature() {
        let c = Config::default();
        assert_eq!(item_parts(ItemKind::Gpu, &c, &[]), [CellPart::Main]);
        for kind in [ItemKind::Gpu, ItemKind::GpuTemp] {
            assert_eq!(
                item_parts(kind, &c, &[0, 2]),
                [CellPart::Gpu(0), CellPart::Gpu(2)]
            );
        }
        assert_eq!(
            part_metric(ItemKind::Gpu, CellPart::Gpu(2)),
            MetricKey::GpuAdapterUtil(2)
        );
        assert_eq!(
            part_metric(ItemKind::GpuTemp, CellPart::Gpu(2)),
            MetricKey::GpuAdapterTempC(2)
        );
        let label = |kind, style| cell_label(kind, CellPart::Gpu(1), style, false);
        assert_eq!(label(ItemKind::Gpu, LabelStyle::Full), "GPU2");
        assert_eq!(label(ItemKind::GpuTemp, LabelStyle::Short), "T2");
        assert_eq!(
            label(ItemKind::Gpu, LabelStyle::None),
            "2",
            "the number stays"
        );
    }

    #[test]
    fn each_gpus_load_and_temperature_sit_side_by_side() {
        let mut c = Config::default();
        let order = [ItemKind::Gpu, ItemKind::Cpu, ItemKind::GpuTemp];
        c.items.retain(|i| order.contains(&i.kind));
        c.items
            .sort_by_key(|i| order.iter().position(|&k| k == i.kind));
        c.items.iter_mut().for_each(|i| i.enabled = true);
        let cells = |specs: &[ItemSpec]| {
            specs
                .iter()
                .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
                .collect::<Vec<_>>()
        };
        let items = specs_with(&c, &[0, 2]);
        assert_eq!(
            cells(&items),
            [
                (ItemKind::Gpu, CellPart::Gpu(0)),
                (ItemKind::GpuTemp, CellPart::Gpu(0)),
                (ItemKind::Gpu, CellPart::Gpu(2)),
                (ItemKind::GpuTemp, CellPart::Gpu(2)),
                (ItemKind::Cpu, CellPart::Main),
            ]
        );
        let mut items_after_cpu = specs(&[ItemKind::Cpu]);
        items_after_cpu.extend(items.into_iter().filter(|s| s.kind != ItemKind::Cpu));
        let l = compute_layout(&items_after_cpu, input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_no_overlap(&l);
        let x = |kind, i| {
            l.cells
                .iter()
                .find(|c| c.kind == kind && c.part == CellPart::Gpu(i))
                .unwrap()
                .rect
                .x
        };
        for i in [0, 2] {
            assert_eq!(x(ItemKind::Gpu, i), x(ItemKind::GpuTemp, i), "GPU {i}");
        }
        c.items.retain(|i| i.kind != ItemKind::GpuTemp);
        assert_eq!(
            cells(&specs_with(&c, &[0, 2]))[..2],
            [
                (ItemKind::Gpu, CellPart::Gpu(0)),
                (ItemKind::Gpu, CellPart::Gpu(2))
            ],
            "load alone keeps its cells together"
        );
    }

    #[test]
    fn several_adapters_get_a_pair_of_cells_each() {
        let mut c = Config::default();
        c.network.adapters = vec!["eth".into()];
        assert_eq!(
            item_parts(ItemKind::Network, &c, &[]),
            [CellPart::Down, CellPart::Up],
            "one adapter takes the combined cells"
        );
        c.network.adapters.push("wifi".into());
        assert_eq!(
            item_parts(ItemKind::Network, &c, &[]),
            [
                CellPart::AdapterDown(0),
                CellPart::AdapterUp(0),
                CellPart::AdapterDown(1),
                CellPart::AdapterUp(1)
            ]
        );
        c.display.upload_on_top = true;
        assert_eq!(
            item_parts(ItemKind::Network, &c, &[])[..2],
            [CellPart::AdapterUp(0), CellPart::AdapterDown(0)]
        );
        assert_eq!(
            part_metric(ItemKind::Network, CellPart::AdapterUp(1)),
            MetricKey::NetAdapterUpBps(1)
        );
        let label = |part, icons| cell_label(ItemKind::Network, part, LabelStyle::Full, icons);
        assert_eq!(label(CellPart::AdapterDown(0), false), "1↓");
        assert_eq!(label(CellPart::AdapterUp(1), false), "2↑");
        assert_eq!(
            label(CellPart::AdapterUp(1), true),
            "2",
            "the icon shows the arrow"
        );
        assert_eq!(
            cell_icon(ItemKind::Network, CellPart::AdapterUp(1)),
            "\u{E898}"
        );
        assert_eq!(CellPart::AdapterDown(3).base(), CellPart::Down);
        assert_eq!(CellPart::Gpu(3).base(), CellPart::Main);
    }

    fn adapter_specs(n: u8) -> Vec<ItemSpec> {
        vec![ItemSpec {
            kind: ItemKind::Network,
            parts: (0..n)
                .flat_map(|i| [CellPart::AdapterDown(i), CellPart::AdapterUp(i)])
                .collect(),
        }]
    }

    #[test]
    fn each_adapters_pair_shares_a_text_column() {
        let mut items = specs(&[ItemKind::Cpu]);
        items.extend(adapter_specs(2));
        let l = compute_layout(&items, input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_no_overlap(&l);
        for i in 0..2 {
            let x = |part| l.cells.iter().find(|c| c.part == part).unwrap().rect.x;
            assert_eq!(x(CellPart::AdapterDown(i)), x(CellPart::AdapterUp(i)));
        }
    }

    #[test]
    fn each_adapter_gets_its_own_graph_tile() {
        for inline in [false, true] {
            let mut i = input(DisplayMode::Graph, 48.0, 96);
            i.graph_inline = inline;
            let l = compute_layout(&adapter_specs(2), i, &Fixed);
            let parts: Vec<CellPart> = l.cells.iter().map(|c| c.part).collect();
            assert_eq!(parts, [CellPart::AdapterDown(0), CellPart::AdapterDown(1)]);
            assert_no_overlap(&l);
        }
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
