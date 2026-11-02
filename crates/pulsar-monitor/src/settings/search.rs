use pulsar_core::tr_noop;

pub struct IndexRow {
    pub tab: i32,
    pub page: &'static str,
    pub section: &'static str,
    pub title: &'static str,
    /// The condition that must have this value for the row to wear this title.
    pub when: Option<(&'static str, bool)>,
    pub descriptions: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub is_page: bool,
}

const INDEX: &[IndexRow] = include!(concat!(env!("OUT_DIR"), "/settings_index.rs"));

/// The one condition the index may carry; [`entries`] resolves it.
const FAHRENHEIT: &str = "root.alert-fahrenheit";
const SEPARATOR: &str = " › ";

/// An item row of the Items page; it takes its name from code, not the `.slint` file.
pub struct Item {
    pub id: String,
    pub name: &'static str,
}

pub struct Entry {
    /// Stays the same across languages and source order.
    pub id: String,
    pub tab: i32,
    pub title: &'static str,
    pub path: String,
    pub description: &'static str,
    /// False for a whole page, which has no row to bring into view.
    pub is_row: bool,
    title_text: String,
    place_text: String,
    other_text: String,
}

/// Lower case without Latin accents, so `energia` finds `energía`.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

fn texts<'a>(
    messages: impl IntoIterator<Item = &'a &'static str>,
    translate: &impl Fn(&'static str) -> &'static str,
) -> String {
    let mut out = String::new();
    for &message in messages {
        out.push_str(message);
        out.push('\n');
        out.push_str(translate(message));
        out.push('\n');
    }
    fold(&out)
}

/// Everything that can be found, with the temperature titles of the unit in use.
pub fn entries(
    items: &[Item],
    fahrenheit: bool,
    translate: impl Fn(&'static str) -> &'static str,
) -> Vec<Entry> {
    let mut found: Vec<Entry> = INDEX
        .iter()
        .filter(|row| match row.when {
            Some((FAHRENHEIT, wanted)) => wanted == fahrenheit,
            _ => true,
        })
        .map(|row| index_entry(row, &translate))
        .collect();
    let items_page = INDEX.iter().find(|row| row.is_page && row.tab == 0);
    let page = items_page.map_or("", |row| row.page);
    for item in items {
        found.push(Entry {
            id: item.id.clone(),
            tab: 0,
            title: translate(item.name),
            path: translate(page).to_string(),
            description: "",
            is_row: true,
            title_text: texts([&item.name], &translate),
            place_text: texts([&page], &translate),
            other_text: String::new(),
        });
    }
    found
}

fn index_entry(row: &IndexRow, translate: &impl Fn(&'static str) -> &'static str) -> Entry {
    let path = if row.is_page {
        translate(tr_noop!("Settings page")).to_string()
    } else if row.section.is_empty() {
        translate(row.page).to_string()
    } else {
        format!(
            "{}{SEPARATOR}{}",
            translate(row.page),
            translate(row.section)
        )
    };
    let description = row
        .descriptions
        .iter()
        .map(|&d| translate(d))
        .filter(|d| !d.contains('{'))
        .max_by_key(|d| d.chars().count())
        .unwrap_or_default();
    Entry {
        id: if row.is_page {
            format!("page/{}", row.tab)
        } else {
            format!("row/{}/{}", row.tab, row.title)
        },
        tab: row.tab,
        title: translate(row.title),
        path,
        description,
        is_row: !row.is_page,
        title_text: texts([&row.title], translate),
        place_text: texts([&row.page, &row.section], translate),
        other_text: texts(row.descriptions.iter().chain(row.keywords), translate),
    }
}

fn starts_word(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(at, _)| {
        !text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
    })
}

/// How well `token` fits, lower being better.
fn rank(entry: &Entry, token: &str) -> Option<u32> {
    if starts_word(&entry.title_text, token) {
        Some(0)
    } else if entry.title_text.contains(token) {
        Some(1)
    } else if entry.place_text.contains(token) {
        Some(2)
    } else if entry.other_text.contains(token) {
        Some(3)
    } else {
        None
    }
}

/// The entries holding every word of `query`, best first. Scripts written
/// without spaces match as a substring of the title, so `字体` finds `字体大小`.
pub fn matches<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let query = fold(query);
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(u32, &Entry)> = entries
        .iter()
        .filter_map(|entry| {
            let ranks: Option<Vec<u32>> = tokens.iter().map(|t| rank(entry, t)).collect();
            Some((ranks?.iter().sum(), entry))
        })
        .collect();
    ranked.sort_by_key(|&(rank, entry)| (rank, entry.title.chars().count()));
    ranked.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use pulsar_core::config::Config;
    use pulsar_core::l10n;

    use super::*;
    use crate::language;

    fn english(message: &'static str) -> &'static str {
        message
    }

    fn items() -> Vec<Item> {
        vec![
            Item {
                id: "item/Cpu".to_string(),
                name: "CPU",
            },
            Item {
                id: "item/Ram".to_string(),
                name: "Memory",
            },
        ]
    }

    fn ids(found: &[&Entry]) -> Vec<String> {
        found.iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn every_word_must_match() {
        let all = entries(&[], false, english);
        let found = ids(&matches(&all, "font size"));
        assert_eq!(found.first().map(String::as_str), Some("row/1/Font size"));
        assert!(!found.contains(&"row/1/Bold".to_string()));
        assert!(matches(&all, "font zzzz").is_empty());
    }

    #[test]
    fn a_blank_query_finds_nothing() {
        let all = entries(&[], false, english);
        assert!(matches(&all, "").is_empty());
        assert!(matches(&all, "  \t ").is_empty());
    }

    #[test]
    fn titles_rank_above_descriptions() {
        let all = entries(&[], false, english);
        let found = matches(&all, "offset");
        assert_eq!(found[0].id, "row/1/Offset");
    }

    #[test]
    fn a_section_or_page_name_finds_its_rows() {
        let all = entries(&[], false, english);
        let found = ids(&matches(&all, "alerts"));
        assert!(found.contains(&"row/7/Battery charge (%)".to_string()));
        let found = ids(&matches(&all, "layout mode"));
        assert!(found.contains(&"row/1/Mode".to_string()));
    }

    #[test]
    fn choices_and_buttons_are_searched() {
        let all = entries(&[], false, english);
        assert!(ids(&matches(&all, "fahrenheit")).contains(&"row/5/Temperature".to_string()));
        assert!(ids(&matches(&all, "mbps")).contains(&"row/6/Network rates".to_string()));
    }

    #[test]
    fn matching_ignores_case_and_accents() {
        assert_eq!(fold("Energía TAMAÑO"), "energia tamano");
    }

    #[test]
    fn items_are_found_with_stable_ids() {
        let all = entries(&items(), false, english);
        let found = matches(&all, "memory");
        assert_eq!(found[0].id, "item/Ram");
        assert!(found[0].is_row);
        assert_eq!(found[0].tab, 0);
        assert_eq!(found[0].path, "Items");
    }

    #[test]
    fn the_closest_title_comes_first() {
        let all = entries(&items(), false, english);
        let found = matches(&all, "font");
        assert_eq!(found[0].id, "row/1/Font");
        assert_eq!(found[1].id, "row/1/Font size");
    }

    #[test]
    fn only_the_unit_in_use_is_listed() {
        let celsius = entries(&[], false, english);
        let found = ids(&matches(&celsius, "cpu temperature"));
        assert!(found.contains(&"row/7/CPU temperature (°C)".to_string()));
        assert!(!found.contains(&"row/7/CPU temperature (°F)".to_string()));
        let fahrenheit = entries(&[], true, english);
        let found = ids(&matches(&fahrenheit, "cpu temperature"));
        assert!(found.contains(&"row/7/CPU temperature (°F)".to_string()));
        assert!(!found.contains(&"row/7/CPU temperature (°C)".to_string()));
    }

    #[test]
    fn the_only_condition_is_the_one_entries_resolves() {
        for row in INDEX {
            if let Some((condition, _)) = row.when {
                assert_eq!(condition, FAHRENHEIT, "{}", row.title);
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let all = entries(&items(), false, english);
        let unique: HashSet<&str> = all.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn a_page_entry_has_no_row() {
        let all = entries(&[], false, english);
        let page = all
            .iter()
            .find(|e| e.id == "page/2")
            .expect("the theme page");
        assert_eq!(page.title, "Theme");
        assert!(!page.is_row);
    }

    #[test]
    fn descriptions_shown_have_no_placeholders() {
        let all = entries(&[], false, english);
        assert!(all.iter().all(|e| !e.description.contains('{')));
    }

    fn table(pairs: &[(&'static str, &'static str)]) -> HashMap<&'static str, &'static str> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn spanish_and_english_both_match() {
        let es = table(&[("Font", "Fuente"), ("Font size", "Tamaño de fuente")]);
        let all = entries(&[], false, |m| es.get(m).copied().unwrap_or(m));
        for query in ["fuente", "FUENTE", "tamano", "font"] {
            let found = ids(&matches(&all, query));
            assert!(found.contains(&"row/1/Font size".to_string()), "{query}");
        }
        let found = matches(&all, "tamaño fuente");
        assert_eq!(found[0].title, "Tamaño de fuente");
    }

    #[test]
    fn chinese_matches_without_spaces() {
        let zh = table(&[("Font", "字体"), ("Font size", "字体大小")]);
        let all = entries(&[], false, |m| zh.get(m).copied().unwrap_or(m));
        let found = ids(&matches(&all, "字体"));
        assert!(found.contains(&"row/1/Font".to_string()));
        assert!(found.contains(&"row/1/Font size".to_string()));
        let found = ids(&matches(&all, "大小"));
        assert!(found.contains(&"row/1/Font size".to_string()));
        assert!(!found.contains(&"row/1/Font".to_string()));
    }

    /// The only test here that switches the language.
    #[test]
    fn the_catalogs_translate_the_index() {
        language::init(&Config::default());
        for (code, query) in [("es", "fuente"), ("zh-CN", "字体"), ("ko", "글꼴")] {
            l10n::set_language(Some(code));
            let all = entries(&[], false, l10n::translate);
            let found = ids(&matches(&all, query));
            assert!(found.contains(&"row/1/Font".to_string()), "{code}");
            let row = all.iter().find(|e| e.id == "row/1/Font").expect("the row");
            assert_ne!(row.title, "Font", "{code}");

            let mut seen: HashMap<(i32, &str), &str> = HashMap::new();
            for entry in all.iter().filter(|e| e.is_row) {
                let other = seen.insert((entry.tab, entry.title), &entry.id);
                assert_eq!(other, None, "{code}: {} on tab {}", entry.title, entry.tab);
            }
        }
        l10n::set_language(Some("en"));
    }
}
