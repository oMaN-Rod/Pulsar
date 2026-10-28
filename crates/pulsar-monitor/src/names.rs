//! Names for items and choices, shared by the menu and the settings window.

use pulsar_core::config::{ColorMode, GraphStyle, PopupDetail};
use pulsar_core::metric::ItemKind;
use pulsar_core::tr;

pub fn item_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Cpu => tr!("CPU"),
        ItemKind::Ram => tr!("Memory"),
        ItemKind::Gpu => tr!("GPU"),
        ItemKind::Disk => tr!("Disk"),
        ItemKind::Network => tr!("Network"),
        ItemKind::Ping => tr!("Ping"),
        ItemKind::GpuTemp => tr!("GPU temperature"),
        ItemKind::CpuTemp => tr!("CPU temperature"),
        ItemKind::CpuPower => tr!("CPU power"),
        ItemKind::CpuClock => tr!("CPU clock"),
        ItemKind::DiskTemp => tr!("Drive temperature"),
        ItemKind::ChipsetTemp => tr!("Chipset temperature"),
        ItemKind::Battery => tr!("Battery"),
    }
}

/// Graph styles in menu order; `None` is text only.
pub const STYLES: [Option<GraphStyle>; 5] = [
    Some(GraphStyle::Area),
    Some(GraphStyle::Line),
    Some(GraphStyle::Bars),
    Some(GraphStyle::Meter),
    None,
];

pub fn style_name(style: Option<GraphStyle>) -> &'static str {
    match style {
        Some(GraphStyle::Area) => tr!("Area graph"),
        Some(GraphStyle::Line) => tr!("Line graph"),
        Some(GraphStyle::Bars) => tr!("Bar graph"),
        Some(GraphStyle::Meter) => tr!("Level meter"),
        None => tr!("Text only"),
    }
}

pub const COLOR_MODES: [ColorMode; 3] = [ColorMode::Item, ColorMode::Accent, ColorMode::Load];

pub fn color_mode_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Item => tr!("Item colour"),
        ColorMode::Accent => tr!("Windows accent"),
        ColorMode::Load => tr!("Colour by load"),
    }
}

pub const DETAILS: [PopupDetail; 3] = [
    PopupDetail::Simple,
    PopupDetail::Standard,
    PopupDetail::Detailed,
];

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
    fn every_item_and_choice_has_a_name() {
        for kind in ItemKind::ALL {
            assert!(!item_name(kind).is_empty());
        }
        for style in STYLES {
            assert!(!style_name(style).is_empty());
        }
        for mode in COLOR_MODES {
            assert!(!color_mode_name(mode).is_empty());
        }
        for detail in DETAILS {
            assert!(!detail_name(detail).is_empty());
        }
    }
}
