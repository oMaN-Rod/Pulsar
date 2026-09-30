use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::metric::ItemKind;

pub const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Graph,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub display: Display,
    pub items: Vec<ItemConfig>,
    pub ping: PingConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    pub update_check: bool,
    pub sample_interval_ms: u32,
    pub history_len: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub mode: DisplayMode,
    pub hover_popup: bool,
    pub position: Position,
    pub offset_px: i32,
    pub fallback_margin_px: i32,
    pub font_size_pt: f32,
    pub show_on_all_taskbars: bool,
    pub accent_graphs: bool,
    /// `#RRGGBB` behind each graph tile; `None` uses the theme tint.
    pub tile_color: Option<String>,
    /// 0–100; `None` uses the theme default, 0 removes the tiles.
    pub tile_opacity: Option<u8>,
    /// `#RRGGBB` for a panel behind the whole overlay; `None` uses the theme.
    pub panel_color: Option<String>,
    /// 0–100; 0 (the default) draws no panel.
    pub panel_opacity: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemConfig {
    pub kind: ItemKind,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// `#RRGGBB`; `None` uses the theme default.
    #[serde(default)]
    pub color: Option<String>,
}

fn enabled_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PingConfig {
    pub host: String,
    pub interval_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            general: General::default(),
            display: Display::default(),
            items: ItemKind::ALL
                .iter()
                .map(|&kind| ItemConfig {
                    kind,
                    enabled: kind != ItemKind::Ping,
                    color: None,
                })
                .collect(),
            ping: PingConfig::default(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: false,
            update_check: true,
            sample_interval_ms: 1000,
            history_len: 60,
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Graph,
            hover_popup: true,
            position: Position::Right,
            offset_px: 0,
            fallback_margin_px: 160,
            font_size_pt: 9.0,
            show_on_all_taskbars: false,
            accent_graphs: false,
            tile_color: None,
            tile_opacity: None,
            panel_color: None,
            panel_opacity: 0,
        }
    }
}

impl Default for PingConfig {
    fn default() -> Self {
        Self {
            host: "1.1.1.1".to_string(),
            interval_ms: 2000,
        }
    }
}

impl Config {
    /// Clamps numeric ranges and repairs the item list so every `ItemKind`
    /// appears exactly once, keeping the user's order.
    pub fn sanitize(mut self) -> Self {
        self.version = CURRENT_VERSION;
        let g = &mut self.general;
        g.sample_interval_ms = g.sample_interval_ms.clamp(500, 5000);
        g.history_len = g.history_len.clamp(10, 600);
        let d = &mut self.display;
        d.font_size_pt = if d.font_size_pt.is_finite() {
            d.font_size_pt.clamp(6.0, 20.0)
        } else {
            9.0
        };
        d.fallback_margin_px = d.fallback_margin_px.clamp(0, 2000);
        d.offset_px = d.offset_px.clamp(-2000, 2000);
        d.tile_opacity = d.tile_opacity.map(|o| o.min(100));
        d.panel_opacity = d.panel_opacity.min(100);
        for color in [&mut d.tile_color, &mut d.panel_color] {
            if color.as_deref().is_some_and(|c| !is_hex_color(c)) {
                *color = None;
            }
        }
        self.ping.interval_ms = self.ping.interval_ms.clamp(1000, 60_000);
        if self.ping.host.trim().is_empty() {
            self.ping.host = PingConfig::default().host;
        }

        let mut seen = Vec::new();
        self.items.retain(|item| {
            let first = !seen.contains(&item.kind);
            seen.push(item.kind);
            first
        });
        for kind in ItemKind::ALL {
            if !seen.contains(&kind) {
                self.items.push(ItemConfig {
                    kind,
                    enabled: false,
                    color: None,
                });
            }
        }
        for item in &mut self.items {
            if item.color.as_deref().is_some_and(|c| !is_hex_color(c)) {
                item.color = None;
            }
        }
        self
    }

    pub fn enabled_items(&self) -> Vec<ItemKind> {
        self.items
            .iter()
            .filter(|i| i.enabled)
            .map(|i| i.kind)
            .collect()
    }

    pub fn is_enabled(&self, kind: ItemKind) -> bool {
        self.items.iter().any(|i| i.kind == kind && i.enabled)
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether moving from `old` to `new` needs a new sampler: the set of
/// sources or the interval changed, or the ping target changed while ping is on.
pub fn sampling_changed(old: &Config, new: &Config) -> bool {
    let mut a = old.enabled_items();
    let mut b = new.enabled_items();
    a.sort_by_key(|k| *k as u8);
    b.sort_by_key(|k| *k as u8);
    let pinging = new.is_enabled(ItemKind::Ping);
    a != b
        || old.general.sample_interval_ms != new.general.sample_interval_ms
        || (pinging
            && (old.ping.host != new.ping.host || old.ping.interval_ms != new.ping.interval_ms))
}

/// `%APPDATA%\Pulsar\config.toml`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Pulsar").join("config.toml"))
}

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    /// Set when the file existed but could not be used; it was moved to `.bak`.
    pub warning: Option<String>,
}

/// Reads and sanitises the file without ever moving it; for callers that
/// poll a file another process owns and may be half-way through writing.
pub fn read(path: &Path) -> io::Result<Config> {
    let text = fs::read_to_string(path)?;
    toml::from_str::<Config>(&text)
        .map(Config::sanitize)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn load(path: &Path) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Loaded {
                config: Config::default(),
                warning: None,
            };
        }
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            return back_up_invalid(path, "the file is not UTF-8 text");
        }
        Err(e) => {
            return Loaded {
                config: Config::default(),
                warning: Some(format!("Could not read {}: {e}", path.display())),
            };
        }
    };
    match toml::from_str::<Config>(&text) {
        Ok(config) => Loaded {
            config: config.sanitize(),
            warning: None,
        },
        Err(e) => back_up_invalid(path, &e.to_string()),
    }
}

fn back_up_invalid(path: &Path, reason: &str) -> Loaded {
    let backup = path.with_extension("toml.bak");
    let moved = fs::rename(path, &backup).is_ok();
    let where_ = if moved {
        format!(" It was saved as {}.", backup.display())
    } else {
        String::new()
    };
    Loaded {
        config: Config::default(),
        warning: Some(format!(
            "Settings file was invalid and defaults were used.{where_}\n{reason}"
        )),
    }
}

/// Writes atomically: a temp file in the same directory, then rename.
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Pulsar").join("config.toml");
        save(&path, &Config::default()).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn read_leaves_an_invalid_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = = half written").unwrap();
        assert!(read(&path).is_err());
        assert!(path.exists(), "never renamed or backed up");
        assert!(!dir.path().join("config.toml.bak").exists());
    }

    #[test]
    fn read_returns_a_sanitised_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[general]
sample_interval_ms = 1
",
        )
        .unwrap();
        assert_eq!(read(&path).unwrap().general.sample_interval_ms, 500);
    }

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("nope.toml"));
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn non_utf8_file_is_backed_up_and_defaults_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "[display]\nmode = \"text\"\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        fs::write(&path, utf16).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warning.is_some());
        assert!(
            !path.exists(),
            "a later save must not overwrite the user's file"
        );
        assert!(dir.path().join("config.toml.bak").exists());
    }

    #[test]
    fn invalid_file_is_backed_up_and_defaults_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is = = not toml").unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warning.is_some());
        assert!(!path.exists());
        assert!(dir.path().join("config.toml.bak").exists());
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[display]\nmode = \"text\"\n").unwrap();
        let config = load(&path).config;
        assert_eq!(config.display.mode, DisplayMode::Text);
        assert!(config.display.hover_popup);
        assert_eq!(config.general.sample_interval_ms, 1000);
        assert_eq!(config.items.len(), ItemKind::ALL.len());
    }

    #[test]
    fn sanitize_clamps_ranges() {
        let mut c = Config::default();
        c.general.sample_interval_ms = 1;
        c.general.history_len = 1_000_000;
        c.display.font_size_pt = f32::NAN;
        c.ping.host = "   ".into();
        let c = c.sanitize();
        assert_eq!(c.general.sample_interval_ms, 500);
        assert_eq!(c.general.history_len, 600);
        assert_eq!(c.display.font_size_pt, 9.0);
        assert_eq!(c.ping.host, "1.1.1.1");
    }

    #[test]
    fn sanitize_repairs_items_keeping_order() {
        let c = Config {
            items: vec![
                ItemConfig {
                    kind: ItemKind::Gpu,
                    enabled: true,
                    color: Some("#00FF00".into()),
                },
                ItemConfig {
                    kind: ItemKind::Gpu,
                    enabled: false,
                    color: None,
                },
                ItemConfig {
                    kind: ItemKind::Cpu,
                    enabled: true,
                    color: Some("red".into()),
                },
            ],
            ..Config::default()
        }
        .sanitize();
        let kinds: Vec<ItemKind> = c.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds[..2], [ItemKind::Gpu, ItemKind::Cpu]);
        assert_eq!(kinds.len(), ItemKind::ALL.len());
        assert!(c.items[0].enabled, "first duplicate wins");
        assert_eq!(c.items[0].color.as_deref(), Some("#00FF00"));
        assert_eq!(c.items[1].color, None, "invalid colour dropped");
        assert!(!c.items[2].enabled, "items added by repair start disabled");
    }

    #[test]
    fn default_order_pairs_items_in_text_columns() {
        let kinds: Vec<ItemKind> = Config::default().items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                ItemKind::Cpu,
                ItemKind::Ram,
                ItemKind::Gpu,
                ItemKind::Disk,
                ItemKind::Network,
                ItemKind::Ping
            ]
        );
    }

    #[test]
    fn background_defaults_keep_theme_tiles_and_no_panel() {
        let d = Config::default().display;
        assert_eq!(d.tile_color, None);
        assert_eq!(d.tile_opacity, None);
        assert_eq!(d.panel_color, None);
        assert_eq!(d.panel_opacity, 0);
    }

    #[test]
    fn background_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[display]
tile_color = \"#101010\"
tile_opacity = 0
panel_color = \"#202020\"
panel_opacity = 60
",
        )
        .unwrap();
        let d = load(&path).config.display;
        assert_eq!(d.tile_color.as_deref(), Some("#101010"));
        assert_eq!(d.tile_opacity, Some(0));
        assert_eq!(d.panel_color.as_deref(), Some("#202020"));
        assert_eq!(d.panel_opacity, 60);
    }

    #[test]
    fn sanitize_clamps_opacity_and_drops_bad_background_colours() {
        let mut c = Config::default();
        c.display.tile_color = Some("grey".into());
        c.display.tile_opacity = Some(250);
        c.display.panel_color = Some("#12345".into());
        c.display.panel_opacity = 101;
        let d = c.sanitize().display;
        assert_eq!(d.tile_color, None);
        assert_eq!(d.tile_opacity, Some(100));
        assert_eq!(d.panel_color, None);
        assert_eq!(d.panel_opacity, 100);
    }

    #[test]
    fn sampling_changes_only_for_sources_interval_or_ping() {
        let base = Config::default();
        let mut reordered = base.clone();
        reordered.items.swap(0, 1);
        reordered.display.mode = DisplayMode::Text;
        reordered.display.font_size_pt = 12.0;
        assert!(
            !sampling_changed(&base, &reordered),
            "order and display settings keep the sampler"
        );

        let mut ping_on = base.clone();
        ping_on.items.iter_mut().for_each(|i| i.enabled = true);
        assert!(sampling_changed(&base, &ping_on));

        let mut slower = base.clone();
        slower.general.sample_interval_ms = 2000;
        assert!(sampling_changed(&base, &slower));

        let mut host = ping_on.clone();
        host.ping.host = "8.8.8.8".into();
        assert!(
            sampling_changed(&ping_on, &host),
            "ping target changed while pinging"
        );
    }

    #[test]
    fn ping_settings_do_not_matter_while_ping_is_off() {
        let base = Config::default();
        let mut typed = base.clone();
        typed.ping.host = "google.c".into();
        typed.ping.interval_ms = 5000;
        assert!(
            !sampling_changed(&base, &typed),
            "typing a host with ping disabled must not restart sampling"
        );
    }

    #[test]
    fn ping_is_disabled_by_default() {
        assert!(!Config::default().is_enabled(ItemKind::Ping));
        assert!(Config::default().is_enabled(ItemKind::Cpu));
    }
}
