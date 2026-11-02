//! Default item colors, shared by the overlay and the settings window.

use crate::config::LoadThresholds;
use crate::layout::CellPart;
use crate::metric::{ItemKind, Unit};

pub type Rgb = (u8, u8, u8);

pub fn default_rgb(kind: ItemKind, part: CellPart) -> Rgb {
    match (kind, part) {
        (ItemKind::Cpu, _) => (0x4C, 0xC2, 0xFF),
        (ItemKind::Ram, _) => (0xB1, 0x86, 0xF6),
        (ItemKind::Disk, _) => (0x6C, 0xCB, 0x5F),
        (ItemKind::Network, CellPart::Up) => (0xF7, 0x63, 0x0C),
        (ItemKind::Network, _) => (0xFF, 0xB9, 0x00),
        (ItemKind::Gpu, _) => (0xFF, 0x6F, 0xB5),
        (ItemKind::GpuTemp, _) => (0xFF, 0x8C, 0x42),
        (ItemKind::GpuMemory, _) => (0xE8, 0x7B, 0xDE),
        (ItemKind::CpuTemp, _) => (0xFF, 0x5A, 0x5F),
        (ItemKind::CpuPower, _) => (0xF5, 0xD0, 0x4A),
        (ItemKind::CpuClock, _) => (0x8E, 0xA8, 0xFF),
        (ItemKind::ChipsetTemp, _) => (0xFF, 0x9E, 0x7A),
        (ItemKind::DiskTemp, _) => (0xB5, 0xE3, 0x6B),
        (ItemKind::Ping, _) => (0x2E, 0xD5, 0xC4),
        (ItemKind::Battery, _) => (0x7E, 0xD3, 0x21),
        (ItemKind::Fps, _) => (0xD9, 0x6B, 0xFF),
    }
}

/// How high a reading is, for coloring by load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Load {
    Low,
    Moderate,
    High,
    Critical,
}

/// The load a value represents; `None` for units without a natural scale,
/// such as transfer rates.
pub fn load_level(unit: Unit, value: f64, thresholds: &LoadThresholds) -> Option<Load> {
    let [moderate, high, critical] = match unit {
        Unit::Percent => thresholds.percent,
        Unit::Celsius => thresholds.celsius,
        Unit::Millis => thresholds.millis,
        _ => return None,
    }
    .map(f64::from);
    if !value.is_finite() {
        return None;
    }
    Some(if value >= critical {
        Load::Critical
    } else if value >= high {
        Load::High
    } else if value >= moderate {
        Load::Moderate
    } else {
        Load::Low
    })
}

/// Green to red, darker on a light taskbar so it stays readable.
pub fn load_rgb(load: Load, light_taskbar: bool) -> Rgb {
    match (load, light_taskbar) {
        (Load::Low, false) => (0x6C, 0xCB, 0x5F),
        (Load::Moderate, false) => (0xFF, 0xD3, 0x3D),
        (Load::High, false) => (0xFF, 0x8C, 0x1A),
        (Load::Critical, false) => (0xFF, 0x4D, 0x4F),
        (Load::Low, true) => (0x10, 0x7C, 0x10),
        (Load::Moderate, true) => (0xA8, 0x80, 0x00),
        (Load::High, true) => (0xC8, 0x5A, 0x00),
        (Load::Critical, true) => (0xC4, 0x2B, 0x1C),
    }
}

/// The overlay's colors when no custom color is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeRgb {
    pub text: Rgb,
    pub label: Rgb,
    /// Drawn at the theme's tile opacity.
    pub tile: Rgb,
    pub panel: Rgb,
}

pub fn theme_rgb(light_taskbar: bool) -> ThemeRgb {
    if light_taskbar {
        ThemeRgb {
            text: (0x1A, 0x1A, 0x1A),
            label: (0x5C, 0x5C, 0x5C),
            tile: (0, 0, 0),
            panel: (0xF3, 0xF3, 0xF3),
        }
    } else {
        ThemeRgb {
            text: (0xFF, 0xFF, 0xFF),
            label: (0xB8, 0xB8, 0xB8),
            tile: (0xFF, 0xFF, 0xFF),
            panel: (0x20, 0x20, 0x20),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_text_contrasts_with_the_taskbar() {
        assert_eq!(theme_rgb(false).text, (0xFF, 0xFF, 0xFF));
        assert_eq!(theme_rgb(true).text, (0x1A, 0x1A, 0x1A));
        assert_ne!(theme_rgb(false).panel, theme_rgb(true).panel);
    }

    #[test]
    fn items_have_distinct_default_colors() {
        let all: Vec<Rgb> = ItemKind::ALL
            .into_iter()
            .map(|k| default_rgb(k, CellPart::Main))
            .collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:?} repeats");
        }
    }

    #[test]
    fn load_levels_follow_the_unit() {
        let level = |unit, value| load_level(unit, value, &LoadThresholds::default());
        assert_eq!(level(Unit::Percent, 12.0), Some(Load::Low));
        assert_eq!(level(Unit::Percent, 50.0), Some(Load::Moderate));
        assert_eq!(level(Unit::Percent, 80.0), Some(Load::High));
        assert_eq!(level(Unit::Percent, 95.0), Some(Load::Critical));
        assert_eq!(level(Unit::Celsius, 92.0), Some(Load::Critical));
        assert_eq!(level(Unit::Celsius, 65.0), Some(Load::Low));
        assert_eq!(level(Unit::Millis, 120.0), Some(Load::High));
        assert_eq!(level(Unit::NetworkRate, 1e9), None);
        assert_eq!(level(Unit::Percent, f64::NAN), None);
    }

    #[test]
    fn load_levels_follow_custom_thresholds() {
        let t = LoadThresholds {
            percent: [10, 20, 30],
            celsius: [40, 50, 60],
            millis: [5, 5, 200],
        };
        assert_eq!(load_level(Unit::Percent, 9.9, &t), Some(Load::Low));
        assert_eq!(load_level(Unit::Percent, 10.0, &t), Some(Load::Moderate));
        assert_eq!(load_level(Unit::Percent, 30.0, &t), Some(Load::Critical));
        assert_eq!(load_level(Unit::Celsius, 55.0, &t), Some(Load::High));
        assert_eq!(
            load_level(Unit::Millis, 5.0, &t),
            Some(Load::High),
            "equal thresholds skip a color"
        );
    }

    #[test]
    fn load_colors_differ_by_level_and_taskbar() {
        let levels = [Load::Low, Load::Moderate, Load::High, Load::Critical];
        for light in [false, true] {
            let all: Vec<Rgb> = levels.iter().map(|&l| load_rgb(l, light)).collect();
            for (i, a) in all.iter().enumerate() {
                assert!(!all[i + 1..].contains(a));
            }
        }
        assert_ne!(load_rgb(Load::Low, false), load_rgb(Load::Low, true));
    }

    #[test]
    fn network_up_differs_from_down() {
        assert_ne!(
            default_rgb(ItemKind::Network, CellPart::Down),
            default_rgb(ItemKind::Network, CellPart::Up)
        );
        assert_eq!(
            default_rgb(ItemKind::Network, CellPart::Main),
            default_rgb(ItemKind::Network, CellPart::Down)
        );
    }
}
