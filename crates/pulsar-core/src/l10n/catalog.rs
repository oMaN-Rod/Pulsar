//! Keeps a crate's catalogs in step with its sources. Each crate's tests
//! call [`sync`]; with `PULSAR_UPDATE_L10N=1` set it rewrites the template
//! and catalogs instead of failing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{LANGUAGES, po};

pub const UPDATE_VAR: &str = "PULSAR_UPDATE_L10N";

const TEMPLATE_HEADER: &str = "Project-Id-Version: Pulsar\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n";

#[derive(Debug, PartialEq)]
pub struct Message {
    pub msgid: String,
    /// The files using it, relative to the crate.
    pub files: Vec<String>,
}

/// Checks (or, with [`UPDATE_VAR`] set, rewrites) `locales/<domain>.pot`
/// and every `locales/<code>/LC_MESSAGES/<domain>.po` under `root`.
pub fn sync(root: &Path, domain: &str, dirs: &[&str]) -> Result<(), String> {
    let messages = extract(root, dirs)?;
    let locales = root.join("locales");
    let mut wanted = vec![(locales.join(format!("{domain}.pot")), template(&messages))];
    let mut problems = Vec::new();
    for (code, path) in catalogs(&locales, domain)? {
        if !LANGUAGES.iter().skip(1).any(|l| l.code == code) {
            problems.push(format!(
                "{code} has a catalog but is not in l10n::LANGUAGES"
            ));
        }
        let existing = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for entry in po::parse(&existing) {
            if !entry.msgstr.is_empty() && placeholders(&entry.msgid) != placeholders(&entry.msgstr)
            {
                problems.push(format!(
                    "{code}: \"{}\" has different placeholders from its translation",
                    entry.msgid
                ));
            }
        }
        wanted.push((path, merge(&existing, &messages)));
    }
    for language in LANGUAGES.iter().skip(1) {
        if !wanted
            .iter()
            .any(|(p, _)| p.starts_with(locales.join(language.code)))
        {
            problems.push(format!("{} has no {domain}.po", language.code));
        }
    }
    let update = std::env::var_os(UPDATE_VAR).is_some();
    for (path, text) in wanted {
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current.replace("\r\n", "\n") == text {
            continue;
        }
        if update {
            fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            problems.push(format!(
                "{} is out of date; run the tests with {UPDATE_VAR}=1",
                path.display()
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn catalogs(locales: &Path, domain: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let mut found = Vec::new();
    let entries = fs::read_dir(locales).map_err(|e| format!("{}: {e}", locales.display()))?;
    for entry in entries.flatten() {
        let path = entry
            .path()
            .join("LC_MESSAGES")
            .join(format!("{domain}.po"));
        if path.is_file() {
            found.push((entry.file_name().to_string_lossy().into_owned(), path));
        }
    }
    found.sort();
    Ok(found)
}

/// Every message the `.rs` and `.slint` files under `dirs` ask for, in
/// order of first use.
pub fn extract(root: &Path, dirs: &[&str]) -> Result<Vec<Message>, String> {
    let mut files = Vec::new();
    for dir in dirs {
        collect_files(&root.join(dir), &mut files)?;
    }
    files.sort();
    let mut messages: Vec<Message> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for msgid in messages_in(&text) {
            match index.get(&msgid) {
                Some(&i) => {
                    let files = &mut messages[i].files;
                    if !files.contains(&relative) {
                        files.push(relative.clone());
                    }
                }
                None => {
                    index.insert(msgid.clone(), messages.len());
                    messages.push(Message {
                        msgid,
                        files: vec![relative.clone()],
                    });
                }
            }
        }
    }
    Ok(messages)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs" || e == "slint") {
            out.push(path);
        }
    }
    Ok(())
}

/// The macro calls whose literal is a message; spelled out of pieces so
/// this file does not match them itself.
const MARKERS: [&str; 3] = [
    concat!("tr", "!("),
    concat!("tr_noop", "!("),
    concat!("@tr", "("),
];

/// The literals passed to `tr!`, `tr_noop!` and `@tr`, outside comments.
fn messages_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in MARKERS {
        let mut from = 0;
        while let Some(i) = text[from..].find(marker).map(|i| i + from) {
            from = i + marker.len();
            let before = text[..i].chars().next_back();
            if !marker.starts_with('@') && before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
            if text[line_start..i].contains("//") {
                continue;
            }
            if let Some(msgid) = literal(text[from..].trim_start()) {
                found.push((i, msgid));
            }
        }
    }
    found.sort_by_key(|&(i, _)| i);
    found.into_iter().map(|(_, m)| m).collect()
}

/// The value of the string literal `s` starts with.
fn literal(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('"')?.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                'u' => {
                    let code: String = chars
                        .by_ref()
                        .skip_while(|&c| c == '{')
                        .take_while(|&c| c != '}')
                        .collect();
                    out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
                }
                'x' => {
                    let code: String = chars.by_ref().take(2).collect();
                    out.push(char::from(u8::from_str_radix(&code, 16).ok()?));
                }
                '\n' | '\r' => while chars.next_if(|c| c.is_whitespace()).is_some() {},
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// The arguments a template uses, by position.
fn placeholders(s: &str) -> Vec<usize> {
    let mut used = Vec::new();
    let mut next = 0;
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        if let Some(after) = tail.strip_prefix("{{") {
            rest = after;
            continue;
        }
        let inner = tail[1..].find('}').map(|end| &tail[1..1 + end]);
        match inner {
            Some("") => {
                used.push(next);
                next += 1;
            }
            Some(digits) if digits.bytes().all(|b| b.is_ascii_digit()) => {
                used.extend(digits.parse::<usize>().ok());
            }
            _ => {}
        }
        rest = &tail[1..];
    }
    used.sort_unstable();
    used
}

fn header_block(header: &str) -> String {
    format!("{}{}", po::field("msgid", ""), po::field("msgstr", header))
}

fn entry_block(m: &Message, msgstr: &str, fuzzy: bool) -> String {
    let mut out = format!("\n#: {}\n", m.files.join(" "));
    if fuzzy {
        out.push_str("#, fuzzy\n");
    }
    out.push_str(&po::field("msgid", &m.msgid));
    out.push_str(&po::field("msgstr", msgstr));
    out
}

/// The `.pot` template: every message, untranslated.
pub fn template(messages: &[Message]) -> String {
    let mut out = String::from("# Template for Pulsar's translations; generated by the tests.\n");
    out.push_str(&header_block(TEMPLATE_HEADER));
    for m in messages {
        out.push_str(&entry_block(m, "", false));
    }
    out
}

/// `existing` with exactly `messages`: new ones untranslated, removed ones
/// dropped, and its leading comments, header and translations kept.
pub fn merge(existing: &str, messages: &[Message]) -> String {
    let entries = po::parse(existing);
    let header = entries
        .iter()
        .find(|e| e.msgid.is_empty())
        .map_or(TEMPLATE_HEADER, |e| &e.msgstr);
    let mut out: String = existing
        .lines()
        .take_while(|l| l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect();
    out.push_str(&header_block(header));
    for m in messages {
        let old = entries.iter().find(|e| e.msgid == m.msgid);
        out.push_str(&entry_block(
            m,
            old.map_or("", |e| &e.msgstr),
            old.is_some_and(|e| e.fuzzy),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(template: &str) -> String {
        template
            .replace("TR(", MARKERS[0])
            .replace("NOOP(", MARKERS[1])
            .replace("AT(", MARKERS[2])
    }

    #[test]
    fn finds_literals_in_code_but_not_comments() {
        let text = source(
            r#"
            let a = TR("Settings");
            let b = TR(
                "Pulsar {} is available",
                version,
            );
            // TR("commented out")
            let c = NOOP("Load");
            let d = xTR("not ours");
            text: AT("Line\nbreak \"quoted\" \u{2026}");
            "#,
        );
        assert_eq!(
            messages_in(&text),
            [
                "Settings",
                "Pulsar {} is available",
                "Load",
                "Line\nbreak \"quoted\" …"
            ]
        );
    }

    #[test]
    fn placeholders_compare_by_position() {
        assert_eq!(placeholders("{} of {}"), [0, 1]);
        assert_eq!(placeholders("{1} de {0}"), [0, 1]);
        assert_eq!(placeholders("{{not}} {}"), [0]);
        assert_ne!(placeholders("{} of {}"), placeholders("de {}"));
    }

    #[test]
    fn merging_keeps_translations_and_follows_the_sources() {
        let existing = "# Spanish\nmsgid \"\"\nmsgstr \"Language: es\\n\"\n\n#: old.rs\nmsgid \"Keep\"\nmsgstr \"Guardar\"\n\nmsgid \"Gone\"\nmsgstr \"Ido\"\n";
        let messages = [
            Message {
                msgid: "New".into(),
                files: vec!["src/a.rs".into()],
            },
            Message {
                msgid: "Keep".into(),
                files: vec!["src/a.rs".into(), "ui/b.slint".into()],
            },
        ];
        let merged = merge(existing, &messages);
        assert_eq!(
            merged,
            "# Spanish\nmsgid \"\"\nmsgstr \"Language: es\\n\"\n\n#: src/a.rs\nmsgid \"New\"\nmsgstr \"\"\n\n#: src/a.rs ui/b.slint\nmsgid \"Keep\"\nmsgstr \"Guardar\"\n"
        );
        assert_eq!(merge(&merged, &messages), merged, "stable once merged");
    }
}
