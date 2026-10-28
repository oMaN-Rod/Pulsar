use std::fmt;
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

/// How an item's history is drawn behind its text in graph mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphStyle {
    #[default]
    Area,
    Line,
    Bars,
    /// A level meter for the latest value instead of the history.
    Meter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// The item's own colour, or the accent with `accent_graphs`.
    #[default]
    Item,
    Accent,
    /// Green, yellow, orange or red by how high the value is.
    Load,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupDetail {
    /// The value and its graph.
    Simple,
    #[default]
    Standard,
    /// Everything Pulsar reads for the item.
    Detailed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupBackground {
    Solid,
    /// The blurred system material behind flyouts; solid where Windows
    /// does not offer it.
    #[default]
    Translucent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateUnit {
    /// Bytes per second, scaled by 1024.
    #[default]
    Bytes,
    /// Bits per second, scaled by 1000.
    Bits,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub temperature: TemperatureUnit,
    /// Network rates only; disk rates stay in bytes.
    pub network: RateUnit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupConfig {
    pub detail: PopupDetail,
    pub background: PopupBackground,
    pub graph: bool,
    /// Rows in top-process lists; 0 leaves the lists out.
    pub processes: u8,
}

pub const MAX_POPUP_PROCESSES: u8 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Left,
    Right,
    Floating,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub display: Display,
    pub items: Vec<ItemConfig>,
    pub ping: PingConfig,
    pub disk: DiskConfig,
    pub network: NetworkConfig,
    pub units: Units,
    pub popup: PopupConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    pub update_check: bool,
    pub sample_interval_ms: u32,
    pub history_len: usize,
    /// Off keeps Pulsar out of the notification area except while a
    /// notification is showing.
    pub tray_icon: bool,
    /// A code from `l10n::LANGUAGES`; `None` follows Windows.
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub mode: DisplayMode,
    /// Graph mode: label and value on one line instead of stacked.
    pub graph_inline: bool,
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
    pub labels: LabelStyle,
    /// An icon before each label; with `LabelStyle::None` it is the only label.
    pub icons: bool,
    /// `#RRGGBB` overrides for label and value text; `None` follows the theme.
    pub label_color: Option<String>,
    pub value_color: Option<String>,
    /// Overrides for a light taskbar; `None` uses the colours above.
    pub label_color_light: Option<String>,
    pub value_color_light: Option<String>,
    /// Network shows the upload rate before the download rate.
    pub upload_on_top: bool,
    /// Draw each label in its item's colour.
    pub color_labels: bool,
    /// None uses Segoe UI Variable.
    pub font_family: Option<String>,
    pub font_bold: bool,
    pub hide_in_fullscreen: bool,
    /// Top-left corner in physical screen pixels; None centres it above the taskbar.
    pub float_position: Option<[i32; 2]>,
    pub lock_position: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStyle {
    #[default]
    Full,
    /// One letter per item.
    Short,
    /// No text; needs icons.
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveValue {
    #[default]
    Activity,
    Used,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskConfig {
    /// Drive letters with their own cell; empty shows all disks combined.
    pub drives: Vec<String>,
    pub value: DriveValue,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// PDH instance name of the one adapter to show; None sums hardware adapters.
    pub adapter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemConfig {
    pub kind: ItemKind,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// `#RRGGBB`; `None` uses the theme default.
    #[serde(default)]
    pub color: Option<String>,
    /// Graph mode: draw the history graph behind the text; off shows text only.
    #[serde(default = "enabled_default")]
    pub graph: bool,
    #[serde(default)]
    pub style: GraphStyle,
    #[serde(default)]
    pub color_mode: ColorMode,
    /// `None` uses `PopupConfig::detail`.
    #[serde(default)]
    pub popup: Option<PopupDetail>,
}

impl ItemConfig {
    pub fn new(kind: ItemKind, enabled: bool) -> Self {
        Self {
            kind,
            enabled,
            color: None,
            graph: true,
            style: GraphStyle::Area,
            color_mode: ColorMode::Item,
            popup: None,
        }
    }
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
                .map(|&kind| {
                    let enabled = matches!(
                        kind,
                        ItemKind::Cpu
                            | ItemKind::Ram
                            | ItemKind::Gpu
                            | ItemKind::Disk
                            | ItemKind::Network
                    );
                    ItemConfig::new(kind, enabled)
                })
                .collect(),
            ping: PingConfig::default(),
            disk: DiskConfig::default(),
            network: NetworkConfig::default(),
            units: Units::default(),
            popup: PopupConfig::default(),
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
            tray_icon: true,
            language: None,
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Graph,
            graph_inline: false,
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
            labels: LabelStyle::Full,
            icons: false,
            label_color: None,
            value_color: None,
            label_color_light: None,
            value_color_light: None,
            upload_on_top: false,
            color_labels: false,
            font_family: None,
            font_bold: false,
            hide_in_fullscreen: true,
            float_position: None,
            lock_position: false,
        }
    }
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            detail: PopupDetail::Standard,
            background: PopupBackground::Translucent,
            graph: true,
            processes: 5,
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
        g.language = g
            .language
            .as_deref()
            .and_then(crate::l10n::find)
            .map(|l| l.code.to_string());
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
        for color in [
            &mut d.tile_color,
            &mut d.panel_color,
            &mut d.label_color,
            &mut d.value_color,
            &mut d.label_color_light,
            &mut d.value_color_light,
        ] {
            if color.as_deref().is_some_and(|c| !is_hex_color(c)) {
                *color = None;
            }
        }
        if d.labels == LabelStyle::None {
            d.icons = true;
        }
        if d.font_family
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            d.font_family = None;
        }
        let mut drives: Vec<String> = Vec::new();
        for drive in &self.disk.drives {
            let upper = drive.trim().to_ascii_uppercase();
            if upper.len() == 1
                && upper.as_bytes()[0].is_ascii_uppercase()
                && !drives.contains(&upper)
            {
                drives.push(upper);
            }
        }
        drives.truncate(8);
        self.disk.drives = drives;
        if self
            .network
            .adapter
            .as_deref()
            .is_some_and(|a| a.trim().is_empty())
        {
            self.network.adapter = None;
        }
        self.ping.interval_ms = self.ping.interval_ms.clamp(1000, 60_000);
        self.popup.processes = self.popup.processes.min(MAX_POPUP_PROCESSES);
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
                self.items.push(ItemConfig::new(kind, false));
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

    pub fn drive_letters(&self) -> Vec<u8> {
        self.disk.drives.iter().map(|d| d.as_bytes()[0]).collect()
    }

    pub fn is_enabled(&self, kind: ItemKind) -> bool {
        self.items.iter().any(|i| i.kind == kind && i.enabled)
    }

    /// Items whose graph tile shows text only.
    pub fn text_only_items(&self) -> Vec<ItemKind> {
        self.items
            .iter()
            .filter(|i| !i.graph)
            .map(|i| i.kind)
            .collect()
    }

    pub fn item(&self, kind: ItemKind) -> Option<&ItemConfig> {
        self.items.iter().find(|i| i.kind == kind)
    }

    pub fn item_mut(&mut self, kind: ItemKind) -> Option<&mut ItemConfig> {
        self.items.iter_mut().find(|i| i.kind == kind)
    }

    /// Moves `kind` past the nearest enabled item before or after it, so the
    /// move shows on the overlay; does nothing at either end.
    pub fn move_item(&mut self, kind: ItemKind, earlier: bool) {
        let Some(from) = self.items.iter().position(|i| i.kind == kind) else {
            return;
        };
        let to = if earlier {
            self.items[..from].iter().rposition(|i| i.enabled)
        } else {
            self.items[from + 1..]
                .iter()
                .position(|i| i.enabled)
                .map(|p| from + 1 + p)
        };
        if let Some(to) = to {
            let item = self.items.remove(from);
            self.items.insert(to, item);
        }
    }

    /// How much the popup for `kind` shows.
    pub fn popup_detail(&self, kind: ItemKind) -> PopupDetail {
        self.item(kind)
            .and_then(|i| i.popup)
            .unwrap_or(self.popup.detail)
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
        || old.network.adapter != new.network.adapter
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
    /// Set when the file existed but could not be used.
    pub warning: Option<LoadWarning>,
}

/// Why the settings file was not used; formatted when shown, once the
/// language is known.
#[derive(Debug)]
pub enum LoadWarning {
    Unreadable {
        path: PathBuf,
        error: String,
    },
    /// The file was moved to `backup`, if that worked.
    Invalid {
        backup: Option<PathBuf>,
        reason: String,
    },
}

impl fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadWarning::Unreadable { path, error } => {
                f.write_str(&crate::tr!("Could not read {}: {}", path.display(), error))
            }
            LoadWarning::Invalid { backup, reason } => {
                f.write_str(crate::tr!(
                    "Settings file was invalid and defaults were used."
                ))?;
                if let Some(backup) = backup {
                    write!(
                        f,
                        " {}",
                        crate::tr!("It was saved as {}.", backup.display())
                    )?;
                }
                write!(f, "\n{reason}")
            }
        }
    }
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
            return back_up_invalid(path, crate::tr!("the file is not UTF-8 text"));
        }
        Err(e) => {
            return Loaded {
                config: Config::default(),
                warning: Some(LoadWarning::Unreadable {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                }),
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
    Loaded {
        config: Config::default(),
        warning: Some(LoadWarning::Invalid {
            backup: moved.then_some(backup),
            reason: reason.to_string(),
        }),
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
                    color: Some("#00FF00".into()),
                    ..ItemConfig::new(ItemKind::Gpu, true)
                },
                ItemConfig::new(ItemKind::Gpu, false),
                ItemConfig {
                    color: Some("red".into()),
                    ..ItemConfig::new(ItemKind::Cpu, true)
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
                ItemKind::CpuTemp,
                ItemKind::CpuPower,
                ItemKind::CpuClock,
                ItemKind::Ram,
                ItemKind::ChipsetTemp,
                ItemKind::Gpu,
                ItemKind::GpuTemp,
                ItemKind::Disk,
                ItemKind::DiskTemp,
                ItemKind::Network,
                ItemKind::Ping,
                ItemKind::Battery
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
    fn new_items_and_fields_default_off() {
        let c = Config::default();
        assert!(!c.is_enabled(ItemKind::GpuTemp));
        assert!(!c.is_enabled(ItemKind::CpuTemp));
        assert!(!c.is_enabled(ItemKind::CpuPower));
        assert!(!c.is_enabled(ItemKind::CpuClock));
        assert!(!c.is_enabled(ItemKind::DiskTemp));
        assert!(!c.is_enabled(ItemKind::ChipsetTemp));
        assert!(!c.is_enabled(ItemKind::Battery));
        assert_eq!(c.items.len(), 13);
        let d = &c.display;
        assert_eq!(d.labels, LabelStyle::Full);
        assert!(!d.icons && !d.color_labels && !d.font_bold && !d.lock_position);
        assert!(!d.graph_inline);
        assert!(c.general.tray_icon);
        assert!(d.hide_in_fullscreen);
        assert_eq!(
            (
                d.label_color.as_deref(),
                d.value_color.as_deref(),
                d.font_family.as_deref()
            ),
            (None, None, None)
        );
        assert_eq!(d.float_position, None);
        assert!(c.disk.drives.is_empty());
        assert_eq!(c.disk.value, DriveValue::Activity);
        assert_eq!(c.network.adapter, None);
    }

    #[test]
    fn an_old_file_loads_with_the_new_item_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[[items]]\nkind = \"cpu\"\n[[items]]\nkind = \"gpu\"\n",
        )
        .unwrap();
        let c = load(&path).config;
        let kinds: Vec<ItemKind> = c.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds[..2], [ItemKind::Cpu, ItemKind::Gpu]);
        assert!(!c.is_enabled(ItemKind::GpuTemp));
        assert!(c.display.hide_in_fullscreen);
        assert!(c.general.tray_icon);
    }

    #[test]
    fn sanitize_repairs_new_display_fields() {
        let mut c = Config::default();
        c.display.label_color = Some("blue".into());
        c.display.value_color = Some("#ABCDEF".into());
        c.display.font_family = Some("   ".into());
        c.disk.drives = vec!["c".into(), "CC".into(), "1".into(), "D".into(), "C".into()];
        c.network.adapter = Some(" ".into());
        let c = c.sanitize();
        assert_eq!(c.display.label_color, None);
        assert_eq!(c.display.value_color.as_deref(), Some("#ABCDEF"));
        assert_eq!(c.display.font_family, None);
        assert_eq!(c.disk.drives, ["C", "D"]);
        assert_eq!(c.drive_letters(), [b'C', b'D']);
        assert_eq!(c.network.adapter, None);
    }

    #[test]
    fn sanitize_keeps_only_known_languages() {
        let mut c = Config::default();
        c.general.language = Some("ES".into());
        assert_eq!(c.sanitize().general.language.as_deref(), Some("es"));
        let mut c = Config::default();
        c.general.language = Some("tlh".into());
        assert_eq!(c.sanitize().general.language, None);
    }

    #[test]
    fn floating_and_new_fields_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config::default();
        c.display.position = Position::Floating;
        c.display.float_position = Some([-1200, 40]);
        c.display.font_family = Some("Bahnschrift".into());
        c.display.graph_inline = true;
        c.general.tray_icon = false;
        c.disk.drives = vec!["C".into()];
        c.disk.value = DriveValue::Used;
        c.network.adapter = Some("Realtek 2.5GbE".into());
        save(&path, &c).unwrap();
        assert_eq!(load(&path).config, c);
    }

    #[test]
    fn adapter_and_gpu_sensors_change_sampling() {
        let base = Config::default();
        let mut adapter = base.clone();
        adapter.network.adapter = Some("x".into());
        assert!(sampling_changed(&base, &adapter));
        let mut temp = base.clone();
        temp.items
            .iter_mut()
            .filter(|i| i.kind == ItemKind::GpuTemp)
            .for_each(|i| i.enabled = true);
        assert!(sampling_changed(&base, &temp));
        let mut drives = base.clone();
        drives.disk.drives = vec!["C".into()];
        assert!(
            !sampling_changed(&base, &drives),
            "per-drive values are always sampled"
        );
    }

    #[test]
    fn ping_is_disabled_by_default() {
        assert!(!Config::default().is_enabled(ItemKind::Ping));
        assert!(Config::default().is_enabled(ItemKind::Cpu));
    }

    #[test]
    fn labels_none_needs_icons() {
        let mut c = Config::default();
        c.display.labels = LabelStyle::None;
        c.display.icons = false;
        assert!(c.sanitize().display.icons);
    }

    #[test]
    fn graphs_default_on_and_can_be_turned_off_per_item() {
        let c = Config::default();
        assert!(c.items.iter().all(|i| i.graph));
        assert!(c.text_only_items().is_empty());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[[items]]
kind = \"network\"
graph = false
[[items]]
kind = \"cpu\"
",
        )
        .unwrap();
        let c = load(&path).config;
        assert_eq!(c.text_only_items(), [ItemKind::Network]);
        assert!(c.items[1].graph, "missing field keeps the graph");
    }

    #[test]
    fn styles_units_and_popups_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config::default();
        c.units.temperature = TemperatureUnit::Fahrenheit;
        c.units.network = RateUnit::Bits;
        c.display.upload_on_top = true;
        c.display.label_color_light = Some("#202020".into());
        c.popup = PopupConfig {
            detail: PopupDetail::Detailed,
            background: PopupBackground::Solid,
            graph: false,
            processes: 8,
        };
        let cpu = c.item_mut(ItemKind::Cpu).unwrap();
        cpu.style = GraphStyle::Bars;
        cpu.color_mode = ColorMode::Load;
        cpu.popup = Some(PopupDetail::Simple);
        save(&path, &c).unwrap();
        let loaded = load(&path).config;
        assert_eq!(loaded, c);
        assert_eq!(loaded.popup_detail(ItemKind::Cpu), PopupDetail::Simple);
        assert_eq!(loaded.popup_detail(ItemKind::Ram), PopupDetail::Detailed);
    }

    #[test]
    fn moving_an_item_skips_hidden_ones() {
        let mut c = Config::default();
        let enabled = |c: &Config| -> Vec<ItemKind> { c.enabled_items() };
        assert_eq!(
            enabled(&c)[..2],
            [ItemKind::Cpu, ItemKind::Ram],
            "temperature, power and clock sit hidden between them"
        );
        c.move_item(ItemKind::Ram, true);
        assert_eq!(enabled(&c)[..2], [ItemKind::Ram, ItemKind::Cpu]);
        c.move_item(ItemKind::Ram, true);
        assert_eq!(
            enabled(&c)[..2],
            [ItemKind::Ram, ItemKind::Cpu],
            "already first"
        );
        c.move_item(ItemKind::Ram, false);
        assert_eq!(enabled(&c)[..2], [ItemKind::Cpu, ItemKind::Ram]);
        let last = *enabled(&c).last().unwrap();
        c.move_item(last, false);
        assert_eq!(*enabled(&c).last().unwrap(), last, "already last");
        assert_eq!(c.items.len(), ItemKind::ALL.len());
    }

    #[test]
    fn an_old_item_gets_the_default_style_and_popup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[[items]]\nkind = \"cpu\"\ngraph = false\n").unwrap();
        let c = load(&path).config;
        let cpu = c.item(ItemKind::Cpu).unwrap();
        assert!(!cpu.graph);
        assert_eq!(
            (cpu.style, cpu.color_mode, cpu.popup),
            (GraphStyle::Area, ColorMode::Item, None)
        );
        assert_eq!(c.units, Units::default());
        assert_eq!(c.popup, PopupConfig::default());
    }

    #[test]
    fn sanitize_caps_popup_processes_and_drops_bad_light_colours() {
        let mut c = Config::default();
        c.popup.processes = 200;
        c.display.value_color_light = Some("white".into());
        let c = c.sanitize();
        assert_eq!(c.popup.processes, MAX_POPUP_PROCESSES);
        assert_eq!(c.display.value_color_light, None);
    }

    #[test]
    fn labels_and_icons_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[display]\nlabels = \"short\"\nicons = true\n").unwrap();
        let d = load(&path).config.display;
        assert_eq!(d.labels, LabelStyle::Short);
        assert!(d.icons);
    }
}
