use perfbar_core::config::Config;
use perfbar_core::layout::CellPart;
use perfbar_core::metric::ItemKind;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// `#RRGGBB`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#').filter(|h| h.len() == 6)?;
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    /// DWM stores the accent colour as `0xAABBGGRR`.
    pub fn from_abgr(value: u32) -> Self {
        Self::rgb(value as u8, (value >> 8) as u8, (value >> 16) as u8)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub text: Color,
    pub label: Color,
    pub tile: Color,
    /// Near-transparent fill so the whole overlay receives mouse input.
    pub hit: Color,
    /// Optional panel behind the whole overlay.
    pub panel: Option<Color>,
    items: Vec<(ItemKind, CellPart, Color)>,
}

impl Palette {
    pub fn new(light_taskbar: bool, config: &Config, accent: Option<Color>) -> Self {
        let (text, label, theme_tile, theme_panel) = if light_taskbar {
            (
                Color::rgb(0x1A, 0x1A, 0x1A),
                Color::rgb(0x5C, 0x5C, 0x5C),
                Color::rgb(0, 0, 0).with_alpha(0.06),
                Color::rgb(0xF3, 0xF3, 0xF3),
            )
        } else {
            (
                Color::rgb(0xFF, 0xFF, 0xFF),
                Color::rgb(0xB8, 0xB8, 0xB8),
                Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(0.08),
                Color::rgb(0x20, 0x20, 0x20),
            )
        };
        let d = &config.display;
        let opacity = |percent: u8| f32::from(percent.min(100)) / 100.0;
        let tile = d
            .tile_color
            .as_deref()
            .and_then(Color::parse_hex)
            .unwrap_or(theme_tile)
            .with_alpha(d.tile_opacity.map_or(theme_tile.a, opacity));
        let panel = (d.panel_opacity > 0).then(|| {
            d.panel_color
                .as_deref()
                .and_then(Color::parse_hex)
                .unwrap_or(theme_panel)
                .with_alpha(opacity(d.panel_opacity))
        });
        let mut items = Vec::new();
        for kind in ItemKind::ALL {
            let custom = config
                .items
                .iter()
                .find(|i| i.kind == kind)
                .and_then(|i| i.color.as_deref())
                .and_then(Color::parse_hex);
            let parts: &[CellPart] = if kind == ItemKind::Network {
                &[CellPart::Down, CellPart::Up]
            } else {
                &[CellPart::Main]
            };
            for &part in parts {
                let color = custom
                    .or(if config.display.accent_graphs {
                        accent
                    } else {
                        None
                    })
                    .unwrap_or_else(|| default_color(kind, part));
                items.push((kind, part, color));
            }
        }
        Self {
            text,
            label,
            panel,
            tile,
            hit: Color::rgb(0, 0, 0).with_alpha(1.0 / 255.0),
            items,
        }
    }

    pub fn graph(&self, kind: ItemKind, part: CellPart) -> Color {
        self.items
            .iter()
            .find(|(k, p, _)| *k == kind && *p == part)
            .map(|(_, _, c)| *c)
            .unwrap_or(self.text)
    }
}

pub fn default_color(kind: ItemKind, part: CellPart) -> Color {
    match (kind, part) {
        (ItemKind::Cpu, _) => Color::rgb(0x4C, 0xC2, 0xFF),
        (ItemKind::Ram, _) => Color::rgb(0xB1, 0x86, 0xF6),
        (ItemKind::Disk, _) => Color::rgb(0x6C, 0xCB, 0x5F),
        (ItemKind::Network, CellPart::Up) => Color::rgb(0xF7, 0x63, 0x0C),
        (ItemKind::Network, _) => Color::rgb(0xFF, 0xB9, 0x00),
        (ItemKind::Gpu, _) => Color::rgb(0xFF, 0x6F, 0xB5),
        (ItemKind::Ping, _) => Color::rgb(0x2E, 0xD5, 0xC4),
    }
}

fn read_dword(subkey: windows::core::PCWSTR, value: windows::core::PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    Some(data)
}

/// The taskbar follows the "Windows mode" setting, not the app mode.
pub fn taskbar_is_light() -> bool {
    read_dword(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|v| v != 0)
}

pub fn accent_color() -> Option<Color> {
    read_dword(w!(r"Software\Microsoft\Windows\DWM"), w!("AccentColor")).map(Color::from_abgr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfbar_core::config::ItemConfig;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Color::parse_hex("#FF8000"), Some(Color::rgb(255, 128, 0)));
        assert_eq!(Color::parse_hex("#ff8000"), Some(Color::rgb(255, 128, 0)));
        assert_eq!(Color::parse_hex("FF8000"), None);
        assert_eq!(Color::parse_hex("#FF80"), None);
        assert_eq!(Color::parse_hex("#GG8000"), None);
    }

    #[test]
    fn abgr_is_decoded_in_byte_order() {
        assert_eq!(Color::from_abgr(0xFF00_80FF), Color::rgb(0xFF, 0x80, 0x00));
    }

    #[test]
    fn text_contrasts_with_taskbar() {
        let config = Config::default();
        assert_eq!(
            Palette::new(false, &config, None).text,
            Color::rgb(255, 255, 255)
        );
        assert_eq!(
            Palette::new(true, &config, None).text,
            Color::rgb(0x1A, 0x1A, 0x1A)
        );
    }

    #[test]
    fn custom_colour_beats_accent_beats_default() {
        let accent = Color::rgb(1, 2, 3);
        let mut config = Config::default();
        config.display.accent_graphs = true;
        config.items = vec![ItemConfig {
            kind: ItemKind::Cpu,
            enabled: true,
            color: Some("#102030".into()),
        }];
        let p = Palette::new(false, &config, Some(accent));
        assert_eq!(
            p.graph(ItemKind::Cpu, CellPart::Main),
            Color::rgb(0x10, 0x20, 0x30)
        );
        assert_eq!(p.graph(ItemKind::Gpu, CellPart::Main), accent);

        config.display.accent_graphs = false;
        let p = Palette::new(false, &config, Some(accent));
        assert_eq!(
            p.graph(ItemKind::Gpu, CellPart::Main),
            default_color(ItemKind::Gpu, CellPart::Main)
        );
    }

    #[test]
    fn tile_defaults_to_theme_tint_and_no_panel() {
        let p = Palette::new(false, &Config::default(), None);
        assert_eq!(p.tile, Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(0.08));
        assert_eq!(p.panel, None);
    }

    #[test]
    fn tile_colour_and_opacity_come_from_config() {
        let mut config = Config::default();
        config.display.tile_color = Some("#102030".into());
        config.display.tile_opacity = Some(50);
        let p = Palette::new(false, &config, None);
        assert_eq!(p.tile, Color::rgb(0x10, 0x20, 0x30).with_alpha(0.5));

        config.display.tile_color = None;
        config.display.tile_opacity = Some(0);
        assert_eq!(Palette::new(true, &config, None).tile.a, 0.0);
    }

    #[test]
    fn panel_appears_only_with_opacity() {
        let mut config = Config::default();
        config.display.panel_color = Some("#202020".into());
        assert_eq!(Palette::new(false, &config, None).panel, None);
        config.display.panel_opacity = 60;
        assert_eq!(
            Palette::new(false, &config, None).panel,
            Some(Color::rgb(0x20, 0x20, 0x20).with_alpha(0.6))
        );
        config.display.panel_color = None;
        assert_eq!(
            Palette::new(true, &config, None).panel,
            Some(Color::rgb(0xF3, 0xF3, 0xF3).with_alpha(0.6)),
            "theme panel colour on a light taskbar"
        );
    }

    #[test]
    fn network_parts_have_distinct_defaults() {
        let p = Palette::new(false, &Config::default(), None);
        assert_ne!(
            p.graph(ItemKind::Network, CellPart::Down),
            p.graph(ItemKind::Network, CellPart::Up)
        );
    }

    #[test]
    fn reads_theme_from_registry_without_panicking() {
        let _ = taskbar_is_light();
        let _ = accent_color();
    }
}
