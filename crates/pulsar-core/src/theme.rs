//! Named colour themes: the built-in ones and TOML files in the themes
//! folder next to the config file.
//!
//! A theme supplies the overlay's default colours; colours set in the config
//! still win. Each theme has a dark and a light variant. A file that only
//! describes the dark one gets a light one derived from it.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::colors::{self, Rgb};
use crate::layout::CellPart;
use crate::metric::ItemKind;

/// The theme used when none is chosen or the chosen one is missing.
pub const DEFAULT_ID: &str = "windows";

/// Larger theme files are ignored.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// A dark variant this opaque brings its own background, so it also serves
/// a light taskbar unchanged.
const OWN_BACKGROUND_OPACITY: u8 = 50;

/// Item colours derived for a light taskbar keep at least this contrast
/// against it.
const LIGHT_ITEM_CONTRAST: f64 = 3.0;

const BUILTIN: [(&str, &str, &str); 8] = [
    (DEFAULT_ID, crate::tr_noop!("Windows default"), ""),
    (
        "aurora",
        crate::tr_noop!("Aurora"),
        include_str!("../themes/aurora.toml"),
    ),
    (
        "ember",
        crate::tr_noop!("Ember"),
        include_str!("../themes/ember.toml"),
    ),
    (
        "ocean",
        crate::tr_noop!("Ocean"),
        include_str!("../themes/ocean.toml"),
    ),
    (
        "forest",
        crate::tr_noop!("Forest"),
        include_str!("../themes/forest.toml"),
    ),
    (
        "mono",
        crate::tr_noop!("Mono"),
        include_str!("../themes/mono.toml"),
    ),
    (
        "neon",
        crate::tr_noop!("Neon"),
        include_str!("../themes/neon.toml"),
    ),
    (
        "sunset",
        crate::tr_noop!("Sunset"),
        include_str!("../themes/sunset.toml"),
    ),
];

/// One variant's colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    pub text: Rgb,
    pub label: Rgb,
    pub tile: Rgb,
    /// 0–100.
    pub tile_opacity: u8,
    /// Also the popup background.
    pub panel: Rgb,
    /// 0–100; 0 draws no panel behind the overlay.
    pub panel_opacity: u8,
    items: Vec<(ItemKind, Rgb)>,
}

impl Colors {
    pub fn windows(light: bool) -> Self {
        let t = colors::theme_rgb(light);
        Self {
            text: t.text,
            label: t.label,
            tile: t.tile,
            tile_opacity: if light { 6 } else { 8 },
            panel: t.panel,
            panel_opacity: 0,
            items: Vec::new(),
        }
    }

    pub fn item(&self, kind: ItemKind, part: CellPart) -> Rgb {
        self.items
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or_else(|| colors::default_rgb(kind, part), |(_, c)| *c)
    }

    pub fn set_item(&mut self, kind: ItemKind, color: Rgb) {
        self.items.retain(|(k, _)| *k != kind);
        self.items.push((kind, color));
    }

    /// The item's colour when the theme sets one.
    pub fn item_override(&self, kind: ItemKind) -> Option<Rgb> {
        self.items.iter().find(|(k, _)| *k == kind).map(|(_, c)| *c)
    }

    fn from_file(file: &VariantFile, base: Colors) -> Self {
        let pick = |hex: &Option<String>, fallback: Rgb| {
            hex.as_deref().and_then(parse_hex).unwrap_or(fallback)
        };
        let items = ItemKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let hex = file.items.get(kind_key(kind))?;
                parse_hex(hex).map(|c| (kind, c))
            })
            .collect();
        Self {
            text: pick(&file.text, base.text),
            label: pick(&file.label, base.label),
            tile: pick(&file.tile, base.tile),
            tile_opacity: file.tile_opacity.map_or(base.tile_opacity, |o| o.min(100)),
            panel: pick(&file.panel, base.panel),
            panel_opacity: file
                .panel_opacity
                .map_or(base.panel_opacity, |o| o.min(100)),
            items,
        }
    }

    /// Windows' light colours with this variant's item colours, darkened
    /// until they read on a light taskbar.
    fn derive_light(&self) -> Self {
        if self.panel_opacity >= OWN_BACKGROUND_OPACITY {
            return self.clone();
        }
        let light = Self::windows(true);
        let items = self
            .items
            .iter()
            .map(|&(kind, c)| {
                (
                    kind,
                    darken_to_contrast(c, light.panel, LIGHT_ITEM_CONTRAST),
                )
            })
            .collect();
        Self { items, ..light }
    }

    fn to_file(&self) -> VariantFile {
        VariantFile {
            text: Some(to_hex(self.text)),
            label: Some(to_hex(self.label)),
            tile: Some(to_hex(self.tile)),
            tile_opacity: Some(self.tile_opacity),
            panel: Some(to_hex(self.panel)),
            panel_opacity: Some(self.panel_opacity),
            items: self
                .items
                .iter()
                .map(|&(k, c)| (kind_key(k).to_string(), to_hex(c)))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub id: String,
    /// Untranslated for built-in themes; see [`Entry::name`].
    pub name: String,
    pub dark: Colors,
    pub light: Colors,
    /// Applying the theme turns on "Colour labels by item".
    pub color_labels: bool,
}

impl Theme {
    pub fn windows() -> Self {
        Self {
            id: DEFAULT_ID.into(),
            name: BUILTIN[0].1.into(),
            dark: Colors::windows(false),
            light: Colors::windows(true),
            color_labels: false,
        }
    }

    pub fn colors(&self, light_taskbar: bool) -> &Colors {
        if light_taskbar {
            &self.light
        } else {
            &self.dark
        }
    }

    fn from_file(id: &str, file: &ThemeFile) -> Self {
        let dark = Colors::from_file(&file.dark, Colors::windows(false));
        let light = match &file.light {
            Some(light) => Colors::from_file(light, Colors::windows(true)),
            None => dark.derive_light(),
        };
        Self {
            id: id.into(),
            name: file.name.clone().unwrap_or_else(|| id.into()),
            dark,
            light,
            color_labels: file.color_labels,
        }
    }

    pub fn to_file(&self) -> ThemeFile {
        ThemeFile {
            name: Some(self.name.clone()),
            color_labels: self.color_labels,
            dark: self.dark.to_file(),
            light: Some(self.light.to_file()),
        }
    }
}

/// A theme as written in a `.toml` file; every field is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeFile {
    pub name: Option<String>,
    pub color_labels: bool,
    pub dark: VariantFile,
    /// Derived from `dark` when missing.
    pub light: Option<VariantFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VariantFile {
    pub text: Option<String>,
    pub label: Option<String>,
    pub tile: Option<String>,
    pub tile_opacity: Option<u8>,
    pub panel: Option<String>,
    pub panel_opacity: Option<u8>,
    /// Item kind (`cpu`, `gpu_temp`, …) to `#RRGGBB`.
    pub items: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Built-in names are untranslated msgids.
    pub name: String,
    pub builtin: bool,
}

impl Entry {
    /// The name in the current language.
    pub fn display_name(&self) -> String {
        BUILTIN
            .iter()
            .find(|b| self.builtin && b.0 == self.id)
            .map_or_else(|| self.name.clone(), |b| crate::l10n::translate(b.1).into())
    }
}

/// `%APPDATA%\Pulsar\themes`.
pub fn themes_dir() -> Option<PathBuf> {
    crate::config::default_path().and_then(|p| p.parent().map(|d| d.join("themes")))
}

/// Built-in themes first, then the files in `dir` by name.
pub fn list(dir: Option<&Path>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = BUILTIN
        .iter()
        .map(|&(id, name, _)| Entry {
            id: id.into(),
            name: name.into(),
            builtin: true,
        })
        .collect();
    let mut files: Vec<Entry> = dir
        .map(user_files)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(id, path)| {
            let file = read_file(&path)?;
            Some(Entry {
                name: file.name.unwrap_or_else(|| id.clone()),
                id,
                builtin: false,
            })
        })
        .collect();
    files.sort_by_key(|e| e.name.to_lowercase());
    entries.extend(files);
    entries
}

/// The theme with `id`, or Windows default when it is missing or unreadable.
pub fn load(dir: Option<&Path>, id: Option<&str>) -> Theme {
    let Some(id) = id.filter(|id| *id != DEFAULT_ID) else {
        return Theme::windows();
    };
    if let Some(&(id, name, text)) = BUILTIN.iter().find(|b| b.0 == id) {
        let file = toml::from_str::<ThemeFile>(text).expect("built-in themes parse");
        return Theme {
            name: name.into(),
            ..Theme::from_file(id, &file)
        };
    }
    let file = valid_id(id)
        .then(|| dir.map(|d| d.join(format!("{id}.toml"))))
        .flatten()
        .and_then(|path| read_file(&path));
    match file {
        Some(file) => Theme::from_file(id, &file),
        None => {
            log::warn!("theme {id:?} not found; using the default");
            Theme::windows()
        }
    }
}

/// Writes `theme` as a new file named after it and returns its id.
pub fn save(dir: &Path, theme: &Theme) -> io::Result<String> {
    std::fs::create_dir_all(dir)?;
    let stem = slug(&theme.name, "theme");
    let id = (1..)
        .map(|n| {
            if n == 1 {
                stem.clone()
            } else {
                format!("{stem}-{n}")
            }
        })
        .find(|id| BUILTIN.iter().all(|b| b.0 != id) && !dir.join(format!("{id}.toml")).exists())
        .expect("an unused name exists");
    let text = toml::to_string_pretty(&theme.to_file()).map_err(io::Error::other)?;
    std::fs::write(dir.join(format!("{id}.toml")), text)?;
    Ok(id)
}

/// Ids are lowercase letters, digits and dashes, so one can't name a path.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(crate) fn slug(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(48).collect();
    if out.is_empty() { fallback.into() } else { out }
}

fn user_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .filter_map(|p| {
            let id = p.file_stem()?.to_str()?.to_string();
            (valid_id(&id) && BUILTIN.iter().all(|b| b.0 != id)).then_some((id, p))
        })
        .collect()
}

fn read_file(path: &Path) -> Option<ThemeFile> {
    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_FILE_BYTES {
        log::warn!("{} is too large for a theme", path.display());
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text)
        .inspect_err(|e| log::warn!("could not read theme {}: {e}", path.display()))
        .ok()
}

fn kind_key(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Cpu => "cpu",
        ItemKind::Ram => "ram",
        ItemKind::Disk => "disk",
        ItemKind::Network => "network",
        ItemKind::Gpu => "gpu",
        ItemKind::GpuTemp => "gpu_temp",
        ItemKind::Ping => "ping",
        ItemKind::CpuTemp => "cpu_temp",
        ItemKind::CpuPower => "cpu_power",
        ItemKind::CpuClock => "cpu_clock",
        ItemKind::DiskTemp => "disk_temp",
        ItemKind::ChipsetTemp => "chipset_temp",
        ItemKind::Battery => "battery",
        ItemKind::Fps => "fps",
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// WCAG relative luminance.
fn luminance((r, g, b): Rgb) -> f64 {
    let channel = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG contrast ratio, 1–21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn darken_to_contrast(c: Rgb, background: Rgb, ratio: f64) -> Rgb {
    let mut c = c;
    for _ in 0..40 {
        if contrast(c, background) >= ratio {
            break;
        }
        let scale = |v: u8| (f64::from(v) * 0.92) as u8;
        c = (scale(c.0), scale(c.1), scale(c.2));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_loads_with_a_name_and_item_colours() {
        for entry in list(None) {
            let theme = load(None, Some(&entry.id));
            assert_eq!(theme.id, entry.id);
            assert!(!theme.name.is_empty());
            if entry.id != DEFAULT_ID {
                assert_eq!(theme.dark.items.len(), ItemKind::ALL.len(), "{}", entry.id);
            }
        }
    }

    #[test]
    fn missing_and_default_ids_give_windows() {
        assert_eq!(load(None, None), Theme::windows());
        assert_eq!(load(None, Some("nope")), Theme::windows());
        assert_eq!(load(None, Some("../config")), Theme::windows());
    }

    #[test]
    fn derived_light_variants_read_on_a_light_taskbar() {
        let background = Colors::windows(true).panel;
        for entry in list(None) {
            let theme = load(None, Some(&entry.id));
            if theme.light.panel_opacity >= OWN_BACKGROUND_OPACITY {
                continue;
            }
            assert_eq!(theme.light.text, Colors::windows(true).text);
            for &(kind, c) in &theme.light.items {
                assert!(
                    contrast(c, background) >= LIGHT_ITEM_CONTRAST,
                    "{} {kind:?}",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn a_theme_with_its_own_background_keeps_it_on_light_taskbars() {
        let neon = load(None, Some("neon"));
        assert_eq!(neon.light, neon.dark);
        assert!(neon.color_labels);
    }

    #[test]
    fn missing_fields_fall_back_to_windows() {
        let file: ThemeFile =
            toml::from_str("[dark]\ntext = \"#123456\"\n[dark.items]\ncpu = \"bad\"").unwrap();
        let theme = Theme::from_file("x", &file);
        assert_eq!(theme.dark.text, (0x12, 0x34, 0x56));
        assert_eq!(theme.dark.label, Colors::windows(false).label);
        assert_eq!(theme.dark.item_override(ItemKind::Cpu), None);
        assert_eq!(theme.name, "x");
    }

    #[test]
    fn saved_themes_round_trip_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let theme = Theme {
            name: "My Theme!".into(),
            ..load(None, Some("ocean"))
        };
        let first = save(dir.path(), &theme).unwrap();
        let second = save(dir.path(), &theme).unwrap();
        assert_eq!(first, "my-theme");
        assert_eq!(second, "my-theme-2");
        let back = load(Some(dir.path()), Some(&first));
        assert_eq!(back.dark, theme.dark);
        assert_eq!(back.light, theme.light);
        assert_eq!(back.name, "My Theme!");
        let names: Vec<_> = list(Some(dir.path()))
            .into_iter()
            .filter(|e| !e.builtin)
            .collect();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn files_cannot_shadow_builtins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ocean.toml"), "name = \"Fake\"").unwrap();
        assert_eq!(load(Some(dir.path()), Some("ocean")).name, "Ocean");
        assert!(list(Some(dir.path())).iter().all(|e| e.builtin));
        let saved = save(
            dir.path(),
            &Theme {
                name: "Ocean".into(),
                ..Theme::windows()
            },
        )
        .unwrap();
        assert_eq!(saved, "ocean-2");
    }

    #[test]
    fn slugs_are_valid_ids() {
        for name in ["Ocean", "  ", "Ünïcode café", "a/b\\c", &"x".repeat(200)] {
            assert!(valid_id(&slug(name, "theme")), "{name:?}");
        }
    }
}
