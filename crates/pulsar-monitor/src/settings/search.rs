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
    /// Every item has this row, under its options.
    pub per_item: bool,
}

const INDEX: &[IndexRow] = include!(concat!(env!("OUT_DIR"), "/settings_index.rs"));

/// The one condition the index may carry; [`entries`] resolves it.
const FAHRENHEIT: &str = "root.alert-fahrenheit";
const SEPARATOR: &str = " › ";
/// The worst [`rank`] of a match in an entry's title.
const TITLE_RANK: u32 = 1;

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
    /// The id of the item whose row this is. Such a row is found by its id,
    /// because every item repeats the same titles.
    pub owner: Option<String>,
    title_text: String,
    place_text: String,
    other_text: String,
    /// The owning item's name, on a row under an item's options.
    item_text: String,
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
        .filter(|row| !row.per_item)
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
            owner: Some(item.id.clone()),
            title_text: texts([&item.name], &translate),
            place_text: texts([&page], &translate),
            other_text: String::new(),
            item_text: String::new(),
        });
        found.extend(
            INDEX
                .iter()
                .filter(|row| row.per_item)
                .map(|row| item_row_entry(row, item, &translate)),
        );
    }
    found
}

fn item_row_entry(
    row: &IndexRow,
    item: &Item,
    translate: &impl Fn(&'static str) -> &'static str,
) -> Entry {
    Entry {
        id: format!("{}/{}", item.id, row.title),
        path: format!("{}{SEPARATOR}{}", translate(row.page), translate(item.name)),
        owner: Some(item.id.clone()),
        place_text: texts([&row.page, &item.name], translate),
        item_text: texts([&item.name], translate),
        ..index_entry(row, translate)
    }
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
        owner: None,
        title_text: texts([&row.title], translate),
        place_text: texts([&row.page, &row.section], translate),
        other_text: texts(row.descriptions.iter().chain(row.keywords), translate),
        item_text: String::new(),
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
/// A row under an item's options needs a word of its own title, so naming an
/// item doesn't list all its rows, and comes last unless the query names its item.
pub fn matches<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let query = fold(query);
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(bool, u32, &Entry)> = entries
        .iter()
        .filter_map(|entry| {
            let ranks: Vec<u32> = tokens
                .iter()
                .map(|t| rank(entry, t))
                .collect::<Option<_>>()?;
            let under_item = !entry.item_text.is_empty();
            if under_item && ranks.iter().all(|&rank| rank > TITLE_RANK) {
                return None;
            }
            let unnamed = under_item && !tokens.iter().any(|t| entry.item_text.contains(t));
            Some((unnamed, ranks.iter().sum(), entry))
        })
        .collect();
    ranked.sort_by_key(|&(unnamed, rank, entry)| (unnamed, rank, entry.title.chars().count()));
    ranked.into_iter().map(|(_, _, entry)| entry).collect()
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

    const ITEM_ROWS: [&str; 9] = [
        "Overlay label",
        "Short overlay label",
        "Graph style",
        "Tile width",
        "Tile height",
        "Color",
        "Popup detail",
        "Popup width",
        "Popup graph height",
    ];

    #[test]
    fn every_item_has_each_of_its_rows() {
        let all = entries(&items(), false, english);
        for item in items() {
            for title in ITEM_ROWS {
                let id = format!("{}/{title}", item.id);
                let entry = all.iter().find(|e| e.id == id).expect(&id);
                assert_eq!(entry.title, title);
                assert_eq!(entry.path, format!("Items › {}", item.name));
                assert_eq!(entry.tab, 0);
                assert!(entry.is_row);
                assert_eq!(entry.owner.as_ref(), Some(&item.id));
            }
        }
    }

    #[test]
    fn the_items_page_lists_only_the_load_thresholds_as_plain_rows() {
        let all = entries(&items(), false, english);
        let plain: Vec<&str> = all
            .iter()
            .filter(|e| e.id.starts_with("row/0/"))
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(
            plain,
            [
                "row/0/Percent (%)",
                "row/0/Temperature (°C)",
                "row/0/Ping (ms)"
            ]
        );
        let found = ids(&matches(&all, "thresholds"));
        for id in plain {
            assert!(found.contains(&id.to_string()), "{id}");
        }
    }

    #[test]
    fn rows_of_the_other_pages_keep_their_ids_and_have_no_owner() {
        let all = entries(&items(), false, english);
        let row = all.iter().find(|e| e.id == "row/3/Width").unwrap();
        assert_eq!(row.owner, None);
        assert!(row.path.starts_with("Popups"), "{}", row.path);
    }

    #[test]
    fn naming_an_item_does_not_list_its_rows() {
        let all = entries(&items(), false, english);
        for query in ["memory", "items", "items memory"] {
            let found = ids(&matches(&all, query));
            assert!(
                !found.iter().any(|id| id.starts_with("item/Ram/")),
                "{query}: {found:?}"
            );
        }
        assert_eq!(ids(&matches(&all, "memory"))[0], "item/Ram");
    }

    #[test]
    fn a_word_of_the_title_finds_the_row_of_every_item() {
        let all = entries(&items(), false, english);
        let found = ids(&matches(&all, "graph style"));
        assert!(found.contains(&"item/Cpu/Graph style".to_string()));
        assert!(found.contains(&"item/Ram/Graph style".to_string()));
    }

    #[test]
    fn tile_sizes_are_found_for_every_item_and_can_be_narrowed() {
        let all = entries(&items(), false, english);
        for title in ["Tile width", "Tile height"] {
            let found = ids(&matches(&all, title));
            assert!(found.contains(&format!("item/Cpu/{title}")), "{found:?}");
            assert!(found.contains(&format!("item/Ram/{title}")), "{found:?}");
            let found = ids(&matches(&all, &format!("memory {title}")));
            assert_eq!(found, [format!("item/Ram/{title}")]);
        }
        let row = all.iter().find(|e| e.id == "item/Ram/Tile width").unwrap();
        assert!(row.description.contains("cover taskbar buttons"));
    }

    #[test]
    fn overlay_labels_are_found_without_being_listed_by_hand() {
        let all = entries(&items(), false, english);
        for title in ["Overlay label", "Short overlay label"] {
            let found = ids(&matches(&all, title));
            assert!(found.contains(&format!("item/Cpu/{title}")), "{found:?}");
            assert!(found.contains(&format!("item/Ram/{title}")), "{found:?}");
            let found = ids(&matches(&all, &format!("memory {title}")));
            assert_eq!(found[0], format!("item/Ram/{title}"));
            assert!(!found.iter().any(|id| id.starts_with("item/Cpu/")));
        }
        let row = all
            .iter()
            .find(|e| e.id == "item/Ram/Overlay label")
            .unwrap();
        assert!(row.description.contains("Settings and popups keep"));
        let row = all
            .iter()
            .find(|e| e.id == "item/Ram/Short overlay label")
            .unwrap();
        assert!(row.description.contains("short labels"));
        let found = ids(&matches(&all, "download label"));
        assert_eq!(found[0], "row/6/Download label");
        let found = ids(&matches(&all, "upload label"));
        assert_eq!(found[0], "row/6/Upload label");
        let row = all.iter().find(|e| e.id == "row/6/Upload label").unwrap();
        assert!(row.description.contains("Replaces the arrow"));
    }

    #[test]
    fn naming_the_item_narrows_a_row_to_it() {
        let all = entries(&items(), false, english);
        let found = ids(&matches(&all, "memory popup width"));
        assert_eq!(found, ["item/Ram/Popup width"]);
        let found = ids(&matches(&all, "cpu color"));
        assert_eq!(found[0], "item/Cpu/Color");
        assert!(!found.contains(&"item/Ram/Color".to_string()));
    }

    #[test]
    fn rows_under_items_come_after_the_other_results() {
        let all = entries(&items(), false, english);
        let found = matches(&all, "popup width");
        assert_eq!(found[0].id, "row/3/Width");
        let first = found.iter().position(|e| e.owner.is_some()).unwrap();
        assert!(found[first..].iter().all(|e| e.owner.is_some()));
        assert_eq!(found.len() - first, items().len());
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
            for entry in all.iter().filter(|e| e.is_row && e.owner.is_none()) {
                let other = seen.insert((entry.tab, entry.title), &entry.id);
                assert_eq!(other, None, "{code}: {} on tab {}", entry.title, entry.tab);
            }
        }

        for (code, width, height) in [
            ("es", "ancho del mosaico", "alto del mosaico"),
            ("zh-CN", "磁贴宽度", "磁贴高度"),
            ("ko", "타일 너비", "타일 높이"),
        ] {
            l10n::set_language(Some(code));
            let all = entries(&items(), false, l10n::translate);
            let found = ids(&matches(&all, width));
            assert!(found.contains(&"item/Ram/Tile width".to_string()), "{code}");
            let found = ids(&matches(&all, height));
            assert!(
                found.contains(&"item/Cpu/Tile height".to_string()),
                "{code}"
            );
        }

        for (code, label, short, download) in [
            (
                "es",
                "etiqueta en la superposicion",
                "etiqueta corta",
                "etiqueta de descarga",
            ),
            ("zh-CN", "叠加层标签", "叠加层短标签", "下载标签"),
            (
                "ko",
                "오버레이 레이블",
                "오버레이 짧은 레이블",
                "다운로드 레이블",
            ),
        ] {
            l10n::set_language(Some(code));
            let all = entries(&items(), false, l10n::translate);
            let found = ids(&matches(&all, label));
            assert!(
                found.contains(&"item/Ram/Overlay label".to_string()),
                "{code}"
            );
            let found = ids(&matches(&all, short));
            assert!(
                found.contains(&"item/Cpu/Short overlay label".to_string()),
                "{code}"
            );
            assert_eq!(
                ids(&matches(&all, download))[0],
                "row/6/Download label",
                "{code}"
            );
        }

        for (code, query, item_query) in [
            ("es", "ancho de la ventana", "memoria ancho de la ventana"),
            ("zh-CN", "弹出窗口宽度", "内存 弹出窗口宽度"),
            ("ko", "팝업 너비", "메모리 팝업 너비"),
        ] {
            l10n::set_language(Some(code));
            let all = entries(&items(), false, l10n::translate);
            let row = all.iter().find(|e| e.id == "item/Ram/Popup width").unwrap();
            assert_ne!(row.title, "Popup width", "{code}");
            assert_ne!(row.path, "Items › Memory", "{code}");
            let found = ids(&matches(&all, query));
            assert!(
                found.contains(&"item/Cpu/Popup width".to_string()),
                "{code}: {found:?}"
            );
            assert!(
                found.contains(&"item/Ram/Popup width".to_string()),
                "{code}"
            );
            assert_eq!(
                ids(&matches(&all, item_query))[0],
                "item/Ram/Popup width",
                "{code}"
            );
            let english = ids(&matches(&all, "memory popup width"));
            assert_eq!(english[0], "item/Ram/Popup width", "{code}");
        }
        l10n::set_language(Some("en"));
    }
}
