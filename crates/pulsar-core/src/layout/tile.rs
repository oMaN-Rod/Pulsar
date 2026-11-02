//! Where a graph tile puts its label, graphic and text, for both measuring
//! and drawing.

use unicode_segmentation::UnicodeSegmentation;

use super::{
    CellPart, GRAPH_FONT_SCALE, Labels, PADDING_DIP, Rect, TextMeasure, TileText, cell_icon,
    cell_label, graph_text_width, inline_slots, network_pair, part_metric,
};
use crate::config::GraphStyle;
use crate::format::widest_value;
use crate::metric::ItemKind;

const STACK_INSET_DIP: f32 = 2.0;
const STACK_GAP_DIP: f32 = 3.0;
const STACK_MIN_PX_DIP: f32 = 7.0;
/// Upper-case letters leave room between them at one line per em.
const STACK_TIGHT_EM: f32 = 1.0;
const STACK_LOOSE_EM: f32 = 1.12;
const RING_INSET_DIP: f32 = 3.0;
const RING_GAP_DIP: f32 = 4.0;
const CORES_GAP_DIP: f32 = 5.0;
const CORES_INSET_DIP: f32 = 4.0;
const CORE_PITCH_DIP: f32 = 3.0;
const CORE_BAR_GAP_DIP: f32 = 1.0;
const MIN_CORES_DIP: f32 = 36.0;
const MAX_CORES_DIP: f32 = 120.0;
const HEAT_PITCH_DIP: f32 = 10.0;
const MIN_HEAT_DIP: f32 = 24.0;
const STRIP_BESIDE_DIP: f32 = 40.0;
const STRIP_TOP_DIP: f32 = 2.0;
const STRIP_BOTTOM_DIP: f32 = 3.0;
const MIN_STRIP_DIP: f32 = 5.0;
const STRIP_LABEL_GAP_DIP: f32 = 4.0;
const BUBBLE_DIP: f32 = 10.0;
const BATTERY_W_DIP: f32 = 24.0;
const BATTERY_H_DIP: f32 = 12.0;
const BADGE_GAP_DIP: f32 = 4.0;
const TREND_DIP: f32 = 8.0;
const TREND_GAP_DIP: f32 = 3.0;

/// What a tile needs room for beside its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TileGraphic {
    /// Nothing, or a graph drawn behind the text.
    #[default]
    Behind,
    Ring,
    /// One bar per logical processor, plus any empty slot between core types.
    Cores(u16),
    /// One cell per logical processor.
    Heat(u16),
    /// A level or sample strip under or beside the text.
    Segments,
    Spark,
    Bubble,
    Battery,
}

impl TileGraphic {
    pub fn of(style: GraphStyle, cores: u16, hybrid: bool) -> Self {
        match style {
            GraphStyle::Ring => Self::Ring,
            GraphStyle::Cores => Self::Cores(cores.saturating_add(u16::from(hybrid))),
            GraphStyle::Heat => Self::Heat(cores),
            GraphStyle::Segments
            | GraphStyle::HeatStrip
            | GraphStyle::Bullet
            | GraphStyle::RangeRail
            | GraphStyle::Uptime
            | GraphStyle::CoreTypes => Self::Segments,
            GraphStyle::Spark => Self::Spark,
            GraphStyle::Bubble | GraphStyle::Combined => Self::Bubble,
            GraphStyle::BatteryCell => Self::Battery,
            GraphStyle::Area
            | GraphStyle::Line
            | GraphStyle::Bars
            | GraphStyle::Meter
            | GraphStyle::DotMatrix
            | GraphStyle::MirrorBars
            | GraphStyle::Tint => Self::Behind,
        }
    }
}

/// How the overlay's tiles are arranged.
#[derive(Clone, Copy, Debug)]
pub struct TileLook<'a> {
    pub text: TileText<'a>,
    pub inline: bool,
    pub vertical_bar: bool,
    pub trend: bool,
    pub scale: f32,
}

/// A label drawn one upright letter per line.
#[derive(Clone, Debug, PartialEq)]
pub struct StackedLabel {
    pub icon: Option<&'static str>,
    pub glyphs: Vec<String>,
    pub px: f32,
    pub step: f32,
    pub width: f32,
}

impl StackedLabel {
    pub fn lines(&self) -> usize {
        self.glyphs.len() + usize::from(self.icon.is_some())
    }
}

/// Shrinks to fit `height` down to a readable size, then drops what is left.
pub fn stacked_label(
    kind: ItemKind,
    part: CellPart,
    text: TileText<'_>,
    font_px: f32,
    height: f32,
    scale: f32,
    m: &dyn TextMeasure,
) -> StackedLabel {
    let label = cell_label(kind, part, text.labels, text.icons, text.names);
    let mut glyphs: Vec<String> = label
        .graphemes(true)
        .filter(|g| !g.trim().is_empty() && *g != ":")
        .map(str::to_owned)
        .collect();
    let icon = text.icons.then(|| cell_icon(kind, part));
    let icon_lines = usize::from(icon.is_some());
    let lines = (glyphs.len() + icon_lines).max(1) as f32;
    let px = font_px
        .min(height / lines / STACK_TIGHT_EM)
        .max(font_px.min(STACK_MIN_PX_DIP * scale));
    let fits = (height / (px * STACK_TIGHT_EM)).floor().max(1.0) as usize;
    glyphs.truncate(fits.saturating_sub(icon_lines));
    let shown = (glyphs.len() + icon_lines).max(1) as f32;
    let step = (px * STACK_LOOSE_EM)
        .min(height / shown)
        .max(px * STACK_TIGHT_EM);
    let width = glyphs
        .iter()
        .map(|g| m.width(g, px))
        .chain(icon.map(|i| m.icon_width(i, px)))
        .fold(0.0, f32::max);
    StackedLabel {
        icon,
        glyphs,
        px,
        step,
        width,
    }
}

fn ring_diameter(tile_h: f32, scale: f32) -> f32 {
    (tile_h - 2.0 * RING_INSET_DIP * scale).max(1.0)
}

fn cores_width(count: u16, scale: f32) -> f32 {
    let pitch = (CORE_PITCH_DIP * scale).round().max(2.0);
    let gap = (CORE_BAR_GAP_DIP * scale).round().max(1.0);
    (f32::from(count) * pitch - gap).clamp(MIN_CORES_DIP * scale, MAX_CORES_DIP * scale)
}

/// Rows of heat cells in `area_h`, so the cells stay about square.
pub fn heat_rows(area_h: f32, scale: f32) -> usize {
    ((area_h / (HEAT_PITCH_DIP * scale)).floor() as usize).clamp(1, 4)
}

fn heat_width(count: u16, tile_h: f32, scale: f32) -> f32 {
    let rows = heat_rows(tile_h - 2.0 * CORES_INSET_DIP * scale, scale);
    let columns = usize::from(count).div_ceil(rows) as f32;
    (columns * HEAT_PITCH_DIP * scale - scale).clamp(MIN_HEAT_DIP * scale, MAX_CORES_DIP * scale)
}

/// Whether a strip fits under one line of text; otherwise it goes beside.
fn strip_below(
    kind: ItemKind,
    part: CellPart,
    look: TileLook<'_>,
    tile_h: f32,
    px: f32,
    m: &dyn TextMeasure,
) -> bool {
    let scale = look.scale;
    let spare = tile_h - (STRIP_TOP_DIP + STRIP_BOTTOM_DIP) * scale - m.line_height(px);
    !look.inline
        && network_pair(kind, part, look.text.upload_on_top).is_none()
        && spare >= MIN_STRIP_DIP * scale
}

/// The room a tile's label and graphic take beside its text. Stacked tiles on
/// a vertical bar have none to spare, so their graphic goes behind the text.
pub fn tile_chrome(
    kind: ItemKind,
    part: CellPart,
    graphic: TileGraphic,
    look: TileLook<'_>,
    tile_h: f32,
    px: f32,
    m: &dyn TextMeasure,
) -> f32 {
    let scale = look.scale;
    let stack = look.text.vertical_labels.then(|| {
        let height = tile_h - 2.0 * STACK_INSET_DIP * scale;
        stacked_label(kind, part, look.text, px, height, scale, m).width
    });
    let beside = match graphic {
        TileGraphic::Behind => 0.0,
        TileGraphic::Ring | TileGraphic::Cores(_) | TileGraphic::Heat(_) if look.vertical_bar => {
            0.0
        }
        TileGraphic::Battery if look.vertical_bar => 0.0,
        TileGraphic::Bubble => (BUBBLE_DIP + BADGE_GAP_DIP) * scale,
        TileGraphic::Battery => (BATTERY_W_DIP + BADGE_GAP_DIP) * scale,
        TileGraphic::Ring => ring_diameter(tile_h, scale) + RING_GAP_DIP * scale,
        TileGraphic::Cores(count) => cores_width(count, scale) + CORES_GAP_DIP * scale,
        TileGraphic::Heat(count) => heat_width(count, tile_h, scale) + CORES_GAP_DIP * scale,
        TileGraphic::Segments | TileGraphic::Spark
            if strip_below(kind, part, look, tile_h, px, m) =>
        {
            // The label and value share the line above the strip.
            if look.text.vertical_labels {
                0.0
            } else {
                let labels = Labels {
                    style: look.text.labels,
                    icons: look.text.icons,
                    vertical: false,
                    names: look.text.names,
                };
                let label = labels.width(kind, part, px, m);
                let value = m.width(widest_value(part_metric(kind, part).unit()), px);
                label.min(value) + STRIP_LABEL_GAP_DIP * scale
            }
        }
        TileGraphic::Segments | TileGraphic::Spark if look.vertical_bar => 0.0,
        TileGraphic::Segments | TileGraphic::Spark => (STRIP_BESIDE_DIP + CORES_GAP_DIP) * scale,
    };
    stack.map_or(0.0, |w| w + STACK_GAP_DIP * scale) + beside + trend_room(graphic, look)
}

/// On a narrow vertical bar an arrow shares the bubble's column.
fn shares_column(graphic: TileGraphic, look: TileLook<'_>) -> bool {
    look.trend && look.vertical_bar && graphic == TileGraphic::Bubble
}

fn trend_room(graphic: TileGraphic, look: TileLook<'_>) -> f32 {
    if look.trend && !shares_column(graphic, look) {
        (TREND_DIP + TREND_GAP_DIP) * look.scale
    } else {
        0.0
    }
}

/// Where each part of a tile is drawn; `px` is the size of its text.
#[derive(Clone, Debug, PartialEq)]
pub struct TileRegions {
    pub stack: Option<(StackedLabel, Rect)>,
    pub ring: Option<Rect>,
    /// Per-core bars or cells.
    pub cores: Option<Rect>,
    /// A segment meter or a spark line.
    pub strip: Option<Rect>,
    /// A status dot or battery beside the text.
    pub badge: Option<Rect>,
    pub trend: Option<Rect>,
    /// The strip is under one line holding both the label and the value.
    pub strip_below: bool,
    pub text_x: f32,
    /// For a graph behind the text.
    pub graph: Rect,
}

pub fn tile_regions(
    rect: Rect,
    kind: ItemKind,
    part: CellPart,
    graphic: TileGraphic,
    look: TileLook<'_>,
    px: f32,
    m: &dyn TextMeasure,
) -> TileRegions {
    let scale = look.scale;
    let inset = scale;
    let mut x = rect.x + PADDING_DIP * scale;
    let mut graph = Rect {
        x: rect.x + inset,
        y: rect.y + inset,
        w: rect.w - 2.0 * inset,
        h: rect.h - 2.0 * inset,
    };
    let graph_right = rect.right() - inset;
    let shared = shares_column(graphic, look);
    let trend = look.trend.then(|| {
        let w = (TREND_DIP * scale).round();
        let top = if shared { rect.h / 2.0 } else { 0.0 };
        Rect {
            x: (rect.right() - PADDING_DIP * scale - w).round(),
            y: rect.y + top,
            w,
            h: rect.h - top,
        }
    });
    let rect = Rect {
        w: (rect.w - trend_room(graphic, look)).max(1.0),
        ..rect
    };
    let stack = look.text.vertical_labels.then(|| {
        let height = rect.h - 2.0 * STACK_INSET_DIP * scale;
        let label = stacked_label(kind, part, look.text, px, height, scale, m);
        let area = Rect {
            x,
            y: rect.y + STACK_INSET_DIP * scale,
            w: label.width,
            h: height,
        };
        x += label.width + STACK_GAP_DIP * scale;
        graph.x = x - inset;
        graph.w = graph_right - graph.x;
        (label, area)
    });
    let ring = (graphic == TileGraphic::Ring).then(|| {
        let mut diameter = ring_diameter(rect.h, scale);
        if look.vertical_bar {
            diameter = diameter.min(rect.w - 2.0 * RING_INSET_DIP * scale).max(1.0);
        }
        let left = if look.vertical_bar {
            rect.right() - RING_INSET_DIP * scale - diameter
        } else {
            x
        };
        if !look.vertical_bar {
            x += diameter + RING_GAP_DIP * scale;
        }
        Rect {
            x: left,
            y: rect.y + (rect.h - diameter) / 2.0,
            w: diameter,
            h: diameter,
        }
    });
    let beside = |x: f32| {
        let text_w = if look.inline {
            inline_slots(kind, part, look.text, px, scale, m).1
        } else {
            graph_text_width(kind, part, look.text, px / GRAPH_FONT_SCALE, m)
        };
        let left = (x + text_w + CORES_GAP_DIP * scale).round();
        let right = (rect.right() - PADDING_DIP * scale).round();
        let inset = CORES_INSET_DIP * scale;
        Rect {
            x: left,
            y: (rect.y + inset).round(),
            w: (right - left).max(1.0),
            h: (rect.h - 2.0 * inset).round().max(1.0),
        }
    };
    let cores = matches!(graphic, TileGraphic::Cores(_) | TileGraphic::Heat(_))
        .then(|| if look.vertical_bar { graph } else { beside(x) });
    let is_strip = matches!(graphic, TileGraphic::Segments | TileGraphic::Spark);
    let below = is_strip && strip_below(kind, part, look, rect.h, px, m);
    let strip = is_strip.then(|| {
        if below {
            let top = rect.y + STRIP_TOP_DIP * scale + m.line_height(px);
            let bottom = rect.y + rect.h - STRIP_BOTTOM_DIP * scale;
            Rect {
                x: x.round(),
                y: top.round(),
                w: (rect.right() - PADDING_DIP * scale - x).round().max(1.0),
                h: (bottom - top).round().max(1.0),
            }
        } else if look.vertical_bar {
            graph
        } else {
            beside(x)
        }
    });
    let badge_size = match graphic {
        TileGraphic::Bubble => Some((BUBBLE_DIP, BUBBLE_DIP)),
        TileGraphic::Battery => Some((BATTERY_W_DIP, BATTERY_H_DIP)),
        _ => None,
    };
    let badge = badge_size.map(|(w, h)| {
        if graphic == TileGraphic::Battery && look.vertical_bar {
            return graph;
        }
        let (w, h) = ((w * scale).round(), (h * scale).round());
        let share = if shared { rect.h / 2.0 } else { rect.h };
        Rect {
            x: (rect.right() - PADDING_DIP * scale - w).round(),
            y: (rect.y + (share - h) / 2.0).round(),
            w,
            h,
        }
    });
    TileRegions {
        stack,
        ring,
        cores,
        strip,
        badge,
        trend,
        strip_below: below,
        text_x: x,
        graph,
    }
}
