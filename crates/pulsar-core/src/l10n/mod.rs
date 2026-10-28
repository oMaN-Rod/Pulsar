//! Text in the user's language.
//!
//! The English text in the source is the key: `tr!("Settings")` looks it up
//! in the current language's catalogs and falls back to the English. Each
//! crate keeps a gettext catalog per language at
//! `locales/<code>/LC_MESSAGES/<package>.po`; this crate's catalogs are
//! always loaded, and an application adds its own with [`add_catalogs`].

#[doc(hidden)]
pub mod catalog;
mod po;

use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::{PoisonError, RwLock};

use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::core::PWSTR;

#[derive(Debug, PartialEq, Eq)]
pub struct Language {
    /// The catalog folder name, and what the config stores.
    pub code: &'static str,
    /// The language's name for itself.
    pub name: &'static str,
    /// A Windows font for windows in this language, when Segoe UI lacks
    /// its script and font fallback could pick the wrong regional forms.
    pub font: Option<&'static str>,
    /// Windows language tags this language is chosen for: a tag matches
    /// itself and any longer tag it starts.
    tags: &'static [&'static str],
}

/// English first: it needs no catalog.
pub const LANGUAGES: [Language; 3] = [
    Language {
        code: "en",
        name: "English",
        font: None,
        tags: &["en"],
    },
    Language {
        code: "es",
        name: "Español",
        font: None,
        tags: &["es"],
    },
    Language {
        code: "zh-CN",
        name: "简体中文",
        font: Some("Microsoft YaHei UI"),
        tags: &["zh-CN", "zh-SG", "zh-Hans"],
    },
];

/// `.po` sources by language code.
pub type Catalogs = &'static [(&'static str, &'static str)];

const CATALOGS: Catalogs = include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

type Table = HashMap<&'static str, &'static str>;

struct State {
    added: Vec<Catalogs>,
    language: &'static Language,
    table: Option<&'static Table>,
    /// Tables already built, so switching back costs nothing.
    built: Vec<(&'static str, &'static Table)>,
}

static STATE: RwLock<State> = RwLock::new(State {
    added: Vec::new(),
    language: &LANGUAGES[0],
    table: None,
    built: Vec::new(),
});

/// Adds an application's catalogs; call before [`set_language`].
pub fn add_catalogs(catalogs: Catalogs) {
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    state.added.push(catalogs);
    state.built.clear();
}

/// Switches to `choice` (a language code), or to the best match for the
/// Windows display languages when it is `None` or unknown.
pub fn set_language(choice: Option<&str>) -> &'static Language {
    let language = choice
        .and_then(find)
        .unwrap_or_else(|| detect(&system_languages()));
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    state.language = language;
    let table = (language.code != LANGUAGES[0].code).then(|| state.table_for(language.code));
    state.table = table;
    language
}

pub fn current() -> &'static Language {
    STATE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .language
}

pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code))
}

/// What `None` resolves to on this machine.
pub fn system_default() -> &'static Language {
    detect(&system_languages())
}

/// The text for `msgid` in the current language. Use [`tr!`](crate::tr)
/// for literals, so the catalog tooling finds them.
pub fn translate(msgid: &'static str) -> &'static str {
    let state = STATE.read().unwrap_or_else(PoisonError::into_inner);
    state
        .table
        .and_then(|t| t.get(msgid).copied())
        .unwrap_or(msgid)
}

impl State {
    fn table_for(&mut self, code: &'static str) -> &'static Table {
        if let Some(&(_, table)) = self.built.iter().find(|(c, _)| *c == code) {
            return table;
        }
        let mut table = Table::new();
        for catalogs in std::iter::once(CATALOGS).chain(self.added.iter().copied()) {
            for &(_, source) in catalogs.iter().filter(|(c, _)| *c == code) {
                for entry in po::parse(source) {
                    if entry.msgid.is_empty()
                        || entry.msgstr.is_empty()
                        || entry.fuzzy
                        || entry.plural
                        || entry.context.is_some()
                    {
                        continue;
                    }
                    table.insert(leak(entry.msgid), leak(entry.msgstr));
                }
            }
        }
        // Built once per language and kept for the life of the process.
        let table: &'static Table = Box::leak(Box::new(table));
        self.built.push((code, table));
        table
    }
}

fn leak(s: std::borrow::Cow<'static, str>) -> &'static str {
    match s {
        std::borrow::Cow::Borrowed(s) => s,
        std::borrow::Cow::Owned(s) => s.leak(),
    }
}

fn tag_matches(tag: &str, pattern: &str) -> bool {
    tag.get(..pattern.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(pattern))
        && matches!(tag.as_bytes().get(pattern.len()), None | Some(b'-'))
}

/// The first of the preferred tags that Pulsar speaks, else English.
fn detect(preferred: &[String]) -> &'static Language {
    preferred
        .iter()
        .find_map(|tag| {
            LANGUAGES
                .iter()
                .find(|l| l.tags.iter().any(|p| tag_matches(tag, p)))
        })
        .unwrap_or(&LANGUAGES[0])
}

/// The user's display languages, most preferred first, such as `es-MX`.
fn system_languages() -> Vec<String> {
    let (mut count, mut len) = (0u32, 0u32);
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_err() {
            return Vec::new();
        }
        let mut buffer = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
        .is_err()
        {
            return Vec::new();
        }
        buffer
            .split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

/// Fills `{}` (the next argument) and `{0}`, `{1}`… (by position) in a
/// translated template; `{{` and `}}` are literal braces.
pub fn format(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let placeholder = tail.strip_prefix('{').and_then(|t| {
            let end = t.find('}')?;
            let index = match &t[..end] {
                "" => {
                    next += 1;
                    next - 1
                }
                digits => digits.parse().ok()?,
            };
            Some((args.get(index)?, end + 2))
        });
        match placeholder {
            Some((arg, len)) => {
                let _ = write!(out, "{arg}");
                rest = &tail[len..];
            }
            None => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The current language's text for an English literal. With arguments it
/// fills the placeholders (see [`l10n::format`](crate::l10n::format)) and
/// returns a `String`; without, a `&'static str`.
#[macro_export]
macro_rules! tr {
    ($msgid:literal) => {
        $crate::l10n::translate($msgid)
    };
    ($msgid:literal, $($arg:expr),+ $(,)?) => {
        $crate::l10n::format(
            $crate::l10n::translate($msgid),
            &[$(&$arg as &dyn ::std::fmt::Display),+],
        )
    };
}

/// Marks a literal for the catalogs without translating it, for text that
/// is translated later with [`l10n::translate`](crate::l10n::translate).
#[macro_export]
macro_rules! tr_noop {
    ($msgid:literal) => {
        $msgid
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn windows_languages_pick_the_first_match() {
        assert_eq!(detect(&tags(&["es-MX", "en-US"])).code, "es");
        assert_eq!(detect(&tags(&["fr-FR", "zh-CN"])).code, "zh-CN");
        assert_eq!(detect(&tags(&["zh-Hans-HK"])).code, "zh-CN");
        assert_eq!(detect(&tags(&["zh-TW", "es"])).code, "es");
        assert_eq!(detect(&tags(&["en-GB", "es-ES"])).code, "en");
        assert_eq!(detect(&tags(&["fr-FR"])).code, "en");
        assert_eq!(detect(&[]).code, "en");
    }

    #[test]
    fn tags_match_whole_subtags() {
        assert!(tag_matches("es", "es"));
        assert!(tag_matches("ES-419", "es"));
        assert!(!tag_matches("est", "es"));
        assert!(!tag_matches("zh-Hant-TW", "zh-Hans"));
    }

    #[test]
    fn codes_are_found_regardless_of_case() {
        assert_eq!(find("zh-cn").map(|l| l.name), Some("简体中文"));
        assert_eq!(find("fr"), None);
    }

    #[test]
    fn placeholders_fill_in_order_or_by_position() {
        assert_eq!(
            format("Pulsar {} is available", &[&"1.2"]),
            "Pulsar 1.2 is available"
        );
        assert_eq!(format("{} of {}", &[&1, &2]), "1 of 2");
        assert_eq!(format("{1} / {0}", &[&"a", &"b"]), "b / a");
        assert_eq!(format("{{literal}} {}", &[&3]), "{literal} 3");
        assert_eq!(format("{5} {x} {", &[&1]), "{5} {x} {");
    }

    #[test]
    fn catalogs_load_with_escapes_intact() {
        let mut state = State {
            added: Vec::new(),
            language: &LANGUAGES[1],
            table: None,
            built: Vec::new(),
        };
        let table = state.table_for("es");
        assert_eq!(
            table.get("Pulsar Settings").copied(),
            Some("Configuración de Pulsar")
        );
        assert_eq!(
            table.get("Adapter \"{}\" not found").copied(),
            Some("No se encontró el adaptador «{}»")
        );
        assert!(std::ptr::eq(state.table_for("es"), table), "built once");
    }

    #[test]
    fn every_catalog_is_a_known_language() {
        for (code, _) in CATALOGS {
            assert!(
                find(code).is_some_and(|l| l.code == *code),
                "{code} is not in LANGUAGES"
            );
        }
    }
}
