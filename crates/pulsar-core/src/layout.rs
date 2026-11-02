//! Pure layout: turns the enabled items, display mode and taskbar metrics into
//! rectangles. All values are physical pixels.

use crate::config::{Config, Display, DisplayMode, DriveValue, LabelStyle, NetworkShow};
use crate::format::widest_value;
use crate::gpu_slots::GpuSlots;
use crate::metric::{ItemKind, MetricKey};

pub mod fit;
mod names;
mod tile;

pub use names::ItemNames;
pub use tile::{
    StackedLabel, TileGraphic, TileLook, TileRegions, heat_rows, stacked_label, tile_chrome,
    tile_regions,
};

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
    /// The item's whole reading; for network, both directions together.
    Main,
    Down,
    Up,
    /// Per-drive activity or used space, by drive letter.
    DriveActive(u8),
    DriveUsed(u8),
    /// One GPU's load or temperature, by its slot in `GpuSlots`.
    Gpu(u8),
    /// One adapter's traffic in one direction, by its place in
    /// `NetworkConfig::adapters`.
    AdapterDown(u8),
    AdapterUp(u8),
    /// Both directions of one adapter.
    AdapterPair(u8),
}

impl CellPart {
    /// The part of a combined item this one shares its color with.
    pub fn base(self) -> CellPart {
        match self {
            CellPart::DriveActive(_)
            | CellPart::DriveUsed(_)
            | CellPart::Gpu(_)
            | CellPart::AdapterPair(_) => CellPart::Main,
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
    /// The size a graph tile draws its text at; shared by measuring and drawing.
    pub text_px: f32,
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
pub struct LayoutInput<'a> {
    pub mode: DisplayMode,
    pub taskbar_height_px: f32,
    /// Not limited by a taskbar: grow taller than it so the font fits.
    pub fit_font: bool,
    pub dpi: u32,
    pub font_size_pt: f32,
    pub labels: LabelStyle,
    pub icons: bool,
    pub graph_inline: bool,
    pub vertical_labels: bool,
    pub upload_on_top: bool,
    /// The width of a vertical taskbar; items then stack from the top.
    pub vertical_bar_px: Option<f32>,
    pub text_spacing: TextSpacing,
    pub names: &'a ItemNames,
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

/// A requested graph tile size, in DIPs; `None` is automatic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileSize {
    pub width: Option<u16>,
    pub height: Option<u16>,
}

/// An enabled item and the cells it draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSpec {
    pub kind: ItemKind,
    pub parts: Vec<CellPart>,
    /// Applies to each of the item's tiles.
    pub tile: TileSize,
    pub graphic: TileGraphic,
}

const PADDING_DIP: f32 = 4.0;
const INLINE_GAP_DIP: f32 = 4.0;
const TILE_GAP_DIP: f32 = 4.0;
const MIN_TILE_DIP: f32 = 44.0;
const MAX_TEXT_ROWS: usize = 3;
const STACKED_PADDING_DIP: f32 = 2.0;
const STACKED_TILE_GAP_DIP: f32 = 2.0;
const MIN_STACKED_TILE_DIP: f32 = 36.0;
const MAX_TILE_HEIGHT_DIP: f32 = 200.0;
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
        ItemKind::GpuMemory => MetricKey::GpuMemoryPercent,
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
        CellPart::Gpu(i) if kind == ItemKind::GpuMemory => MetricKey::GpuAdapterMemoryPercent(i),
        CellPart::Gpu(i) => MetricKey::GpuAdapterUtil(i),
        CellPart::AdapterDown(i) | CellPart::AdapterPair(i) => MetricKey::NetAdapterDownBps(i),
        CellPart::AdapterUp(i) => MetricKey::NetAdapterUpBps(i),
        CellPart::Main | CellPart::Down => primary_metric(kind),
    }
}

/// The text part of a cell's label. Drive letters and GPU and adapter
/// numbers always show; network arrows give way to the direction icons or a
/// custom name, which shows unless labels are off.
pub fn cell_label(
    kind: ItemKind,
    part: CellPart,
    labels: LabelStyle,
    icons: bool,
    names: &ItemNames,
) -> String {
    let number = |i: u8| u32::from(i) + 1;
    let direction = match part {
        CellPart::Down | CellPart::AdapterDown(_) => names.down(),
        CellPart::Up | CellPart::AdapterUp(_) => names.up(),
        _ => None,
    };
    if let Some(name) = direction.filter(|_| labels != LabelStyle::None) {
        return match part {
            CellPart::AdapterDown(i) | CellPart::AdapterUp(i) => format!("{name}{}", number(i)),
            _ => name.to_string(),
        };
    }
    match part {
        CellPart::Down | CellPart::Up if icons => String::new(),
        CellPart::Down => "↓".into(),
        CellPart::Up => "↑".into(),
        CellPart::AdapterDown(i) | CellPart::AdapterUp(i) if icons => number(i).to_string(),
        CellPart::AdapterDown(i) => format!("{}↓", number(i)),
        CellPart::AdapterUp(i) => format!("{}↑", number(i)),
        CellPart::AdapterPair(i) => number(i).to_string(),
        CellPart::DriveActive(d) | CellPart::DriveUsed(d) => format!("{}:", d as char),
        CellPart::Gpu(i) => {
            let name = match labels {
                LabelStyle::Full => names.full(kind),
                LabelStyle::Short => names.short(kind),
                LabelStyle::None => "",
            };
            format!("{name}{}", number(i))
        }
        CellPart::Main => match labels {
            LabelStyle::Full => names.full(kind).into(),
            LabelStyle::Short => names.short(kind).into(),
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
        (ItemKind::GpuMemory, _) => "\u{E7F4}",
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

/// The text size of a tile that may be `shorter` than its automatic height.
/// Only then does a single line shrink, so automatic tiles keep the font size.
fn tile_text_px(
    font_px: f32,
    tile_h: f32,
    shorter: bool,
    scale: f32,
    inline: bool,
    m: &dyn TextMeasure,
) -> f32 {
    if !inline {
        return stacked_text_px(font_px, tile_h, scale);
    }
    let px = font_px * GRAPH_FONT_SCALE;
    let line_h = m.line_height(px);
    if shorter && line_h > tile_h {
        (px * tile_h / line_h).max(1.0)
    } else {
        px
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
    names: &ItemNames,
    font_px: f32,
    m: &dyn TextMeasure,
) -> f32 {
    let text = cell_label(kind, part, labels, icons, names);
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
struct Labels<'a> {
    style: LabelStyle,
    icons: bool,
    vertical: bool,
    names: &'a ItemNames,
}

impl<'a> Labels<'a> {
    fn tile_text(&self, upload_on_top: bool) -> TileText<'a> {
        TileText {
            labels: self.style,
            icons: self.icons,
            vertical_labels: self.vertical,
            upload_on_top,
            names: self.names,
        }
    }

    fn width(&self, kind: ItemKind, part: CellPart, font_px: f32, m: &dyn TextMeasure) -> f32 {
        label_width(kind, part, self.style, self.icons, self.names, font_px, m)
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

/// The two directions a pair part stands for, in the order they are shown;
/// `None` for a part that shows one reading.
pub fn network_pair(kind: ItemKind, part: CellPart, upload_on_top: bool) -> Option<[CellPart; 2]> {
    match (kind, part) {
        (ItemKind::Network, CellPart::Main) => Some(network_parts(upload_on_top)),
        (ItemKind::Network, CellPart::AdapterPair(i)) => {
            let (down, up) = (CellPart::AdapterDown(i), CellPart::AdapterUp(i));
            Some(if upload_on_top {
                [up, down]
            } else {
                [down, up]
            })
        }
        _ => None,
    }
}

pub fn single_parts(kind: ItemKind, part: CellPart, upload_on_top: bool) -> Vec<CellPart> {
    match network_pair(kind, part, upload_on_top) {
        Some(pair) => pair.to_vec(),
        None => vec![part],
    }
}

fn with_pairs_split(items: &[ItemSpec], upload_on_top: bool) -> Vec<ItemSpec> {
    items
        .iter()
        .map(|spec| ItemSpec {
            kind: spec.kind,
            parts: spec
                .parts
                .iter()
                .flat_map(|&p| single_parts(spec.kind, p, upload_on_top))
                .collect(),
            tile: spec.tile,
            graphic: spec.graphic,
        })
        .collect()
}

/// Whether `kind` has a cell per chosen GPU.
fn is_gpu_item(kind: ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Gpu | ItemKind::GpuTemp | ItemKind::GpuMemory
    )
}

/// The enabled items in order, each with the cells it draws.
pub fn item_specs(config: &Config, gpus: &GpuSlots) -> Vec<ItemSpec> {
    let chosen = config.gpu_indexes(gpus);
    let mut specs = specs_with(config, &chosen);
    // An empty table knows nothing yet, so the combined reading still stands in.
    if chosen.is_empty() && !config.gpu.adapters.is_empty() && !gpus.is_empty() {
        specs.retain(|s| !is_gpu_item(s.kind));
    }
    specs
}

/// `item_specs` with `gpus`, the GPUs with cells of their own, already known.
fn specs_with(config: &Config, gpus: &[u8]) -> Vec<ItemSpec> {
    let kinds = config.enabled_items();
    let is_gpu = |k: &ItemKind| is_gpu_item(*k);
    // With several on, each GPU's readings sit side by side, where the first
    // of those items is.
    let paired = !gpus.is_empty() && kinds.iter().filter(|k| is_gpu(k)).count() >= 2;
    let tile = |kind| {
        config
            .item(kind)
            .map_or_else(TileSize::default, |i| TileSize {
                width: i.tile_width,
                height: i.tile_height,
            })
    };
    let threads = std::thread::available_parallelism().map_or(8, |n| n.get());
    let cores = u16::try_from(threads).unwrap_or(u16::MAX);
    let hybrid = crate::hardware::cpu::cached_topology()
        .performance_threads(threads)
        .is_some();
    let graphic = |kind| {
        config
            .item(kind)
            .filter(|i| i.graph)
            .map_or_else(TileGraphic::default, |i| {
                TileGraphic::of(i.style, cores, hybrid)
            })
    };
    let mut specs = Vec::with_capacity(kinds.len());
    for &kind in &kinds {
        if !(paired && is_gpu(&kind)) {
            specs.push(ItemSpec {
                kind,
                parts: item_parts(kind, config, gpus),
                tile: tile(kind),
                graphic: graphic(kind),
            });
        } else if !specs.iter().any(|s| is_gpu(&s.kind)) {
            let order: Vec<ItemKind> = kinds.iter().copied().filter(is_gpu).collect();
            for &i in gpus {
                specs.extend(order.iter().map(|&kind| ItemSpec {
                    kind,
                    parts: vec![CellPart::Gpu(i)],
                    tile: tile(kind),
                    graphic: graphic(kind),
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
    match kind {
        ItemKind::Disk if !drives.is_empty() => drives
            .iter()
            .map(|&d| match config.disk.value {
                DriveValue::Activity => CellPart::DriveActive(d),
                DriveValue::Used => CellPart::DriveUsed(d),
            })
            .collect(),
        kind if is_gpu_item(kind) && !gpus.is_empty() => {
            gpus.iter().map(|&i| CellPart::Gpu(i)).collect()
        }
        ItemKind::Network => network_item_parts(config),
        _ => vec![CellPart::Main],
    }
}

fn network_item_parts(config: &Config) -> Vec<CellPart> {
    let upload_on_top = config.display.upload_on_top;
    let split = config.network.split_tiles && config.display.mode == DisplayMode::Graph;
    let parts = |pair, down, up| match config.network.show {
        NetworkShow::Both if split => single_parts(ItemKind::Network, pair, upload_on_top),
        NetworkShow::Both => vec![pair],
        NetworkShow::Download => vec![down],
        NetworkShow::Upload => vec![up],
    };
    let count = config.network.adapters.len();
    // One chosen adapter takes the place of the combined reading.
    if count > 1 {
        (0..=u8::MAX)
            .take(count)
            .flat_map(|i| {
                parts(
                    CellPart::AdapterPair(i),
                    CellPart::AdapterDown(i),
                    CellPart::AdapterUp(i),
                )
            })
            .collect()
    } else {
        parts(CellPart::Main, CellPart::Down, CellPart::Up)
    }
}

pub fn compute_layout(items: &[ItemSpec], input: LayoutInput<'_>, m: &dyn TextMeasure) -> Layout {
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
    // A stacked label takes the full height, so each reading gets a column.
    let rows = if input.vertical_labels {
        TextRows {
            count: 1,
            gap: 0.0,
            ..rows
        }
    } else {
        rows
    };
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
        vertical: input.vertical_labels,
        names: input.names,
    };
    if let Some(bar_w) = input.vertical_bar_px {
        let items = with_pairs_split(items, input.upload_on_top);
        return match input.mode {
            DisplayMode::Text => {
                stacked_text(&items, bar_w, scale, font_px, labels, input.text_spacing, m)
            }
            DisplayMode::Graph => stacked_tiles(&items, bar_w, scale, font_px, labels, m),
        };
    }
    match input.mode {
        DisplayMode::Text => {
            let columns = columns(items, rows.count, input.upload_on_top);
            text_layout(columns, rows, scale, font_px, labels, input.text_spacing, m)
        }
        DisplayMode::Graph => graph_layout(items, height, scale, font_px, labels, input, m),
    }
}

/// The height at which the font fits uncut. Text keeps the rows the taskbar
/// gives the default font, rather than reflowing to one long row.
fn fitted_height(input: LayoutInput<'_>, scale: f32, font_px: f32, m: &dyn TextMeasure) -> f32 {
    let padding = PADDING_DIP * scale;
    let content = match input.mode {
        DisplayMode::Text => {
            let reference = m.line_height(REFERENCE_FONT_PT * input.dpi as f32 / 72.0);
            let rows = ((input.taskbar_height_px - 2.0 * padding) / reference).floor();
            rows.clamp(1.0, MAX_TEXT_ROWS as f32) * m.line_height(font_px)
        }
        DisplayMode::Graph if input.graph_inline && !input.vertical_labels => {
            m.line_height(font_px * GRAPH_FONT_SCALE)
        }
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
/// are kept together when they fit in a column; with several adapters, each
/// adapter's pair is, and so is each GPU's load and temperature.
fn columns(items: &[ItemSpec], rows: usize, upload_on_top: bool) -> Vec<Vec<(ItemKind, CellPart)>> {
    let mut groups: Vec<Vec<(ItemKind, CellPart)>> = Vec::new();
    for spec in items {
        let paired = spec
            .parts
            .iter()
            .any(|&p| network_pair(spec.kind, p, upload_on_top).is_some());
        let together = if paired { 1 } else { spec.parts.len() };
        for parts in spec.parts.chunks(together.max(1)) {
            let group: Vec<_> = parts
                .iter()
                .flat_map(|&p| single_parts(spec.kind, p, upload_on_top))
                .map(|p| (spec.kind, p))
                .collect();
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
    columns: Vec<Vec<(ItemKind, CellPart)>>,
    rows: TextRows,
    scale: f32,
    font_px: f32,
    labels: Labels<'_>,
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
    for column in columns {
        let label_w = column
            .iter()
            .map(|&(k, p)| {
                if labels.vertical {
                    let text = labels.tile_text(false);
                    let h = height - padding;
                    stacked_label(k, p, text, font_px, h, scale, m).width
                } else {
                    labels.width(k, p, font_px, m)
                }
            })
            .fold(0.0, f32::max);
        let value_w = column
            .iter()
            .map(|&(k, p)| m.width(widest_value(part_metric(k, p).unit()), font_px))
            .fold(0.0, f32::max);
        let col_w = label_w + label_gap + value_w;
        for (row, &(kind, part)) in column.iter().enumerate() {
            let y = top + row as f32 * (line_h + gap);
            let (rect_y, rect_h) = if labels.vertical {
                (padding / 2.0, height - padding)
            } else {
                (y, line_h)
            };
            cells.push(Cell {
                kind,
                part,
                rect: Rect {
                    x,
                    y: rect_y,
                    w: col_w,
                    h: rect_h,
                },
                value_x: x + label_w + label_gap,
                value_y: y,
                text_px: font_px,
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
    labels: Labels<'_>,
    spacing: TextSpacing,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = STACKED_PADDING_DIP * scale;
    let available = (bar_w - 2.0 * padding).max(1.0);
    let gap = spacing.label_gap * scale;
    let row_gap = spacing.row_gap * scale;
    let item_gap = spacing.item_gap * 3.0 / 5.0 * scale;
    let stack = |k, p, px: f32| {
        let text = labels.tile_text(false);
        stacked_label(k, p, text, px, f32::MAX, scale, m)
    };
    let widest_label = |px: f32| {
        items
            .iter()
            .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
            .map(|(k, p)| {
                if labels.vertical {
                    stack(k, p, px).width
                } else {
                    labels.width(k, p, px, m)
                }
            })
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
    let beside = !labelled || labels.vertical || label_w + gap + value_w <= available;
    let needed = if !labelled {
        value_w
    } else if labels.vertical {
        label_w + gap + value_w
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
            } else if labels.vertical {
                let label = stack(spec.kind, part, font_px);
                let h = (label.lines() as f32 * label.step).max(line_h);
                (padding + label_w + gap, y + (h - line_h) / 2.0, h)
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
                text_px: font_px,
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

/// A full-width graph tile per single-reading part for a vertical taskbar,
/// where both network directions will not fit on one line.
fn stacked_tiles(
    items: &[ItemSpec],
    bar_w: f32,
    scale: f32,
    font_px: f32,
    labels: Labels<'_>,
    m: &dyn TextMeasure,
) -> Layout {
    let padding = STACKED_PADDING_DIP * scale;
    let tile_w = (bar_w - 2.0 * padding).max(1.0);
    let available = (tile_w - (PADDING_DIP + 1.0) * scale).max(1.0);
    let small_px = font_px * GRAPH_FONT_SCALE;
    let look = TileLook {
        text: labels.tile_text(false),
        inline: false,
        vertical_bar: true,
        scale,
    };
    let fit = items
        .iter()
        .flat_map(|s| s.parts.iter().map(move |&p| (s, p)))
        .map(|(s, p)| {
            let value = m.width(widest_value(part_metric(s.kind, p).unit()), small_px);
            let text = if labels.vertical {
                value
            } else {
                labels.width(s.kind, p, small_px, m).max(value)
            };
            let tile_h = requested_height(s, scale).unwrap_or(MIN_STACKED_TILE_DIP * scale);
            let chrome = tile_chrome(s.kind, p, s.graphic, look, tile_h, small_px, m);
            (available - chrome).max(1.0) / text.max(1.0)
        })
        .fold(1.0, f32::min);
    let small_px = small_px * fit;
    let font_px = small_px / GRAPH_FONT_SCALE;
    let auto_h =
        (MIN_STACKED_TILE_DIP * scale).max(small_px * STACKED_LINES + STACKED_INSET_DIP * scale);

    let mut cells = Vec::new();
    let mut y = padding;
    for spec in items {
        let tile_h =
            requested_height(spec, scale).map_or(auto_h, |h| h.min(MAX_TILE_HEIGHT_DIP * scale));
        let text_px = stacked_text_px(font_px, tile_h, scale);
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
                text_px,
            });
            y += tile_h + STACKED_TILE_GAP_DIP * scale;
        }
    }
    Layout {
        width: bar_w,
        height: y - STACKED_TILE_GAP_DIP * scale + padding,
        font_px,
        cells,
        vertical: true,
    }
}

/// Widest text a graph tile draws: a pair shows `↓ value` / `↑ value`, other
/// tiles a label line and a value line.
fn graph_text_width(
    kind: ItemKind,
    part: CellPart,
    text: TileText<'_>,
    font_px: f32,
    m: &dyn TextMeasure,
) -> f32 {
    let widest = widest_value(part_metric(kind, part).unit());
    let labels = Labels {
        style: text.labels,
        icons: text.icons,
        vertical: text.vertical_labels,
        names: text.names,
    };
    let label = |part| labels.width(kind, part, font_px, m);
    match network_pair(kind, part, false) {
        Some([first, second]) => {
            let label_w = if labels.names.renames_directions() {
                label(first).max(label(second))
            } else {
                label(first)
            };
            label_w + icon_gap(font_px) + m.width(widest, font_px)
        }
        None if labels.vertical => m.width(widest, font_px),
        None => label(part).max(m.width(widest, font_px)),
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
pub struct TileText<'a> {
    pub labels: LabelStyle,
    pub icons: bool,
    pub vertical_labels: bool,
    pub upload_on_top: bool,
    pub names: &'a ItemNames,
}

/// Where an inline graph tile draws each of its parts, and the width of the
/// whole line. `font_px` is the size the text is drawn at.
pub fn inline_slots(
    kind: ItemKind,
    part: CellPart,
    text: TileText<'_>,
    font_px: f32,
    scale: f32,
    m: &dyn TextMeasure,
) -> (Vec<InlineSlot>, f32) {
    let TileText {
        labels,
        icons,
        names,
        ..
    } = text;
    let parts = single_parts(kind, part, text.upload_on_top);
    let gap = INLINE_GAP_DIP * scale;
    let mut slots = Vec::with_capacity(parts.len());
    let mut x = 0.0;
    for part in parts {
        if !slots.is_empty() {
            x += 2.0 * gap;
        }
        let value_x = x + label_width(kind, part, labels, icons, names, font_px, m) + gap;
        slots.push(InlineSlot {
            part,
            label_x: x,
            value_x,
        });
        x = value_x + m.width(widest_value(part_metric(kind, part).unit()), font_px);
    }
    (slots, x)
}

fn requested_height(spec: &ItemSpec, scale: f32) -> Option<f32> {
    spec.tile
        .height
        .map(|h| (f32::from(h) * scale).round().max(1.0))
}

fn graph_layout(
    items: &[ItemSpec],
    height: f32,
    scale: f32,
    font_px: f32,
    labels: Labels<'_>,
    input: LayoutInput<'_>,
    m: &dyn TextMeasure,
) -> Layout {
    let inline = input.graph_inline && !input.vertical_labels;
    let look = TileLook {
        text: labels.tile_text(input.upload_on_top),
        inline,
        vertical_bar: false,
        scale,
    };
    let padding = PADDING_DIP * scale;
    let auto_h = (height - 2.0 * padding).max(1.0);
    let max_h = if input.fit_font {
        auto_h.max(MAX_TILE_HEIGHT_DIP * scale)
    } else {
        auto_h
    };
    let tile_height =
        |spec: &ItemSpec| requested_height(spec, scale).map_or(auto_h, |h| h.min(max_h));
    let tallest = items.iter().map(tile_height).fold(auto_h, f32::max);
    let height = if tallest > auto_h {
        (tallest + 2.0 * padding).ceil()
    } else {
        height
    };
    let row_h = (height - 2.0 * padding).max(1.0);
    let mut cells = Vec::with_capacity(items.len());
    let mut x = padding;
    for spec in items {
        let tile_h = tile_height(spec);
        let y = if tile_h < row_h {
            (padding + (row_h - tile_h) / 2.0).round()
        } else {
            padding
        };
        let text_px = tile_text_px(font_px, tile_h, tile_h < auto_h, scale, inline, m);
        // A short tile stops widening once its text stops growing.
        let stacked_px = font_px.min(stacked_fit_px(tile_h, scale) / GRAPH_FONT_SCALE);
        let min_w = spec.tile.width.map_or(MIN_TILE_DIP, f32::from) * scale;
        for &part in &spec.parts {
            let text_w = if inline {
                inline_slots(spec.kind, part, look.text, text_px, scale, m).1
            } else {
                graph_text_width(spec.kind, part, look.text, stacked_px, m)
            };
            let chrome = tile_chrome(spec.kind, part, spec.graphic, look, tile_h, text_px, m);
            let w = (text_w + chrome + 2.0 * padding).max(min_w);
            cells.push(Cell {
                kind: spec.kind,
                part,
                rect: Rect { x, y, w, h: tile_h },
                value_x: x + padding,
                value_y: y,
                text_px,
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
    use std::sync::LazyLock;

    use crate::config::{Config, DriveValue, LabelStyle, NetworkShow};

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

    fn names() -> &'static ItemNames {
        static NAMES: LazyLock<ItemNames> =
            LazyLock::new(|| ItemNames::resolve(&Config::default()));
        &NAMES
    }

    fn input(mode: DisplayMode, taskbar_height_px: f32, dpi: u32) -> LayoutInput<'static> {
        LayoutInput {
            mode,
            taskbar_height_px,
            fit_font: false,
            dpi,
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

    fn vertical(mode: DisplayMode, bar_w: f32) -> LayoutInput<'static> {
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
    fn short_vertical_tiles_size_text_for_two_lines_with_inline_on() {
        let mut c = Config::default();
        c.display.mode = DisplayMode::Graph;
        c.item_mut(ItemKind::Cpu).unwrap().tile_height = Some(16);
        let specs = item_specs(&c, &GpuSlots::new());
        let stacked = vertical(DisplayMode::Graph, 120.0);
        let inline = LayoutInput {
            graph_inline: true,
            ..stacked
        };
        let l = compute_layout(&specs, inline, &Fixed);
        assert_eq!(l.cells, compute_layout(&specs, stacked, &Fixed).cells);
        let cell = l.cells.iter().find(|c| c.kind == ItemKind::Cpu).unwrap();
        assert!(cell.text_px * STACKED_LINES <= cell.rect.h);
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
                parts: vec![CellPart::Main],
                tile: TileSize::default(),
                graphic: TileGraphic::default(),
            })
            .collect()
    }

    const ALL: [ItemKind; 15] = ItemKind::ALL;

    fn sized_specs(sizes: &[(ItemKind, Option<u16>, Option<u16>)]) -> Vec<ItemSpec> {
        sizes
            .iter()
            .map(|&(kind, width, height)| ItemSpec {
                kind,
                parts: vec![CellPart::Main],
                tile: TileSize { width, height },
                graphic: TileGraphic::default(),
            })
            .collect()
    }

    fn graph_input(fit_font: bool) -> LayoutInput<'static> {
        LayoutInput {
            fit_font,
            ..input(DisplayMode::Graph, 48.0, 96)
        }
    }

    fn graphic_spec(kind: ItemKind, graphic: TileGraphic) -> ItemSpec {
        ItemSpec {
            kind,
            parts: vec![CellPart::Main],
            tile: TileSize::default(),
            graphic,
        }
    }

    fn look(i: LayoutInput<'_>) -> TileLook<'_> {
        TileLook {
            text: TileText {
                labels: i.labels,
                icons: i.icons,
                vertical_labels: i.vertical_labels,
                upload_on_top: i.upload_on_top,
                names: i.names,
            },
            inline: i.graph_inline && !i.vertical_labels,
            vertical_bar: i.vertical_bar_px.is_some(),
            scale: i.dpi as f32 / 96.0,
        }
    }

    fn regions(l: &Layout, graphic: TileGraphic, i: LayoutInput<'_>) -> TileRegions {
        let c = &l.cells[0];
        tile_regions(c.rect, c.kind, c.part, graphic, look(i), c.text_px, &Fixed)
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - 0.51
            && inner.y >= outer.y - 0.51
            && inner.right() <= outer.right() + 0.51
            && inner.y + inner.h <= outer.y + outer.h + 0.51
    }

    #[test]
    fn a_vertical_label_sits_left_of_the_value_and_the_graph() {
        let items = specs(&[ItemKind::Cpu, ItemKind::Network]);
        let i = LayoutInput {
            vertical_labels: true,
            ..input(DisplayMode::Graph, 48.0, 96)
        };
        let l = compute_layout(&items, i, &Fixed);
        let r = regions(&l, TileGraphic::Behind, i);
        let (stack, area) = r.stack.clone().unwrap();
        assert_eq!(stack.glyphs, ["C", "P", "U"]);
        assert!(area.right() <= r.text_x);
        assert!(r.graph.x >= area.right() - 1.0);
        assert!(stack.step * 3.0 <= area.h);
        assert_eq!(r.graph.right(), l.cells[0].rect.right() - 1.0);
    }

    fn vertical_text(icons: bool) -> TileText<'static> {
        TileText {
            labels: LabelStyle::Full,
            icons,
            vertical_labels: true,
            upload_on_top: false,
            names: names(),
        }
    }

    #[test]
    fn a_long_vertical_label_shrinks_then_stops_inside_the_tile() {
        let text = vertical_text(false);
        let stack = |h| stacked_label(ItemKind::Ping, CellPart::Main, text, 12.0, h, 1.0, &Fixed);
        assert_eq!(stack(100.0).px, 12.0);
        assert_eq!(stack(36.0).glyphs.len(), 4);
        assert_eq!(stack(36.0).px, 9.0);
        let short = stack(14.0);
        assert!(short.px == 7.0 && short.glyphs.len() == 2);
        assert!(short.lines() as f32 * short.step <= 14.01);
    }

    #[test]
    fn a_drive_stacks_its_letter_without_the_colon() {
        let part = CellPart::DriveActive(b'C');
        let stack = stacked_label(
            ItemKind::Disk,
            part,
            vertical_text(true),
            12.0,
            36.0,
            1.0,
            &Fixed,
        );
        assert_eq!(stack.glyphs, ["C"]);
        assert!(stack.icon.is_some());
        assert_eq!(stack.lines(), 2);
    }

    #[test]
    fn text_with_vertical_labels_gives_each_reading_a_full_height_column() {
        let items = specs(&[ItemKind::Cpu, ItemKind::Ram, ItemKind::Gpu]);
        let i = LayoutInput {
            vertical_labels: true,
            ..input(DisplayMode::Text, 48.0, 96)
        };
        let l = compute_layout(&items, i, &Fixed);
        assert_eq!(l.cells.len(), 3);
        for pair in l.cells.windows(2) {
            assert!(pair[0].rect.right() <= pair[1].rect.x);
        }
        for c in &l.cells {
            assert_eq!(c.rect.h, l.height - 4.0);
            assert_eq!(c.value_y, (l.height - Fixed.line_height(12.0)) / 2.0);
            assert!(c.value_x > c.rect.x);
        }
    }

    #[test]
    fn a_ring_takes_room_beside_the_text() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let plain = compute_layout(&specs(&[ItemKind::Cpu]), i, &Fixed);
        let items = [graphic_spec(ItemKind::Cpu, TileGraphic::Ring)];
        let l = compute_layout(&items, i, &Fixed);
        assert!(l.cells[0].rect.w > plain.cells[0].rect.w);
        let r = regions(&l, TileGraphic::Ring, i);
        let ring = r.ring.unwrap();
        assert_eq!(ring.w, ring.h);
        assert!(inside(ring, l.cells[0].rect));
        assert!(r.text_x >= ring.right());
    }

    #[test]
    fn core_bars_sit_after_the_text_and_grow_with_the_core_count() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let w = |n| {
            let items = [graphic_spec(ItemKind::Cpu, TileGraphic::Cores(n))];
            compute_layout(&items, i, &Fixed).cells[0].rect.w
        };
        assert!(w(4) == w(8), "both use the minimum");
        assert!(w(24) > w(8));
        assert!(w(32) > w(24));
        assert_eq!(w(4096), w(256), "capped");
        let graphic = TileGraphic::Cores(24);
        let items = [graphic_spec(ItemKind::Cpu, graphic)];
        let l = compute_layout(&items, i, &Fixed);
        let r = regions(&l, graphic, i);
        let cores = r.cores.unwrap();
        assert!(inside(cores, l.cells[0].rect));
        assert!(cores.x >= r.text_x);
    }

    #[test]
    fn heat_cells_widen_with_the_core_count_and_stop_at_a_cap() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let w = |n| {
            let items = [graphic_spec(ItemKind::Cpu, TileGraphic::Heat(n))];
            compute_layout(&items, i, &Fixed).cells[0].rect.w
        };
        assert!(w(24) > w(8));
        assert_eq!(w(4096), w(512), "capped");
        let graphic = TileGraphic::Heat(24);
        let items = [graphic_spec(ItemKind::Cpu, graphic)];
        let l = compute_layout(&items, i, &Fixed);
        let r = regions(&l, graphic, i);
        let cores = r.cores.unwrap();
        assert!(cores.x >= r.text_x && inside(cores, l.cells[0].rect));
        assert_eq!(heat_rows(cores.h, 1.0), 3);
    }

    #[test]
    fn a_strip_goes_under_a_single_reading_and_beside_a_pair() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        for graphic in [TileGraphic::Segments, TileGraphic::Spark] {
            let items = [graphic_spec(ItemKind::Cpu, graphic)];
            let l = compute_layout(&items, i, &Fixed);
            let r = regions(&l, graphic, i);
            let strip = r.strip.unwrap();
            assert!(r.strip_below);
            assert!(inside(strip, l.cells[0].rect));
            assert!(strip.y >= l.cells[0].rect.y + Fixed.line_height(l.cells[0].text_px));

            let items = [graphic_spec(ItemKind::Network, graphic)];
            let l = compute_layout(&items, i, &Fixed);
            let r = regions(&l, graphic, i);
            assert!(!r.strip_below);
            assert!(r.strip.unwrap().x > r.text_x);
            assert!(inside(r.strip.unwrap(), l.cells[0].rect));
        }
    }

    #[test]
    fn a_strip_tile_has_room_for_its_label_and_value_on_one_line() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let graphic = TileGraphic::Segments;
        let items = [graphic_spec(ItemKind::Cpu, graphic)];
        let l = compute_layout(&items, i, &Fixed);
        let px = l.cells[0].text_px;
        let label = Fixed.width("CPU", px);
        let value = Fixed.width(widest_value(MetricKey::CpuTotal.unit()), px);
        assert!(l.cells[0].rect.w >= label + value + 2.0 * PADDING_DIP);
    }

    #[test]
    fn a_short_tile_or_single_line_moves_the_strip_beside_the_text() {
        let graphic = TileGraphic::Spark;
        let items = [graphic_spec(ItemKind::Cpu, graphic)];
        let inline = LayoutInput {
            graph_inline: true,
            ..input(DisplayMode::Graph, 48.0, 96)
        };
        let l = compute_layout(&items, inline, &Fixed);
        let r = regions(&l, graphic, inline);
        assert!(!r.strip_below);
        assert!(r.strip.unwrap().x > r.text_x);
        let mut spec = graphic_spec(ItemKind::Cpu, graphic);
        spec.tile.height = Some(14);
        let plain = input(DisplayMode::Graph, 48.0, 96);
        let l = compute_layout(&[spec], plain, &Fixed);
        assert!(!regions(&l, graphic, plain).strip_below);
    }

    #[test]
    fn on_a_vertical_bar_a_short_strip_takes_no_room_from_the_text() {
        let i = vertical(DisplayMode::Graph, 48.0);
        let font = |graphic| {
            let mut spec = graphic_spec(ItemKind::Cpu, graphic);
            spec.tile.height = Some(14);
            compute_layout(&[spec], i, &Fixed).font_px
        };
        assert_eq!(font(TileGraphic::Spark), font(TileGraphic::Behind));
        assert_eq!(font(TileGraphic::Segments), font(TileGraphic::Behind));
    }

    #[test]
    fn a_tall_ring_on_a_vertical_bar_stays_inside_its_tile() {
        let i = vertical(DisplayMode::Graph, 48.0);
        let mut spec = graphic_spec(ItemKind::Cpu, TileGraphic::Ring);
        spec.tile.height = Some(100);
        let l = compute_layout(&[spec], i, &Fixed);
        let ring = regions(&l, TileGraphic::Ring, i).ring.unwrap();
        assert!(inside(ring, l.cells[0].rect), "{ring:?}");
        assert_eq!(ring.w, ring.h);
    }

    #[test]
    fn a_requested_width_stretches_the_core_bars() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let graphic = TileGraphic::Cores(4);
        let mut spec = graphic_spec(ItemKind::Cpu, graphic);
        let natural = compute_layout(std::slice::from_ref(&spec), i, &Fixed);
        let before = regions(&natural, graphic, i).cores.unwrap();
        spec.tile.width = Some(200);
        let wide = compute_layout(&[spec], i, &Fixed);
        assert_eq!(wide.cells[0].rect.w, 200.0);
        assert!(regions(&wide, graphic, i).cores.unwrap().w > before.w);
    }

    #[test]
    fn every_graphic_and_label_combination_stays_inside_its_tile() {
        let graphics = [
            TileGraphic::Behind,
            TileGraphic::Ring,
            TileGraphic::Cores(24),
            TileGraphic::Heat(24),
            TileGraphic::Segments,
            TileGraphic::Spark,
            TileGraphic::Bubble,
        ];
        for graphic in graphics {
            for vertical_labels in [false, true] {
                for graph_inline in [false, true] {
                    for icons in [false, true] {
                        for (dpi, bar) in [(96, 48.0), (96, 32.0), (144, 72.0), (192, 96.0)] {
                            for vertical_bar_px in [None, Some(bar * 1.5)] {
                                let i = LayoutInput {
                                    vertical_labels,
                                    graph_inline,
                                    icons,
                                    vertical_bar_px,
                                    ..input(DisplayMode::Graph, bar, dpi)
                                };
                                let items = [graphic_spec(ItemKind::Cpu, graphic)];
                                let l = compute_layout(&items, i, &Fixed);
                                let r = regions(&l, graphic, i);
                                let rect = l.cells[0].rect;
                                let context = format!("{graphic:?} {i:?}");
                                for part in [r.ring, r.cores, r.strip, r.stack.map(|(_, a)| a)]
                                    .into_iter()
                                    .flatten()
                                {
                                    assert!(inside(part, rect), "{part:?} {rect:?} {context}");
                                }
                                assert!(r.text_x < rect.right(), "{context}");
                            }
                        }
                    }
                }
            }
        }
    }

    fn cell_of(l: &Layout, kind: ItemKind) -> &Cell {
        l.cells.iter().find(|c| c.kind == kind).unwrap()
    }

    /// The text a tile draws fits the tile it was measured for.
    fn assert_text_fits(l: &Layout, i: LayoutInput) {
        let scale = i.dpi as f32 / 96.0;
        let padding = PADDING_DIP * scale;
        let text = TileText {
            labels: i.labels,
            icons: i.icons,
            vertical_labels: i.vertical_labels,
            upload_on_top: i.upload_on_top,
            names: i.names,
        };
        for c in &l.cells {
            let (w, h) = if i.graph_inline {
                let (_, line) = inline_slots(c.kind, c.part, text, c.text_px, scale, &Fixed);
                (line, Fixed.line_height(c.text_px))
            } else {
                (
                    graph_text_width(c.kind, c.part, text, c.text_px, &Fixed),
                    c.text_px * STACKED_LINES + STACKED_INSET_DIP * scale,
                )
            };
            if i.vertical_bar_px.is_none() {
                assert!(w + 2.0 * padding <= c.rect.w + 0.01, "{c:?}");
            }
            assert!(h <= c.rect.h + 0.01, "{c:?}");
        }
    }

    #[test]
    fn a_width_request_replaces_the_minimum_but_not_the_text() {
        let cpu = |width| {
            let items = sized_specs(&[(ItemKind::Cpu, width, None)]);
            cell_of(
                &compute_layout(&items, graph_input(false), &Fixed),
                ItemKind::Cpu,
            )
            .rect
            .w
        };
        let labels = TileText {
            labels: LabelStyle::Full,
            icons: false,
            vertical_labels: false,
            upload_on_top: false,
            names: names(),
        };
        let text = graph_text_width(ItemKind::Cpu, CellPart::Main, labels, 12.0, &Fixed) + 8.0;
        assert!(text < 44.0, "{text}");
        assert_eq!(cpu(None), 44.0);
        assert_eq!(cpu(Some(40)), 40.0, "below the automatic minimum");
        assert_eq!(cpu(Some(16)), text, "never below the text");
        assert_eq!(cpu(Some(90)), 90.0);
        let mut i = graph_input(false);
        i.dpi = 192;
        let items = sized_specs(&[(ItemKind::Cpu, Some(90), None)]);
        assert_eq!(compute_layout(&items, i, &Fixed).cells[0].rect.w, 180.0);
    }

    #[test]
    fn a_wider_tile_moves_the_ones_after_it() {
        let auto = compute_layout(
            &sized_specs(&[(ItemKind::Cpu, None, None), (ItemKind::Ram, None, None)]),
            graph_input(false),
            &Fixed,
        );
        let wide = compute_layout(
            &sized_specs(&[
                (ItemKind::Cpu, Some(100), None),
                (ItemKind::Ram, None, None),
            ]),
            graph_input(false),
            &Fixed,
        );
        let grown = 100.0 - cell_of(&auto, ItemKind::Cpu).rect.w;
        assert_eq!(wide.width, auto.width + grown);
        assert_eq!(
            cell_of(&wide, ItemKind::Ram).rect.x,
            cell_of(&auto, ItemKind::Ram).rect.x + grown
        );
        assert_no_overlap(&wide);
    }

    #[test]
    fn a_docked_tile_stays_in_the_bar_and_centres() {
        let auto = compute_layout(
            &sized_specs(&[(ItemKind::Cpu, None, None), (ItemKind::Ram, None, None)]),
            graph_input(false),
            &Fixed,
        );
        let tall = compute_layout(
            &sized_specs(&[
                (ItemKind::Cpu, None, Some(200)),
                (ItemKind::Ram, None, None),
            ]),
            graph_input(false),
            &Fixed,
        );
        assert_eq!(tall, auto, "taller than the bar is the same as automatic");

        let l = compute_layout(
            &sized_specs(&[(ItemKind::Cpu, None, Some(20)), (ItemKind::Ram, None, None)]),
            graph_input(false),
            &Fixed,
        );
        assert_eq!(l.height, 48.0);
        let (cpu, ram) = (cell_of(&l, ItemKind::Cpu), cell_of(&l, ItemKind::Ram));
        assert_eq!((cpu.rect.y, cpu.rect.h), (14.0, 20.0));
        assert_eq!(cpu.value_y, 14.0);
        assert_eq!((ram.rect.y, ram.rect.h), (4.0, 40.0));
    }

    #[test]
    fn a_floating_tile_grows_the_window_and_leaves_the_others_alone() {
        let auto = compute_layout(
            &sized_specs(&[(ItemKind::Cpu, None, None), (ItemKind::Ram, None, None)]),
            graph_input(true),
            &Fixed,
        );
        let l = compute_layout(
            &sized_specs(&[
                (ItemKind::Cpu, None, Some(100)),
                (ItemKind::Ram, None, None),
            ]),
            graph_input(true),
            &Fixed,
        );
        assert_eq!(l.height, 108.0);
        let (cpu, ram) = (cell_of(&l, ItemKind::Cpu), cell_of(&l, ItemKind::Ram));
        let auto_ram = cell_of(&auto, ItemKind::Ram);
        assert_eq!((cpu.rect.y, cpu.rect.h), (4.0, 100.0));
        assert_eq!((ram.rect.w, ram.rect.h), (auto_ram.rect.w, auto_ram.rect.h));
        assert_eq!(ram.text_px, auto_ram.text_px);
        assert_eq!(ram.rect.y, 34.0, "centred");
        assert_inside(&l);
    }

    #[test]
    fn a_floating_tile_stops_at_the_limit_at_any_scale() {
        for (dpi, tile, window) in [(96, 200.0, 208.0), (144, 300.0, 312.0)] {
            let mut i = graph_input(true);
            i.dpi = dpi;
            let items = sized_specs(&[(ItemKind::Cpu, None, Some(5000))]);
            let l = compute_layout(&items, i, &Fixed);
            assert_eq!(l.cells[0].rect.h, tile);
            assert_eq!(l.height, window);
        }
    }

    #[test]
    fn a_floating_font_taller_than_a_request_keeps_the_window() {
        let mut i = graph_input(true);
        i.font_size_pt = 20.0;
        let auto = compute_layout(&specs(&[ItemKind::Cpu, ItemKind::Ram]), i, &Fixed);
        let items = sized_specs(&[(ItemKind::Cpu, None, Some(30)), (ItemKind::Ram, None, None)]);
        let l = compute_layout(&items, i, &Fixed);
        assert_eq!(l.height, auto.height);
        assert_eq!(cell_of(&l, ItemKind::Cpu).rect.h, 30.0);
        assert_text_fits(&l, i);
    }

    #[test]
    fn mixed_heights_fit_each_tile_in_either_order() {
        for fit_font in [false, true] {
            for pt in [9.0, 20.0] {
                let mut i = graph_input(fit_font);
                i.font_size_pt = pt;
                let short = (ItemKind::Cpu, None, Some(20));
                let auto = (ItemKind::Network, None, None);
                let tall = (ItemKind::Ram, None, Some(40));
                let one = compute_layout(&sized_specs(&[short, auto, tall]), i, &Fixed);
                let two = compute_layout(&sized_specs(&[tall, auto, short]), i, &Fixed);
                for l in [&one, &two] {
                    assert_text_fits(l, i);
                    assert_no_overlap(l);
                    for c in &l.cells {
                        assert_eq!(c.text_px, stacked_text_px(l.font_px, c.rect.h, 1.0));
                    }
                    assert!(
                        cell_of(l, ItemKind::Cpu).text_px < cell_of(l, ItemKind::Ram).text_px,
                        "{pt}"
                    );
                }
                for kind in [ItemKind::Cpu, ItemKind::Network, ItemKind::Ram] {
                    let (a, b) = (cell_of(&one, kind), cell_of(&two, kind));
                    assert_eq!(
                        (a.rect.w, a.rect.h, a.rect.y),
                        (b.rect.w, b.rect.h, b.rect.y)
                    );
                    assert_eq!(a.text_px, b.text_px);
                }
                assert_eq!(one.width, two.width);
            }
        }
    }

    #[test]
    fn a_short_tile_measures_narrower_than_a_tall_one() {
        let mut i = graph_input(false);
        i.font_size_pt = 12.0;
        let items = sized_specs(&[(ItemKind::Network, None, Some(20))]);
        let short = compute_layout(&items, i, &Fixed).cells[0].rect.w;
        let items = sized_specs(&[(ItemKind::Network, None, None)]);
        let auto = compute_layout(&items, i, &Fixed).cells[0].rect.w;
        assert!(short < auto, "{short} vs {auto}");
    }

    #[test]
    fn inline_text_shrinks_only_in_a_tile_too_short_for_its_line() {
        for fit_font in [false, true] {
            let mut i = graph_input(fit_font);
            i.graph_inline = true;
            let auto = compute_layout(&specs(&[ItemKind::Cpu, ItemKind::Network]), i, &Fixed);
            let px = 12.0 * GRAPH_FONT_SCALE;
            assert!(auto.cells.iter().all(|c| c.text_px == px));
            for sizes in [
                [
                    (ItemKind::Cpu, None, Some(16)),
                    (ItemKind::Network, None, Some(12)),
                ],
                [
                    (ItemKind::Network, None, Some(12)),
                    (ItemKind::Cpu, None, Some(16)),
                ],
            ] {
                let l = compute_layout(&sized_specs(&sizes), i, &Fixed);
                assert_eq!(
                    cell_of(&l, ItemKind::Cpu).text_px,
                    px,
                    "the line still fits"
                );
                let net = cell_of(&l, ItemKind::Network);
                assert!(net.text_px < px, "{}", net.text_px);
                assert!(net.rect.w < cell_of(&auto, ItemKind::Network).rect.w);
                assert_text_fits(&l, i);
            }
        }
    }

    #[test]
    fn a_size_applies_to_each_network_tile_of_any_arrangement() {
        for (adapters, split, show, cells) in [
            (0, false, NetworkShow::Both, 1),
            (0, true, NetworkShow::Both, 2),
            (0, false, NetworkShow::Upload, 1),
            (2, false, NetworkShow::Both, 2),
            (2, true, NetworkShow::Both, 4),
            (2, false, NetworkShow::Download, 2),
        ] {
            let mut c = Config::default();
            c.display.mode = DisplayMode::Graph;
            c.network.adapters = (0..adapters).map(|i| format!("a{i}")).collect();
            c.network.split_tiles = split;
            c.network.show = show;
            let net = c.item_mut(ItemKind::Network).unwrap();
            net.tile_width = Some(150);
            net.tile_height = Some(24);
            let specs = item_specs(&c, &GpuSlots::new());
            for fit_font in [false, true] {
                let i = graph_input(fit_font);
                let l = compute_layout(&specs, i, &Fixed);
                let tiles: Vec<_> = l
                    .cells
                    .iter()
                    .filter(|c| c.kind == ItemKind::Network)
                    .collect();
                assert_eq!(tiles.len(), cells);
                for tile in tiles {
                    assert_eq!((tile.rect.w, tile.rect.h), (150.0, 24.0), "{:?}", tile.part);
                }
                assert!(
                    l.cells
                        .iter()
                        .filter(|c| c.kind != ItemKind::Network)
                        .all(|c| c.rect.h != 24.0)
                );
                assert_text_fits(&l, i);
                assert_no_overlap(&l);
            }
        }
    }

    #[test]
    fn a_size_applies_to_each_gpu_and_drive_tile() {
        let mut c = gpu_config(&["A", "B"]);
        c.display.mode = DisplayMode::Graph;
        c.disk.drives = vec!["C".into(), "D".into()];
        c.item_mut(ItemKind::Gpu).unwrap().tile_width = Some(100);
        c.item_mut(ItemKind::GpuTemp).unwrap().tile_height = Some(20);
        let disk = c.item_mut(ItemKind::Disk).unwrap();
        disk.enabled = true;
        disk.tile_width = Some(120);
        let gpus = GpuSlots::listing(&[adapter("A", 1), adapter("B", 2)]);
        let l = compute_layout(&item_specs(&c, &gpus), graph_input(false), &Fixed);
        let of = |kind| l.cells.iter().filter(move |c| c.kind == kind);
        assert_eq!(of(ItemKind::Gpu).count(), 2);
        assert!(of(ItemKind::Gpu).all(|c| c.rect.w == 100.0 && c.rect.h == 40.0));
        assert_eq!(of(ItemKind::GpuTemp).count(), 2);
        assert!(of(ItemKind::GpuTemp).all(|c| c.rect.h == 20.0));
        assert_eq!(of(ItemKind::Disk).count(), 2);
        assert!(of(ItemKind::Disk).all(|c| c.rect.w == 120.0));
    }

    #[test]
    fn a_vertical_bar_ignores_width_and_honours_height() {
        for bar in [48.0, 120.0] {
            let i = vertical(DisplayMode::Graph, bar);
            let auto = compute_layout(&specs(&[ItemKind::Cpu, ItemKind::Network]), i, &Fixed);
            let wide = compute_layout(
                &sized_specs(&[
                    (ItemKind::Cpu, Some(300), None),
                    (ItemKind::Network, Some(300), None),
                ]),
                i,
                &Fixed,
            );
            assert_eq!(wide, auto);
            for sizes in [
                [
                    (ItemKind::Cpu, None, Some(20)),
                    (ItemKind::Network, None, Some(90)),
                ],
                [
                    (ItemKind::Network, None, Some(90)),
                    (ItemKind::Cpu, None, Some(20)),
                ],
            ] {
                let l = compute_layout(&sized_specs(&sizes), i, &Fixed);
                assert_eq!(l.width, bar);
                assert_eq!(cell_of(&l, ItemKind::Cpu).rect.h, 20.0);
                let net: Vec<_> = l
                    .cells
                    .iter()
                    .filter(|c| c.kind == ItemKind::Network)
                    .collect();
                assert_eq!(net.len(), 2, "split, each with the size");
                assert!(net.iter().all(|c| c.rect.h == 90.0));
                assert!(net[0].text_px > cell_of(&l, ItemKind::Cpu).text_px);
                assert!(l.cells.iter().all(|c| c.rect.w == auto.cells[0].rect.w));
                assert_no_overlap(&l);
                assert_inside(&l);
                assert_text_fits(&l, i);
            }
            let inline = LayoutInput {
                graph_inline: true,
                ..i
            };
            let auto = compute_layout(&specs(&[ItemKind::Cpu]), inline, &Fixed);
            assert_eq!(auto.cells[0].text_px, auto.font_px * GRAPH_FONT_SCALE);
        }
    }

    #[test]
    fn text_mode_ignores_tile_sizes() {
        let sizes = [
            (ItemKind::Cpu, Some(300), Some(10)),
            (ItemKind::Ram, Some(300), Some(200)),
        ];
        for i in [
            input(DisplayMode::Text, 48.0, 96),
            vertical(DisplayMode::Text, 48.0),
            LayoutInput {
                fit_font: true,
                ..input(DisplayMode::Text, 48.0, 96)
            },
        ] {
            assert_eq!(
                compute_layout(&sized_specs(&sizes), i, &Fixed),
                compute_layout(&specs(&[ItemKind::Cpu, ItemKind::Ram]), i, &Fixed)
            );
        }
    }

    #[test]
    fn hit_testing_follows_the_sized_tiles() {
        for fit_font in [false, true] {
            let items = sized_specs(&[
                (ItemKind::Cpu, Some(80), Some(20)),
                (ItemKind::Ram, None, None),
            ]);
            let l = compute_layout(&items, graph_input(fit_font), &Fixed);
            let cpu = cell_of(&l, ItemKind::Cpu).rect;
            let middle = cpu.x + cpu.w - 1.0;
            assert_eq!(l.hit_test(middle, cpu.y + 1.0), Some(ItemKind::Cpu));
            assert_eq!(l.hit_test(middle, cpu.y + cpu.h - 1.0), Some(ItemKind::Cpu));
            assert_eq!(
                l.hit_test(middle, cpu.y - 1.0),
                None,
                "above the short tile"
            );
            assert_eq!(l.hit_test(middle, cpu.y + cpu.h + 1.0), None, "below it");
            let ram = cell_of(&l, ItemKind::Ram).rect;
            assert_eq!(l.hit_test(ram.x + 1.0, ram.y + 1.0), Some(ItemKind::Ram));
        }
    }

    #[test]
    fn automatic_tiles_ask_for_nothing() {
        let c = Config::default();
        let specs = item_specs(&c, &GpuSlots::new());
        assert!(specs.iter().all(|s| s.tile == TileSize::default()));
    }

    #[test]
    fn a_tile_size_is_part_of_the_spec_key() {
        let mut c = Config::default();
        let before = item_specs(&c, &GpuSlots::new());
        c.items[0].tile_height = Some(30);
        assert_ne!(item_specs(&c, &GpuSlots::new()), before);
    }

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
            vertical_labels: false,
            upload_on_top: false,
            names: names(),
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
        let mut i = input(DisplayMode::Text, 48.0, 96);
        i.upload_on_top = true;
        let l = compute_layout(&specs(&[ItemKind::Network]), i, &Fixed);
        let parts: Vec<CellPart> = l.cells.iter().map(|c| c.part).collect();
        assert_eq!(parts, [CellPart::Up, CellPart::Down]);
    }

    fn network_cells(edit: impl Fn(&mut Config), i: LayoutInput) -> Vec<CellPart> {
        let mut c = Config::default();
        c.display.mode = i.mode;
        c.display.upload_on_top = i.upload_on_top;
        edit(&mut c);
        let l = compute_layout(&item_specs(&c, &GpuSlots::new()), i, &Fixed);
        assert_no_overlap(&l);
        assert_inside(&l);
        for cell in &l.cells {
            let (x, y) = (cell.rect.x + 0.5, cell.rect.y + 0.5);
            assert_eq!(l.hit_test(x, y), Some(cell.kind), "{cell:?}");
        }
        l.cells
            .iter()
            .filter(|c| c.kind == ItemKind::Network)
            .map(|c| c.part)
            .collect()
    }

    /// Text, graph, inline graph, vertical text and vertical graph.
    fn every_mode(upload_on_top: bool) -> [(&'static str, LayoutInput<'static>); 5] {
        let up = |i: LayoutInput<'static>| LayoutInput { upload_on_top, ..i };
        [
            ("text", up(input(DisplayMode::Text, 48.0, 96))),
            ("graph", up(input(DisplayMode::Graph, 48.0, 96))),
            (
                "inline",
                LayoutInput {
                    graph_inline: true,
                    ..up(input(DisplayMode::Graph, 48.0, 96))
                },
            ),
            ("vertical text", up(vertical(DisplayMode::Text, 120.0))),
            ("vertical graph", up(vertical(DisplayMode::Graph, 120.0))),
        ]
    }

    fn tiles_keep_pairs(name: &str) -> bool {
        matches!(name, "graph" | "inline")
    }

    fn two_adapters(c: &mut Config) {
        c.network.adapters = vec!["eth".into(), "wifi".into()];
    }

    #[test]
    fn showing_both_keeps_todays_cells_in_every_mode() {
        for up_first in [false, true] {
            for (name, i) in every_mode(up_first) {
                let order = network_parts(up_first);
                let want = if tiles_keep_pairs(name) {
                    vec![CellPart::Main]
                } else {
                    order.to_vec()
                };
                assert_eq!(network_cells(|_| {}, i), want, "{name}");
            }
        }
    }

    #[test]
    fn one_direction_is_one_cell_in_every_mode() {
        for up_first in [false, true] {
            for (name, i) in every_mode(up_first) {
                let down = |c: &mut Config| c.network.show = NetworkShow::Download;
                let up = |c: &mut Config| c.network.show = NetworkShow::Upload;
                assert_eq!(network_cells(down, i), [CellPart::Down], "{name}");
                assert_eq!(network_cells(up, i), [CellPart::Up], "{name}");
            }
        }
    }

    #[test]
    fn showing_both_for_each_adapter_pairs_them_in_every_mode() {
        for up_first in [false, true] {
            for (name, i) in every_mode(up_first) {
                let want: Vec<CellPart> = if tiles_keep_pairs(name) {
                    vec![CellPart::AdapterPair(0), CellPart::AdapterPair(1)]
                } else {
                    (0..2)
                        .flat_map(|a| {
                            network_pair(ItemKind::Network, CellPart::AdapterPair(a), up_first)
                                .unwrap()
                        })
                        .collect()
                };
                assert_eq!(network_cells(two_adapters, i), want, "{name} {up_first}");
            }
        }
    }

    #[test]
    fn one_direction_for_each_adapter_never_collapses_or_expands() {
        for up_first in [false, true] {
            for (name, i) in every_mode(up_first) {
                let down = |c: &mut Config| {
                    two_adapters(c);
                    c.network.show = NetworkShow::Download;
                };
                let up = |c: &mut Config| {
                    two_adapters(c);
                    c.network.show = NetworkShow::Upload;
                };
                assert_eq!(
                    network_cells(down, i),
                    [CellPart::AdapterDown(0), CellPart::AdapterDown(1)],
                    "{name}"
                );
                assert_eq!(
                    network_cells(up, i),
                    [CellPart::AdapterUp(0), CellPart::AdapterUp(1)],
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn split_tiles_give_graphs_a_tile_per_direction() {
        for up_first in [false, true] {
            for (name, i) in every_mode(up_first) {
                let split = |c: &mut Config| c.network.split_tiles = true;
                assert_eq!(
                    network_cells(split, i),
                    network_parts(up_first),
                    "{name} {up_first}"
                );
            }
        }
        let split_adapters = |c: &mut Config| {
            two_adapters(c);
            c.network.split_tiles = true;
        };
        let i = input(DisplayMode::Graph, 48.0, 96);
        assert_eq!(
            network_cells(split_adapters, i),
            [
                CellPart::AdapterDown(0),
                CellPart::AdapterUp(0),
                CellPart::AdapterDown(1),
                CellPart::AdapterUp(1)
            ]
        );
    }

    #[test]
    fn split_tiles_mean_nothing_with_one_direction_or_without_graphs() {
        let graph = input(DisplayMode::Graph, 48.0, 96);
        let upload = |c: &mut Config| {
            c.network.split_tiles = true;
            c.network.show = NetworkShow::Upload;
        };
        assert_eq!(network_cells(upload, graph), [CellPart::Up]);

        let mut plain = Config::default();
        plain.display.mode = DisplayMode::Text;
        let mut split = plain.clone();
        split.network.split_tiles = true;
        assert_eq!(
            item_specs(&split, &GpuSlots::new()),
            item_specs(&plain, &GpuSlots::new())
        );
    }

    #[test]
    fn text_columns_keep_pairs_and_one_direction_together() {
        // A 2-row bar with CPU first has one free row: a pair moves on whole,
        // and so does one direction of two adapters, as an item's parts.
        let at = |edit: &dyn Fn(&mut Config)| {
            let mut c = Config::default();
            c.display.mode = DisplayMode::Text;
            c.items
                .retain(|i| matches!(i.kind, ItemKind::Cpu | ItemKind::Network));
            edit(&mut c);
            compute_layout(
                &item_specs(&c, &GpuSlots::new()),
                input(DisplayMode::Text, 48.0, 96),
                &Fixed,
            )
        };
        let x = |l: &Layout, part| l.cells.iter().find(|c| c.part == part).unwrap().rect.x;
        let both = at(&two_adapters);
        assert_eq!(
            x(&both, CellPart::AdapterDown(0)),
            x(&both, CellPart::AdapterUp(0))
        );
        assert_ne!(
            x(&both, CellPart::AdapterDown(0)),
            x(&both, CellPart::AdapterDown(1))
        );
        let one = at(&|c: &mut Config| {
            two_adapters(c);
            c.network.show = NetworkShow::Upload;
        });
        assert_eq!(one.cells.len(), 3);
        assert_eq!(
            x(&one, CellPart::AdapterUp(0)),
            x(&one, CellPart::AdapterUp(1)),
            "they fill a column between them"
        );
        let three = at(&|c: &mut Config| {
            c.network.adapters = vec!["a".into(), "b".into(), "c".into()];
            c.network.show = NetworkShow::Download;
        });
        assert_eq!(three.cells.len(), 4);
        assert_no_overlap(&three);
    }

    #[test]
    fn a_single_direction_tile_is_narrower_than_a_pair() {
        let i = input(DisplayMode::Graph, 48.0, 96);
        let width = |part| {
            let items = [ItemSpec {
                kind: ItemKind::Network,
                parts: vec![part],
                tile: TileSize::default(),
                graphic: TileGraphic::default(),
            }];
            compute_layout(&items, i, &Fixed).cells[0].rect.w
        };
        // "99.9 MB/s" (54px) + 2 × 4px padding; a pair adds "↓" and a gap.
        assert_eq!(width(CellPart::Up), 54.0 + 8.0);
        assert_eq!(width(CellPart::AdapterDown(0)), 54.0 + 8.0);
        assert_eq!(width(CellPart::Main), 6.0 + 4.0 + 54.0 + 8.0);
        assert_eq!(width(CellPart::AdapterPair(0)), 12.0 + 4.0 + 54.0 + 8.0);
    }

    #[test]
    fn inline_slots_follow_the_part() {
        let text = TileText {
            labels: LabelStyle::Full,
            icons: false,
            vertical_labels: false,
            upload_on_top: true,
            names: names(),
        };
        let parts = |part| {
            inline_slots(ItemKind::Network, part, text, 10.0, 1.0, &Fixed)
                .0
                .iter()
                .map(|s| s.part)
                .collect::<Vec<_>>()
        };
        for single in [
            CellPart::Down,
            CellPart::Up,
            CellPart::AdapterDown(1),
            CellPart::AdapterUp(1),
        ] {
            assert_eq!(parts(single), [single]);
        }
        assert_eq!(
            parts(CellPart::AdapterPair(1)),
            [CellPart::AdapterUp(1), CellPart::AdapterDown(1)]
        );
        assert_eq!(parts(CellPart::Main), [CellPart::Up, CellPart::Down]);
    }

    #[test]
    fn only_network_main_and_adapter_pairs_are_pairs() {
        assert!(network_pair(ItemKind::Cpu, CellPart::Main, false).is_none());
        assert!(network_pair(ItemKind::Network, CellPart::Down, false).is_none());
        assert!(network_pair(ItemKind::Network, CellPart::AdapterUp(0), false).is_none());
        assert_eq!(CellPart::AdapterPair(0).base(), CellPart::Main);
        assert_eq!(
            part_metric(ItemKind::Network, CellPart::AdapterPair(1)),
            MetricKey::NetAdapterDownBps(1)
        );
    }

    fn geometry(l: &Layout) -> (Vec<[f32; 6]>, f32, f32) {
        let cells = l
            .cells
            .iter()
            .map(|c| [c.rect.x, c.rect.y, c.rect.w, c.rect.h, c.value_x, c.value_y])
            .collect();
        (cells, l.width, l.height)
    }

    #[test]
    fn a_config_from_before_the_network_choices_keeps_its_exact_layout() {
        let c: Config = toml::from_str("[display]\nmode = \"text\"\n").unwrap();
        let text = compute_layout(
            &item_specs(&c, &GpuSlots::new()),
            input(DisplayMode::Text, 48.0, 96),
            &Fixed,
        );
        let parts: Vec<_> = text.cells.iter().map(|c| (c.kind, c.part)).collect();
        assert_eq!(
            parts,
            [
                (ItemKind::Cpu, CellPart::Main),
                (ItemKind::Ram, CellPart::Main),
                (ItemKind::Gpu, CellPart::Main),
                (ItemKind::Disk, CellPart::Main),
                (ItemKind::Network, CellPart::Down),
                (ItemKind::Network, CellPart::Up),
            ]
        );
        assert_eq!(
            geometry(&text),
            (
                vec![
                    [4.0, 9.0, 46.0, 15.0, 26.0, 9.0],
                    [4.0, 24.0, 46.0, 15.0, 26.0, 24.0],
                    [60.0, 9.0, 46.0, 15.0, 82.0, 9.0],
                    [60.0, 24.0, 46.0, 15.0, 82.0, 24.0],
                    [116.0, 9.0, 64.0, 15.0, 126.0, 9.0],
                    [116.0, 24.0, 64.0, 15.0, 126.0, 24.0],
                ],
                184.0,
                48.0
            )
        );

        let c: Config = toml::from_str("").unwrap();
        let graph = compute_layout(
            &item_specs(&c, &GpuSlots::new()),
            input(DisplayMode::Graph, 48.0, 96),
            &Fixed,
        );
        let parts: Vec<_> = graph.cells.iter().map(|c| (c.kind, c.part)).collect();
        assert_eq!(
            parts,
            [
                (ItemKind::Cpu, CellPart::Main),
                (ItemKind::Ram, CellPart::Main),
                (ItemKind::Gpu, CellPart::Main),
                (ItemKind::Disk, CellPart::Main),
                (ItemKind::Network, CellPart::Main),
            ]
        );
        assert_eq!(
            geometry(&graph),
            (
                vec![
                    [4.0, 4.0, 44.0, 40.0, 8.0, 4.0],
                    [52.0, 4.0, 44.0, 40.0, 56.0, 4.0],
                    [100.0, 4.0, 44.0, 40.0, 104.0, 4.0],
                    [148.0, 4.0, 44.0, 40.0, 152.0, 4.0],
                    [196.0, 4.0, 72.0, 40.0, 200.0, 4.0],
                ],
                272.0,
                48.0
            )
        );
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
            cell_label(
                ItemKind::Disk,
                l.cells[1].part,
                LabelStyle::Full,
                false,
                names()
            ),
            "D:"
        );
    }

    #[test]
    fn per_drive_disk_has_a_tile_per_drive_in_graph_mode() {
        let items = [ItemSpec {
            kind: ItemKind::Disk,
            parts: vec![CellPart::DriveActive(b'C'), CellPart::DriveActive(b'D')],
            tile: TileSize::default(),
            graphic: TileGraphic::default(),
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
        let label = |kind, style| cell_label(kind, CellPart::Gpu(1), style, false, names());
        assert_eq!(label(ItemKind::Gpu, LabelStyle::Full), "GPU2");
        assert_eq!(label(ItemKind::GpuTemp, LabelStyle::Short), "T2");
        assert_eq!(
            label(ItemKind::Gpu, LabelStyle::None),
            "2",
            "the number stays"
        );
    }

    use crate::gpu_slots::adapter;

    fn gpu_config(chosen: &[&str]) -> Config {
        let mut c = Config::default();
        c.gpu.adapters = chosen.iter().map(ToString::to_string).collect();
        for kind in [ItemKind::Gpu, ItemKind::GpuTemp] {
            c.item_mut(kind).unwrap().enabled = true;
        }
        c
    }

    fn gpu_cells(specs: &[ItemSpec]) -> Vec<(ItemKind, Vec<CellPart>)> {
        specs
            .iter()
            .filter(|s| matches!(s.kind, ItemKind::Gpu | ItemKind::GpuTemp))
            .map(|s| (s.kind, s.parts.clone()))
            .collect()
    }

    #[test]
    fn each_gpu_keeps_its_load_temperature_and_memory_together() {
        let mut c = Config::default();
        let order = [ItemKind::Gpu, ItemKind::GpuTemp, ItemKind::GpuMemory];
        c.items.retain(|i| order.contains(&i.kind));
        c.items
            .sort_by_key(|i| order.iter().position(|&k| k == i.kind));
        c.items.iter_mut().for_each(|i| i.enabled = true);
        let cells: Vec<_> = specs_with(&c, &[0, 1])
            .iter()
            .flat_map(|s| s.parts.iter().map(move |&p| (s.kind, p)))
            .collect();
        assert_eq!(
            cells,
            [
                (ItemKind::Gpu, CellPart::Gpu(0)),
                (ItemKind::GpuTemp, CellPart::Gpu(0)),
                (ItemKind::GpuMemory, CellPart::Gpu(0)),
                (ItemKind::Gpu, CellPart::Gpu(1)),
                (ItemKind::GpuTemp, CellPart::Gpu(1)),
                (ItemKind::GpuMemory, CellPart::Gpu(1)),
            ]
        );
    }

    #[test]
    fn a_memory_cell_reads_its_own_card() {
        assert_eq!(
            primary_metric(ItemKind::GpuMemory),
            MetricKey::GpuMemoryPercent
        );
        assert_eq!(
            part_metric(ItemKind::GpuMemory, CellPart::Gpu(1)),
            MetricKey::GpuAdapterMemoryPercent(1)
        );
        let label =
            |style| cell_label(ItemKind::GpuMemory, CellPart::Gpu(1), style, false, names());
        assert_eq!(label(LabelStyle::Full), "VRAM2");
        assert_eq!(label(LabelStyle::Short), "V2");
    }

    #[test]
    fn a_chosen_gpu_absent_from_a_known_table_has_no_cell() {
        let c = gpu_config(&["RTX 4090"]);
        let gpus = GpuSlots::listing(&[adapter("Radeon", 1)]);
        let specs = item_specs(&c, &gpus);
        assert_eq!(gpu_cells(&specs), []);
        assert_eq!(
            specs.iter().map(|s| s.kind).collect::<Vec<_>>(),
            c.enabled_items()
                .into_iter()
                .filter(|k| !matches!(k, ItemKind::Gpu | ItemKind::GpuTemp))
                .collect::<Vec<_>>(),
            "the other items stay"
        );
    }

    #[test]
    fn the_combined_reading_stands_in_until_the_table_is_known() {
        let c = gpu_config(&["RTX 4090"]);
        assert_eq!(
            gpu_cells(&item_specs(&c, &GpuSlots::new())),
            [
                (ItemKind::Gpu, vec![CellPart::Main]),
                (ItemKind::GpuTemp, vec![CellPart::Main]),
            ]
        );
    }

    #[test]
    fn without_chosen_gpus_a_known_table_keeps_the_combined_reading() {
        let c = gpu_config(&[]);
        let gpus = GpuSlots::listing(&[adapter("Radeon", 1)]);
        assert_eq!(
            gpu_cells(&item_specs(&c, &gpus)),
            [
                (ItemKind::Gpu, vec![CellPart::Main]),
                (ItemKind::GpuTemp, vec![CellPart::Main]),
            ]
        );
    }

    #[test]
    fn a_chosen_gpu_leaving_takes_its_cell_and_returning_brings_it_back() {
        let c = gpu_config(&["RTX 4090"]);
        let card = [adapter("RTX 4090", 1)];
        let mut gpus = GpuSlots::listing(&card);
        let present = gpu_cells(&item_specs(&c, &gpus));
        assert_eq!(present[0], (ItemKind::Gpu, vec![CellPart::Gpu(0)]));
        gpus.update(&[]);
        assert_eq!(gpu_cells(&item_specs(&c, &gpus)), []);
        gpus.update(&card);
        assert_eq!(gpu_cells(&item_specs(&c, &gpus)), present);
    }

    #[test]
    fn a_chosen_gpu_still_there_keeps_its_cell_when_another_is_absent() {
        let c = gpu_config(&["Gone", "Radeon"]);
        let gpus = GpuSlots::listing(&[adapter("Radeon", 1)]);
        assert_eq!(
            gpu_cells(&item_specs(&c, &gpus)),
            [
                (ItemKind::Gpu, vec![CellPart::Gpu(0)]),
                (ItemKind::GpuTemp, vec![CellPart::Gpu(0)]),
            ]
        );
    }

    #[test]
    fn an_overlay_of_only_absent_gpus_has_no_width() {
        let mut c = gpu_config(&["RTX 4090"]);
        for item in &mut c.items {
            item.enabled = matches!(item.kind, ItemKind::Gpu | ItemKind::GpuTemp);
        }
        let gpus = GpuSlots::listing(&[adapter("Radeon", 1)]);
        let specs = item_specs(&c, &gpus);
        assert!(specs.is_empty());
        let l = compute_layout(&specs, input(DisplayMode::Text, 48.0, 96), &Fixed);
        assert_eq!(l.width, 0.0);
        assert!(l.cells.is_empty());
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
            [CellPart::Main],
            "one adapter takes the combined cells"
        );
        c.network.adapters.push("wifi".into());
        assert_eq!(
            item_parts(ItemKind::Network, &c, &[]),
            [CellPart::AdapterPair(0), CellPart::AdapterPair(1)]
        );
        let cells = |upload_on_top| {
            let mut i = input(DisplayMode::Text, 48.0, 96);
            i.upload_on_top = upload_on_top;
            let items = [ItemSpec {
                kind: ItemKind::Network,
                parts: item_parts(ItemKind::Network, &c, &[]),
                tile: TileSize::default(),
                graphic: TileGraphic::default(),
            }];
            compute_layout(&items, i, &Fixed)
                .cells
                .iter()
                .map(|c| c.part)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            cells(false),
            [
                CellPart::AdapterDown(0),
                CellPart::AdapterUp(0),
                CellPart::AdapterDown(1),
                CellPart::AdapterUp(1)
            ]
        );
        assert_eq!(
            cells(true)[..2],
            [CellPart::AdapterUp(0), CellPart::AdapterDown(0)]
        );
        assert_eq!(
            part_metric(ItemKind::Network, CellPart::AdapterUp(1)),
            MetricKey::NetAdapterUpBps(1)
        );
        let label =
            |part, icons| cell_label(ItemKind::Network, part, LabelStyle::Full, icons, names());
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
            parts: (0..n).map(CellPart::AdapterPair).collect(),
            tile: TileSize::default(),
            graphic: TileGraphic::default(),
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
            assert_eq!(parts, [CellPart::AdapterPair(0), CellPart::AdapterPair(1)]);
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
            cell_label(
                ItemKind::Cpu,
                CellPart::Main,
                LabelStyle::Short,
                false,
                names()
            ),
            "C"
        );
        assert_eq!(
            cell_label(
                ItemKind::Network,
                CellPart::Up,
                LabelStyle::Short,
                false,
                names()
            ),
            "↑"
        );
        assert_eq!(
            cell_label(
                ItemKind::Disk,
                CellPart::DriveActive(b'C'),
                LabelStyle::Short,
                false,
                names()
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
        let label = |style| cell_label(ItemKind::Ram, CellPart::Main, style, true, names());
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
            cell_label(
                ItemKind::Network,
                CellPart::Down,
                LabelStyle::Full,
                true,
                names()
            ),
            ""
        );
        assert_eq!(
            cell_label(
                ItemKind::Disk,
                CellPart::DriveActive(b'C'),
                LabelStyle::None,
                true,
                names()
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
