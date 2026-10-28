use pulsar_core::colors;
use pulsar_core::config::{ColorMode, Config};
use pulsar_core::display::metric_for;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::ItemKind;
pub use pulsar_core::theme::Theme;
use windows::Win32::Graphics::Gdi::{
    COLOR_HIGHLIGHT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

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
    /// Popup text on the theme's popup background; never overridden.
    pub popup_text: Color,
    pub popup_label: Color,
    pub tile: Color,
    /// Near-transparent fill so the whole overlay receives mouse input.
    pub hit: Color,
    /// Optional panel behind the whole overlay.
    pub panel: Option<Color>,
    /// Hover popup background, its 1 px border, and the failed-source message.
    pub popup: Color,
    pub popup_border: Color,
    pub warning: Color,
    items: Vec<(ItemKind, CellPart, Color)>,
    /// Items coloured by how high their value is.
    by_load: Vec<ItemKind>,
    light: bool,
    color_labels: bool,
}

impl Palette {
    /// The palette for the current Windows settings and `config`'s theme.
    /// High contrast replaces every colour with the system's.
    pub fn current(config: &Config) -> Self {
        if let Some(p) = Self::high_contrast() {
            return p;
        }
        let theme = pulsar_core::theme::load(
            pulsar_core::theme::themes_dir().as_deref(),
            config.display.theme.as_deref(),
        );
        Self::new(taskbar_is_light(), config, accent_color(), &theme)
    }

    fn high_contrast() -> Option<Self> {
        let mut hc = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                Some((&raw mut hc).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        if !hc.dwFlags.contains(HCF_HIGHCONTRASTON) {
            return None;
        }
        let sys = |index| Color::from_abgr(unsafe { GetSysColor(index) });
        let (text, back, highlight) = (
            sys(COLOR_WINDOWTEXT),
            sys(COLOR_WINDOW),
            sys(COLOR_HIGHLIGHT),
        );
        let items = ItemKind::ALL
            .into_iter()
            .flat_map(|k| [(k, CellPart::Main), (k, CellPart::Down), (k, CellPart::Up)])
            .map(|(k, p)| (k, p, highlight))
            .collect();
        Some(Self {
            text,
            label: text,
            popup_text: text,
            popup_label: text,
            tile: text.with_alpha(0.0),
            hit: back.with_alpha(1.0 / 255.0),
            panel: Some(back),
            popup: back,
            popup_border: text,
            warning: sys(COLOR_HOTLIGHT),
            items,
            by_load: Vec::new(),
            light: back.r + back.g + back.b > 1.5,
            color_labels: false,
        })
    }

    pub fn new(light_taskbar: bool, config: &Config, accent: Option<Color>, theme: &Theme) -> Self {
        let theme = theme.colors(light_taskbar);
        let (text, label, theme_panel) = (rgb(theme.text), rgb(theme.label), rgb(theme.panel));
        let opacity = |percent: u8| f32::from(percent.min(100)) / 100.0;
        let theme_tile = rgb(theme.tile).with_alpha(opacity(theme.tile_opacity));
        let d = &config.display;
        let (popup_text, popup_label) = (text, label);
        let custom = |hex: &Option<String>| hex.as_deref().and_then(Color::parse_hex);
        let themed = |dark: &Option<String>, light: &Option<String>| {
            light_taskbar
                .then(|| custom(light))
                .flatten()
                .or_else(|| custom(dark))
        };
        let text = themed(&d.value_color, &d.value_color_light).unwrap_or(text);
        let label = themed(&d.label_color, &d.label_color_light).unwrap_or(label);
        let tile = d
            .tile_color
            .as_deref()
            .and_then(Color::parse_hex)
            .unwrap_or(theme_tile)
            .with_alpha(d.tile_opacity.map_or(theme_tile.a, opacity));
        let panel_opacity = d.panel_opacity.unwrap_or(theme.panel_opacity);
        let panel = (panel_opacity > 0).then(|| {
            d.panel_color
                .as_deref()
                .and_then(Color::parse_hex)
                .unwrap_or(theme_panel)
                .with_alpha(opacity(panel_opacity))
        });
        let mut items = Vec::new();
        let mut by_load = Vec::new();
        for kind in ItemKind::ALL {
            let item = config.item(kind);
            let mode = item.map_or(ColorMode::Item, |i| i.color_mode);
            if mode == ColorMode::Load {
                by_load.push(kind);
            }
            let custom = item
                .and_then(|i| i.color.as_deref())
                .and_then(Color::parse_hex);
            let parts: &[CellPart] = if kind == ItemKind::Network {
                &[CellPart::Down, CellPart::Up]
            } else {
                &[CellPart::Main]
            };
            for &part in parts {
                let color = match mode {
                    ColorMode::Accent => accent.or(custom),
                    _ => custom.or(if config.display.accent_graphs {
                        accent
                    } else {
                        None
                    }),
                }
                .unwrap_or_else(|| rgb(theme.item(kind, part)));
                items.push((kind, part, color));
            }
        }
        Self {
            text,
            label,
            popup_text,
            popup_label,
            panel,
            popup: theme_panel.with_alpha(0.98),
            popup_border: if light_taskbar {
                Color::rgb(0, 0, 0).with_alpha(0.12)
            } else {
                Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(0.12)
            },
            warning: if light_taskbar {
                Color::rgb(0x9D, 0x5D, 0x00)
            } else {
                Color::rgb(0xFF, 0xB9, 0x00)
            },
            tile,
            hit: Color::rgb(0, 0, 0).with_alpha(1.0 / 255.0),
            items,
            by_load,
            light: light_taskbar,
            color_labels: d.color_labels,
        }
    }

    /// Value text: the load colour for an item coloured by load, so items
    /// without a graph show their load too; otherwise the value colour.
    pub fn value_at(&self, kind: ItemKind, part: CellPart, value: Option<f64>) -> Color {
        let level = value
            .filter(|_| self.by_load.contains(&kind))
            .and_then(|v| colors::load_level(metric_for(kind, part).unit(), v));
        level.map_or(self.text, |level| rgb(colors::load_rgb(level, self.light)))
    }

    pub fn light(&self) -> bool {
        self.light
    }

    /// The colour for a reading of `value`: its load colour when the item
    /// is coloured by load and the value has a scale, else `graph`.
    pub fn graph_at(&self, kind: ItemKind, part: CellPart, value: Option<f64>) -> Color {
        let level = value
            .filter(|_| self.by_load.contains(&kind))
            .and_then(|v| colors::load_level(metric_for(kind, part).unit(), v));
        match level {
            Some(level) => rgb(colors::load_rgb(level, self.light)),
            None => self.graph(kind, part),
        }
    }

    pub fn graph(&self, kind: ItemKind, part: CellPart) -> Color {
        let part = match part {
            CellPart::DriveActive(_) | CellPart::DriveUsed(_) => CellPart::Main,
            p => p,
        };
        self.items
            .iter()
            .find(|(k, p, _)| *k == kind && *p == part)
            .map(|(_, _, c)| *c)
            .unwrap_or(self.text)
    }

    /// Network arrows always take their series colour; other labels do when
    /// `color_labels` is on.
    pub fn label_for(&self, kind: ItemKind, part: CellPart) -> Color {
        if self.color_labels || kind == ItemKind::Network {
            self.graph(kind, part)
        } else {
            self.label
        }
    }
}

fn rgb((r, g, b): colors::Rgb) -> Color {
    Color::rgb(r, g, b)
}

#[cfg(test)]
fn default_color(kind: ItemKind, part: CellPart) -> Color {
    rgb(colors::default_rgb(kind, part))
}

pub use pulsar_core::system_theme::taskbar_is_light;

pub fn accent_color() -> Option<Color> {
    pulsar_core::system_theme::accent_abgr().map(Color::from_abgr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::config::ItemConfig;

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
            Palette::new(false, &config, None, &Theme::windows()).text,
            Color::rgb(255, 255, 255)
        );
        assert_eq!(
            Palette::new(true, &config, None, &Theme::windows()).text,
            Color::rgb(0x1A, 0x1A, 0x1A)
        );
    }

    #[test]
    fn custom_colour_beats_accent_beats_default() {
        let accent = Color::rgb(1, 2, 3);
        let mut config = Config::default();
        config.display.accent_graphs = true;
        config.items = vec![ItemConfig {
            color: Some("#102030".into()),
            ..ItemConfig::new(ItemKind::Cpu, true)
        }];
        let p = Palette::new(false, &config, Some(accent), &Theme::windows());
        assert_eq!(
            p.graph(ItemKind::Cpu, CellPart::Main),
            Color::rgb(0x10, 0x20, 0x30)
        );
        assert_eq!(p.graph(ItemKind::Gpu, CellPart::Main), accent);

        config.display.accent_graphs = false;
        let p = Palette::new(false, &config, Some(accent), &Theme::windows());
        assert_eq!(
            p.graph(ItemKind::Gpu, CellPart::Main),
            default_color(ItemKind::Gpu, CellPart::Main)
        );
    }

    #[test]
    fn tile_defaults_to_theme_tint_and_no_panel() {
        let p = Palette::new(false, &Config::default(), None, &Theme::windows());
        assert_eq!(p.tile, Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(0.08));
        assert_eq!(p.panel, None);
    }

    #[test]
    fn tile_colour_and_opacity_come_from_config() {
        let mut config = Config::default();
        config.display.tile_color = Some("#102030".into());
        config.display.tile_opacity = Some(50);
        let p = Palette::new(false, &config, None, &Theme::windows());
        assert_eq!(p.tile, Color::rgb(0x10, 0x20, 0x30).with_alpha(0.5));

        config.display.tile_color = None;
        config.display.tile_opacity = Some(0);
        assert_eq!(
            Palette::new(true, &config, None, &Theme::windows()).tile.a,
            0.0
        );
    }

    #[test]
    fn panel_appears_only_with_opacity() {
        let mut config = Config::default();
        config.display.panel_color = Some("#202020".into());
        assert_eq!(
            Palette::new(false, &config, None, &Theme::windows()).panel,
            None
        );
        config.display.panel_opacity = Some(60);
        assert_eq!(
            Palette::new(false, &config, None, &Theme::windows()).panel,
            Some(Color::rgb(0x20, 0x20, 0x20).with_alpha(0.6))
        );
        config.display.panel_color = None;
        assert_eq!(
            Palette::new(true, &config, None, &Theme::windows()).panel,
            Some(Color::rgb(0xF3, 0xF3, 0xF3).with_alpha(0.6)),
            "theme panel colour on a light taskbar"
        );
    }

    #[test]
    fn popup_follows_the_theme() {
        let config = Config::default();
        let dark = Palette::new(false, &config, None, &Theme::windows());
        assert_eq!(dark.popup, Color::rgb(0x20, 0x20, 0x20).with_alpha(0.98));
        let light = Palette::new(true, &config, None, &Theme::windows());
        assert_eq!(light.popup, Color::rgb(0xF3, 0xF3, 0xF3).with_alpha(0.98));
        assert_ne!(dark.warning, light.warning);
    }

    #[test]
    fn network_parts_have_distinct_defaults() {
        let p = Palette::new(false, &Config::default(), None, &Theme::windows());
        assert_ne!(
            p.graph(ItemKind::Network, CellPart::Down),
            p.graph(ItemKind::Network, CellPart::Up)
        );
    }

    #[test]
    fn label_and_value_colours_override_the_theme() {
        let mut c = Config::default();
        c.display.label_color = Some("#112233".into());
        c.display.value_color = Some("#445566".into());
        let p = Palette::new(false, &c, None, &Theme::windows());
        assert_eq!(p.label, Color::rgb(0x11, 0x22, 0x33));
        assert_eq!(p.text, Color::rgb(0x44, 0x55, 0x66));
    }

    #[test]
    fn light_taskbar_colours_override_only_on_a_light_taskbar() {
        let mut c = Config::default();
        c.display.label_color = Some("#EEEEEE".into());
        c.display.label_color_light = Some("#111111".into());
        assert_eq!(
            Palette::new(false, &c, None, &Theme::windows()).label,
            Color::rgb(0xEE, 0xEE, 0xEE)
        );
        assert_eq!(
            Palette::new(true, &c, None, &Theme::windows()).label,
            Color::rgb(0x11, 0x11, 0x11)
        );
        c.display.label_color_light = None;
        assert_eq!(
            Palette::new(true, &c, None, &Theme::windows()).label,
            Color::rgb(0xEE, 0xEE, 0xEE),
            "without a light colour the one colour applies to both"
        );
    }

    #[test]
    fn items_can_follow_the_accent_or_their_load() {
        let accent = Color::rgb(1, 2, 3);
        let mut c = Config::default();
        c.item_mut(ItemKind::Ram).unwrap().color_mode = ColorMode::Accent;
        c.item_mut(ItemKind::Cpu).unwrap().color_mode = ColorMode::Load;
        let p = Palette::new(false, &c, Some(accent), &Theme::windows());
        assert_eq!(p.graph(ItemKind::Ram, CellPart::Main), accent);
        assert_eq!(
            p.graph(ItemKind::Gpu, CellPart::Main),
            default_color(ItemKind::Gpu, CellPart::Main)
        );
        let cpu = |v| p.graph_at(ItemKind::Cpu, CellPart::Main, Some(v));
        assert_eq!(cpu(10.0), rgb(colors::load_rgb(colors::Load::Low, false)));
        assert_eq!(
            cpu(95.0),
            rgb(colors::load_rgb(colors::Load::Critical, false))
        );
        assert_eq!(
            p.graph_at(ItemKind::Cpu, CellPart::Main, None),
            p.graph(ItemKind::Cpu, CellPart::Main),
            "no reading keeps the item colour"
        );
        assert_eq!(
            p.graph_at(ItemKind::Ram, CellPart::Main, Some(95.0)),
            accent,
            "only items set to load change colour"
        );
        assert_eq!(
            p.value_at(ItemKind::Cpu, CellPart::Main, Some(95.0)),
            rgb(colors::load_rgb(colors::Load::Critical, false))
        );
        assert_eq!(
            p.value_at(ItemKind::Ram, CellPart::Main, Some(95.0)),
            p.text
        );
    }

    #[test]
    fn labels_can_take_the_item_colour() {
        let mut c = Config::default();
        let plain = Palette::new(false, &c, None, &Theme::windows());
        assert_eq!(plain.label_for(ItemKind::Cpu, CellPart::Main), plain.label);
        c.display.color_labels = true;
        let coloured = Palette::new(false, &c, None, &Theme::windows());
        assert_eq!(
            coloured.label_for(ItemKind::Cpu, CellPart::Main),
            default_color(ItemKind::Cpu, CellPart::Main)
        );
    }

    #[test]
    fn drive_cells_use_the_disk_colour() {
        let p = Palette::new(false, &Config::default(), None, &Theme::windows());
        assert_eq!(
            p.graph(ItemKind::Disk, CellPart::DriveUsed(b'D')),
            p.graph(ItemKind::Disk, CellPart::Main)
        );
    }

    #[test]
    fn popups_keep_theme_text_whatever_the_overlay_colours() {
        let plain = Palette::new(true, &Config::default(), None, &Theme::windows());
        let mut c = Config::default();
        c.display.label_color = Some("#F8FAFC".into());
        c.display.value_color = Some("#FFFFFF".into());
        let custom = Palette::new(true, &c, None, &Theme::windows());
        assert_eq!(custom.popup_text, plain.text);
        assert_eq!(custom.popup_label, plain.label);
    }
}
