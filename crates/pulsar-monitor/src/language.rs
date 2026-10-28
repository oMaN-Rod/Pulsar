//! This crate's catalogs, and switching every process to the configured
//! language.

use pulsar_core::config::Config;
use pulsar_core::l10n::{self, Catalogs, Language};

const CATALOGS: Catalogs = include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// Loads the catalogs and applies the config's language; call once, early.
pub fn init(config: &Config) -> &'static Language {
    l10n::add_catalogs(CATALOGS);
    apply(config)
}

pub fn apply(config: &Config) -> &'static Language {
    l10n::set_language(config.general.language.as_deref())
}

/// The family for window text in the current language.
#[allow(dead_code)]
pub fn ui_font() -> &'static str {
    l10n::current().font.unwrap_or("Segoe UI Variable Text")
}

/// Switches the `.slint` text too; needs a component to exist already.
#[allow(dead_code)]
pub fn apply_to_slint(language: &Language) {
    if let Err(e) = slint::select_bundled_translation(language.code) {
        log::warn!("could not switch the window to {}: {e}", language.code);
    }
}
