//! Named sets of what the overlay and widget show and how, kept as TOML
//! files in the profiles folder next to the config file. App-wide settings
//! (startup, language, sampling, games, alerts, shortcuts) are not part of
//! a profile.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{
    Config, DiskConfig, Display, FramesConfig, ItemConfig, NetworkConfig, PingConfig, PopupConfig,
    Units, WidgetConfig,
};
use crate::theme::{slug, valid_id};

const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub display: Display,
    pub items: Vec<ItemConfig>,
    pub ping: PingConfig,
    pub disk: DiskConfig,
    pub network: NetworkConfig,
    pub units: Units,
    pub popup: PopupConfig,
    pub widget: WidgetConfig,
    pub frames: FramesConfig,
}

impl Default for Profile {
    fn default() -> Self {
        Self::from_config("", &Config::default())
    }
}

impl Profile {
    pub fn from_config(name: &str, c: &Config) -> Self {
        Self {
            name: name.trim().to_string(),
            display: c.display.clone(),
            items: c.items.clone(),
            ping: c.ping.clone(),
            disk: c.disk.clone(),
            network: c.network.clone(),
            units: c.units,
            popup: c.popup,
            widget: c.widget.clone(),
            frames: c.frames.clone(),
        }
    }

    /// `c` with this profile's settings in place of its own.
    pub fn applied_to(&self, c: &Config) -> Config {
        Config {
            display: self.display.clone(),
            items: self.items.clone(),
            ping: self.ping.clone(),
            disk: self.disk.clone(),
            network: self.network.clone(),
            units: self.units,
            popup: self.popup,
            widget: self.widget.clone(),
            frames: self.frames.clone(),
            ..c.clone()
        }
        .sanitize()
    }

    /// Whether `c` shows exactly what this profile does.
    pub fn matches(&self, c: &Config) -> bool {
        self.applied_to(c) == *c
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
}

/// `%APPDATA%\Pulsar\profiles`.
pub fn profiles_dir() -> Option<PathBuf> {
    crate::config::default_path().and_then(|p| p.parent().map(|d| d.join("profiles")))
}

/// The readable profiles in `dir`, by name.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .filter_map(|p| {
            let id = p.file_stem()?.to_str()?.to_string();
            let profile = valid_id(&id).then(|| read_file(&p)).flatten()?;
            let name = if profile.name.is_empty() {
                id.clone()
            } else {
                profile.name
            };
            Some(Entry { id, name })
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

pub fn load(dir: &Path, id: &str) -> Option<Profile> {
    valid_id(id)
        .then(|| read_file(&dir.join(format!("{id}.toml"))))
        .flatten()
}

/// Writes `profile` to the file named after it, replacing a profile of the
/// same name, and returns its id.
pub fn save(dir: &Path, profile: &Profile) -> io::Result<String> {
    std::fs::create_dir_all(dir)?;
    let id = slug(&profile.name, "profile");
    let text = toml::to_string_pretty(profile).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{id}.toml.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(format!("{id}.toml")))?;
    Ok(id)
}

pub fn delete(dir: &Path, id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    std::fs::remove_file(dir.join(format!("{id}.toml")))
}

fn read_file(path: &Path) -> Option<Profile> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        log::warn!("{} is too large for a profile", path.display());
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text)
        .inspect_err(|e| log::warn!("could not read profile {}: {e}", path.display()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DisplayMode, TemperatureUnit};
    use crate::metric::ItemKind;

    fn gaming() -> Config {
        let mut c = Config::default();
        c.display.mode = DisplayMode::Text;
        c.units.temperature = TemperatureUnit::Fahrenheit;
        c.widget.enabled = true;
        c.item_mut(ItemKind::Fps).unwrap().enabled = true;
        c.sanitize()
    }

    #[test]
    fn applying_restores_the_saved_settings() {
        let dir = tempfile::tempdir().unwrap();
        let id = save(dir.path(), &Profile::from_config("Gaming", &gaming())).unwrap();
        assert_eq!(id, "gaming");
        let profile = load(dir.path(), &id).unwrap();
        let applied = profile.applied_to(&Config::default());
        assert!(profile.matches(&applied));
        assert!(!profile.matches(&Config::default()));
        assert_eq!(applied.display.mode, DisplayMode::Text);
        assert!(applied.is_enabled(ItemKind::Fps));
    }

    #[test]
    fn app_wide_settings_are_kept() {
        let mut c = Config::default();
        c.general.sample_interval_ms = 2000;
        c.hotkeys.record = Some("Ctrl+Alt+R".into());
        c.alerts.cpu.on = true;
        let applied = Profile::from_config("Gaming", &gaming()).applied_to(&c);
        assert_eq!(applied.general, c.general);
        assert_eq!(applied.hotkeys, c.hotkeys);
        assert_eq!(applied.alerts, c.alerts);
    }

    #[test]
    fn saving_a_name_again_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        save(
            dir.path(),
            &Profile::from_config("Gaming", &Config::default()),
        )
        .unwrap();
        save(dir.path(), &Profile::from_config("Gaming", &gaming())).unwrap();
        save(
            dir.path(),
            &Profile::from_config("Desk", &Config::default()),
        )
        .unwrap();
        let names: Vec<_> = list(dir.path()).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Desk", "Gaming"]);
        assert_eq!(
            load(dir.path(), "gaming").unwrap().display.mode,
            DisplayMode::Text
        );
        delete(dir.path(), "desk").unwrap();
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[test]
    fn a_partial_file_fills_in_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("min.toml"), "name = \"Min\"\n").unwrap();
        let profile = load(dir.path(), "min").unwrap();
        assert_eq!(profile.applied_to(&Config::default()), Config::default());
    }

    #[test]
    fn ids_cannot_name_other_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path(), "../config").is_none());
        assert!(delete(dir.path(), "..\\config").is_err());
    }
}
