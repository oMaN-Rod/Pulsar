//! Names for items and choices, shared by the menu and the settings window.

use pulsar_core::config::{ColorMode, GraphStyle, PopupDetail, WidgetStyle};
use pulsar_core::metric::ItemKind;
use pulsar_core::{l10n, tr, tr_noop};

pub fn item_name(kind: ItemKind) -> &'static str {
    l10n::translate(item_msgid(kind))
}

pub fn item_msgid(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Cpu => tr_noop!("CPU"),
        ItemKind::Ram => tr_noop!("Memory"),
        ItemKind::Gpu => tr_noop!("GPU"),
        ItemKind::Disk => tr_noop!("Disk"),
        ItemKind::Network => tr_noop!("Network"),
        ItemKind::Ping => tr_noop!("Ping"),
        ItemKind::GpuTemp => tr_noop!("GPU temperature"),
        ItemKind::GpuMemory => tr_noop!("GPU memory"),
        ItemKind::CpuTemp => tr_noop!("CPU temperature"),
        ItemKind::CpuPower => tr_noop!("CPU power"),
        ItemKind::CpuClock => tr_noop!("CPU clock"),
        ItemKind::DiskTemp => tr_noop!("Drive temperature"),
        ItemKind::ChipsetTemp => tr_noop!("Chipset temperature"),
        ItemKind::Fps => tr_noop!("Frame rate"),
        ItemKind::Battery => tr_noop!("Battery"),
    }
}

const CPU_STYLES: [Option<GraphStyle>; 14] = [
    Some(GraphStyle::Area),
    Some(GraphStyle::Line),
    Some(GraphStyle::Bars),
    Some(GraphStyle::Meter),
    Some(GraphStyle::Segments),
    Some(GraphStyle::Spark),
    Some(GraphStyle::Ring),
    Some(GraphStyle::Cores),
    Some(GraphStyle::Heat),
    Some(GraphStyle::HeatStrip),
    Some(GraphStyle::DotMatrix),
    Some(GraphStyle::Bullet),
    Some(GraphStyle::RangeRail),
    None,
];

const PLAIN_STYLES: [Option<GraphStyle>; 12] = [
    Some(GraphStyle::Area),
    Some(GraphStyle::Line),
    Some(GraphStyle::Bars),
    Some(GraphStyle::Meter),
    Some(GraphStyle::Segments),
    Some(GraphStyle::Spark),
    Some(GraphStyle::Ring),
    Some(GraphStyle::HeatStrip),
    Some(GraphStyle::DotMatrix),
    Some(GraphStyle::Bullet),
    Some(GraphStyle::RangeRail),
    None,
];

/// In menu order; `None` is text only.
pub const STYLE_SETS: [&[Option<GraphStyle>]; 2] = [&CPU_STYLES, &PLAIN_STYLES];

pub fn style_set(kind: ItemKind) -> usize {
    match kind {
        ItemKind::Cpu => 0,
        _ => 1,
    }
}

pub fn styles_for(kind: ItemKind) -> &'static [Option<GraphStyle>] {
    STYLE_SETS[style_set(kind)]
}

/// The widget's styles for `kind`; the first four are for every item.
pub fn widget_styles_for(kind: ItemKind) -> &'static [WidgetStyle] {
    if kind == ItemKind::Cpu {
        &WIDGET_STYLES
    } else {
        &WIDGET_STYLES[..4]
    }
}

pub fn style_name(style: Option<GraphStyle>) -> &'static str {
    match style {
        Some(GraphStyle::Area) => tr!("Area graph"),
        Some(GraphStyle::Line) => tr!("Line graph"),
        Some(GraphStyle::Bars) => tr!("Bar graph"),
        Some(GraphStyle::Meter) => tr!("Level meter"),
        Some(GraphStyle::Ring) => tr!("Ring gauge"),
        Some(GraphStyle::Segments) => tr!("Segment meter"),
        Some(GraphStyle::Spark) => tr!("Spark underline"),
        Some(GraphStyle::Cores) => tr!("Core bars"),
        Some(GraphStyle::Heat) => tr!("Heat cells"),
        Some(GraphStyle::HeatStrip) => tr!("Heat strip"),
        Some(GraphStyle::DotMatrix) => tr!("Dot matrix"),
        Some(GraphStyle::Bullet) => tr!("Bullet meter"),
        Some(GraphStyle::RangeRail) => tr!("Range rail"),
        None => tr!("Text only"),
    }
}

pub const COLOR_MODES: [ColorMode; 3] = [ColorMode::Item, ColorMode::Accent, ColorMode::Load];

pub fn color_mode_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Item => tr!("Item color"),
        ColorMode::Accent => tr!("Windows accent"),
        ColorMode::Load => tr!("Color by load"),
    }
}

pub const DETAILS: [PopupDetail; 3] = [
    PopupDetail::Simple,
    PopupDetail::Standard,
    PopupDetail::Detailed,
];

pub const WIDGET_STYLES: [WidgetStyle; 5] = [
    WidgetStyle::Sparklines,
    WidgetStyle::Tiles,
    WidgetStyle::Meters,
    WidgetStyle::Dials,
    WidgetStyle::CoreWall,
];

pub fn widget_style_name(style: WidgetStyle) -> &'static str {
    match style {
        WidgetStyle::Sparklines => tr!("Sparkline"),
        WidgetStyle::Tiles => tr!("Tile"),
        WidgetStyle::Meters => tr!("Meter"),
        WidgetStyle::Dials => tr!("Dial"),
        WidgetStyle::CoreWall => tr!("Core wall"),
    }
}

pub fn detail_name(detail: PopupDetail) -> &'static str {
    match detail {
        PopupDetail::Simple => tr!("Simple"),
        PopupDetail::Standard => tr!("Standard"),
        PopupDetail::Detailed => tr!("Detailed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_list_only_the_styles_they_can_draw() {
        let lists = |kind, style| styles_for(kind).contains(&Some(style));
        assert!(!lists(ItemKind::Ram, GraphStyle::Heat));
    }

    #[test]
    fn every_item_and_choice_has_a_name() {
        for kind in ItemKind::ALL {
            assert!(!item_name(kind).is_empty());
        }
        for style in STYLE_SETS.into_iter().flatten() {
            assert!(!style_name(*style).is_empty());
        }
        for mode in COLOR_MODES {
            assert!(!color_mode_name(mode).is_empty());
        }
        for detail in DETAILS {
            assert!(!detail_name(detail).is_empty());
        }
    }
}
