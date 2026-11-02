#[path = "../settings_index.rs"]
mod settings_index;

use settings_index::{Scanned, generate, scan};

const SOURCE: &str = include_str!("../ui/settings.slint");

fn rows() -> Vec<Scanned> {
    scan(SOURCE)
}

fn titles_in(line: &str) -> Vec<String> {
    line.split("@tr(\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or_default().to_string())
        .collect()
}

/// Reads the titles a different way from the scanner: by lines, from each row's
/// opening to its `title:`.
fn literal_row_titles() -> Vec<String> {
    let start = SOURCE
        .find("export component SettingsWindow")
        .expect("the window");
    let lines: Vec<&str> = SOURCE[start..].lines().map(str::trim).collect();
    let mut titles = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.ends_with("SettingRow {") || line.ends_with("OptionalRow {") {
            let title = lines[i + 1..]
                .iter()
                .find(|l| l.starts_with("title:"))
                .expect("a row has a title");
            titles.extend(titles_in(title));
        }
    }
    titles
}

#[test]
fn the_scan_covers_every_literal_row_title() {
    let expected = literal_row_titles();
    assert!(expected.len() > 50, "only {} titles", expected.len());
    let scanned: Vec<String> = rows()
        .into_iter()
        .filter(|r| !r.is_page)
        .map(|r| r.title)
        .collect();
    for title in &expected {
        assert!(scanned.contains(title), "not scanned: {title}");
    }
    assert_eq!(scanned.len(), expected.len());
}

#[test]
fn rows_know_their_page_and_section() {
    let rows = rows();
    let font = rows.iter().find(|r| r.title == "Font").expect("the row");
    assert_eq!(
        (font.tab, font.page.as_str(), font.section.as_str()),
        (1, "Layout", "Text")
    );
    assert!(font.descriptions[0].starts_with("Fonts without"));
    let mode = rows.iter().find(|r| r.title == "Mode").expect("the row");
    assert_eq!((mode.section.as_str(), mode.keywords.len()), ("", 2));
}

#[test]
fn every_page_but_the_results_is_listed() {
    let mut tabs: Vec<i32> = rows().iter().filter(|r| r.is_page).map(|r| r.tab).collect();
    tabs.sort_unstable();
    assert_eq!(tabs, (0..=9).collect::<Vec<i32>>());
}

#[test]
fn a_page_entry_keeps_the_page_text() {
    let rows = rows();
    let theme = rows
        .iter()
        .find(|r| r.is_page && r.tab == 2)
        .expect("the page");
    assert_eq!(theme.title, "Theme");
    assert!(theme.descriptions[0].starts_with("A theme sets"));
}

#[test]
fn optional_and_conditional_rows_are_listed() {
    let rows = rows();
    for title in [
        "Single-line graphs",
        "Lock position",
        "Drives",
        "Drive value",
        "Graph style",
        "Tile width",
        "Tile height",
    ] {
        assert!(rows.iter().any(|r| r.title == title), "{title}");
    }
}

#[test]
fn the_rows_of_the_item_template_belong_to_every_item() {
    let per_item: Vec<String> = rows()
        .into_iter()
        .filter(|r| r.per_item)
        .map(|r| r.title)
        .collect();
    assert_eq!(
        per_item,
        [
            "Graph style",
            "Tile width",
            "Tile height",
            "Colour",
            "Popup detail",
            "Popup width",
            "Popup graph height"
        ]
    );
    assert!(rows().iter().all(|r| !r.per_item || r.tab == 0));
    assert!(!rows().iter().any(|r| r.per_item && r.is_page));
}

#[test]
fn the_rows_of_the_item_template_answer_to_their_search_ids() {
    for row in rows().into_iter().filter(|r| r.per_item) {
        let key = format!("search-key: row.id + \"/{}\";", row.title);
        assert!(SOURCE.contains(&key), "{key}");
    }
}

#[test]
fn only_the_item_template_makes_rows_per_item() {
    let source = r#"
        export component SettingsWindow inherits Window {
            if root.tab == 0: Page {
                title: @tr("Items");
                SettingRow { title: @tr("Before"); }
                for row[i] in root.items: VerticalLayout {
                    if row.expanded: VerticalLayout {
                        SettingRow { title: @tr("Inside"); }
                    }
                }
                SettingRow { title: @tr("After"); }
                for other[i] in root.others: VerticalLayout {
                    SettingRow { title: @tr("Elsewhere"); }
                }
            }
        }
    "#;
    let found: Vec<(String, bool)> = scan(source)
        .into_iter()
        .filter(|r| !r.is_page)
        .map(|r| (r.title, r.per_item))
        .collect();
    assert_eq!(
        found,
        [
            ("Before".to_string(), false),
            ("Inside".to_string(), true),
            ("After".to_string(), false),
            ("Elsewhere".to_string(), false),
        ]
    );
}

#[test]
fn rows_without_a_literal_title_are_left_to_the_code() {
    let source = r#"
        export component SettingsWindow inherits Window {
            if root.tab == 3: Page {
                title: @tr("Page");
                for row[i] in root.items: VerticalLayout {
                    SettingRow { title: row.name; Button { text: @tr("Open"); } }
                }
            }
        }
    "#;
    let rows = scan(source);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_page);
}

#[test]
fn a_conditional_title_makes_one_entry_per_message() {
    let source = r#"
        export component SettingsWindow inherits Window {
            if root.tab == 7: Page {
                title: @tr("General");
                SectionTitle { text: @tr("Alerts"); }
                Card {
                    SettingRow {
                        title: root.alert-fahrenheit ? @tr("F") : @tr("C");
                        description: root.x ? @tr("yes {}", 1) : @tr("no");
                        Switch { checked <=> root.y; toggled => { root.changed(); } }
                    }
                }
            }
        }
    "#;
    let rows = scan(source);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[1].when,
        Some(("root.alert-fahrenheit".to_string(), true))
    );
    assert_eq!((rows[1].title.as_str(), rows[2].title.as_str()), ("F", "C"));
    assert_eq!(rows[2].section, "Alerts");
    assert_eq!(rows[2].descriptions, ["yes {}", "no"]);
}

#[test]
fn braces_and_semicolons_in_strings_and_comments_are_ignored() {
    let source = r#"
        export component SettingsWindow inherits Window {
            // if root.tab == 1: Page { SettingRow {
            /* } */
            if root.tab == 4: Page {
                title: @tr("Page; \"one\" {");
                SettingRow {
                    title: @tr("A row");
                    description: "\{Math.round(root.x)}%; }";
                }
            }
        }
    "#;
    let rows = scan(source);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].title, "Page; \"one\" {");
    assert_eq!(rows[1].page, "Page; \"one\" {");
    assert_eq!(rows[1].title, "A row");
    assert!(rows[1].descriptions.is_empty());
}

#[test]
fn the_generated_code_escapes_its_text() {
    let code = generate(&[Scanned {
        title: "say \"hi\"".to_string(),
        when: Some(("root.a".to_string(), false)),
        ..Scanned::default()
    }]);
    assert!(code.contains(r#"title: "say \"hi\"""#));
    assert!(code.contains(r#"when: Some(("root.a", false))"#));
    assert!(code.contains("per_item: false"));
}
