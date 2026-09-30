//! One-click colour themes. Applying one fills the form; nothing about the
//! preset itself is stored.

use pulsar_core::metric::ItemKind;

use crate::form::Form;

pub struct Preset {
    pub name: &'static str,
    pub items: &'static [(ItemKind, &'static str)],
    pub label: Option<&'static str>,
    pub value: Option<&'static str>,
    pub color_labels: bool,
    /// Colour and opacity percent.
    pub tile: Option<(&'static str, u8)>,
    pub panel: Option<(&'static str, u8)>,
}

use ItemKind::{Cpu, Disk, Gpu, GpuTemp, Network, Ping, Ram};

pub const PRESETS: [Preset; 8] = [
    Preset {
        name: "Windows default",
        items: &[],
        label: None,
        value: None,
        color_labels: false,
        tile: None,
        panel: None,
    },
    Preset {
        name: "Aurora",
        items: &[
            (Cpu, "#5EEAD4"),
            (Ram, "#C4B5FD"),
            (Gpu, "#6EE7B7"),
            (GpuTemp, "#F9A8D4"),
            (Disk, "#93C5FD"),
            (Network, "#FDE68A"),
            (Ping, "#99F6E4"),
        ],
        label: Some("#94A3B8"),
        value: Some("#F8FAFC"),
        color_labels: false,
        tile: None,
        panel: None,
    },
    Preset {
        name: "Ember",
        items: &[
            (Cpu, "#FB923C"),
            (Ram, "#F87171"),
            (Gpu, "#FBBF24"),
            (GpuTemp, "#EF4444"),
            (Disk, "#FDBA74"),
            (Network, "#FCD34D"),
            (Ping, "#FCA5A5"),
        ],
        label: Some("#D6A28B"),
        value: Some("#FFF7ED"),
        color_labels: false,
        tile: Some(("#431407", 40)),
        panel: Some(("#1C0A04", 70)),
    },
    Preset {
        name: "Ocean",
        items: &[
            (Cpu, "#38BDF8"),
            (Ram, "#818CF8"),
            (Gpu, "#22D3EE"),
            (GpuTemp, "#F472B6"),
            (Disk, "#60A5FA"),
            (Network, "#2DD4BF"),
            (Ping, "#A5B4FC"),
        ],
        label: Some("#7DD3FC"),
        value: Some("#E0F2FE"),
        color_labels: false,
        tile: Some(("#082F49", 45)),
        panel: None,
    },
    Preset {
        name: "Forest",
        items: &[
            (Cpu, "#86EFAC"),
            (Ram, "#BEF264"),
            (Gpu, "#4ADE80"),
            (GpuTemp, "#FACC15"),
            (Disk, "#A3E635"),
            (Network, "#6EE7B7"),
            (Ping, "#D9F99D"),
        ],
        label: Some("#A3B899"),
        value: Some("#F0FDF4"),
        color_labels: false,
        tile: Some(("#052E16", 45)),
        panel: None,
    },
    Preset {
        name: "Mono",
        items: &[
            (Cpu, "#F3F4F6"),
            (Ram, "#D1D5DB"),
            (Gpu, "#E5E7EB"),
            (GpuTemp, "#9CA3AF"),
            (Disk, "#CBD5E1"),
            (Network, "#F9FAFB"),
            (Ping, "#A1A1AA"),
        ],
        label: Some("#9CA3AF"),
        value: Some("#FFFFFF"),
        color_labels: false,
        tile: None,
        panel: None,
    },
    Preset {
        name: "Neon",
        items: &[
            (Cpu, "#22D3EE"),
            (Ram, "#E879F9"),
            (Gpu, "#A3E635"),
            (GpuTemp, "#FB7185"),
            (Disk, "#FACC15"),
            (Network, "#2DD4BF"),
            (Ping, "#818CF8"),
        ],
        label: Some("#A1A1AA"),
        value: Some("#FFFFFF"),
        color_labels: true,
        tile: None,
        panel: Some(("#0B0B12", 75)),
    },
    Preset {
        name: "Sunset",
        items: &[
            (Cpu, "#F472B6"),
            (Ram, "#FB923C"),
            (Gpu, "#FBBF24"),
            (GpuTemp, "#F43F5E"),
            (Disk, "#FDA4AF"),
            (Network, "#FCD34D"),
            (Ping, "#C084FC"),
        ],
        label: Some("#FDBA74"),
        value: Some("#FFF1F2"),
        color_labels: false,
        tile: Some(("#4C0519", 35)),
        panel: None,
    },
];

pub fn apply(f: &mut Form, preset: &Preset) {
    for row in &mut f.items {
        row.color = preset
            .items
            .iter()
            .find(|(kind, _)| *kind == row.kind)
            .map(|(_, hex)| hex.to_string())
            .unwrap_or_default();
    }
    f.label_color = preset.label.unwrap_or_default().to_string();
    f.value_color = preset.value.unwrap_or_default().to_string();
    f.color_labels = preset.color_labels;
    match preset.tile {
        Some((hex, opacity)) => {
            f.tile_color = hex.to_string();
            f.tile_opacity_custom = true;
            f.tile_opacity = i32::from(opacity);
        }
        None => {
            f.tile_color.clear();
            f.tile_opacity_custom = false;
        }
    }
    match preset.panel {
        Some((hex, opacity)) => {
            f.panel_color = hex.to_string();
            f.panel_opacity = i32::from(opacity);
        }
        None => {
            f.panel_color.clear();
            f.panel_opacity = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::to_form;
    use pulsar_core::config::Config;

    #[test]
    fn presets_have_unique_names_and_valid_colours() {
        let mut names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PRESETS.len());
        for p in &PRESETS {
            let hexes = p
                .items
                .iter()
                .map(|(_, h)| *h)
                .chain(p.label)
                .chain(p.value)
                .chain(p.tile.map(|t| t.0))
                .chain(p.panel.map(|t| t.0));
            for hex in hexes {
                assert!(crate::form::parse_hex(hex).is_some(), "{} {hex}", p.name);
            }
        }
    }

    #[test]
    fn applying_a_preset_sets_colours_and_the_default_clears_them() {
        let mut f = to_form(&Config::default());
        apply(&mut f, &PRESETS[1]);
        assert!(f.items.iter().all(|r| !r.color.is_empty()));
        assert!(!f.label_color.is_empty());
        apply(&mut f, &PRESETS[0]);
        assert!(f.items.iter().all(|r| r.color.is_empty()));
        assert_eq!(f.label_color, "");
        assert_eq!(f.panel_opacity, 0);
        assert!(!f.tile_opacity_custom);
    }
}
