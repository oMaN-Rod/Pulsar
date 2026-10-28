//! Reading and writing gettext `.po` catalogs: the subset Pulsar uses,
//! without plurals or contexts.

use std::borrow::Cow;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry<'a> {
    pub context: Option<Cow<'a, str>>,
    pub msgid: Cow<'a, str>,
    pub msgstr: Cow<'a, str>,
    pub fuzzy: bool,
    pub plural: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Context,
    Id,
    Str,
    Other,
}

/// Every entry in the file, the header (empty `msgid`) included.
pub fn parse(text: &str) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    let mut entry = Entry::default();
    let mut field = Field::None;
    let mut started = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            finish(&mut entry, &mut started, &mut entries);
            field = Field::None;
            continue;
        }
        if let Some(flags) = line.strip_prefix("#,") {
            if started && field != Field::None {
                finish(&mut entry, &mut started, &mut entries);
            }
            entry.fuzzy |= flags.split(',').any(|f| f.trim() == "fuzzy");
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (keyword, rest) = match line.find(|c: char| c.is_whitespace()) {
            Some(i) if !line.starts_with('"') => (&line[..i], line[i..].trim_start()),
            _ => ("", line),
        };
        let value = quoted(rest);
        match keyword {
            "" => {
                let value = value.unwrap_or_default();
                let target = match field {
                    Field::Context => entry.context.get_or_insert_default(),
                    Field::Id => &mut entry.msgid,
                    Field::Str => &mut entry.msgstr,
                    Field::None | Field::Other => continue,
                };
                append(target, value);
            }
            "msgctxt" => {
                if started && field != Field::Context {
                    finish(&mut entry, &mut started, &mut entries);
                }
                started = true;
                entry.context = Some(value.unwrap_or_default());
                field = Field::Context;
            }
            "msgid" => {
                if started && field != Field::Context {
                    finish(&mut entry, &mut started, &mut entries);
                }
                started = true;
                entry.msgid = value.unwrap_or_default();
                field = Field::Id;
            }
            "msgid_plural" => {
                entry.plural = true;
                field = Field::Other;
            }
            "msgstr" | "msgstr[0]" => {
                entry.msgstr = value.unwrap_or_default();
                field = Field::Str;
            }
            _ => field = Field::Other,
        }
    }
    finish(&mut entry, &mut started, &mut entries);
    entries
}

fn finish<'a>(entry: &mut Entry<'a>, started: &mut bool, entries: &mut Vec<Entry<'a>>) {
    if std::mem::take(started) {
        entries.push(std::mem::take(entry));
    }
}

fn append<'a>(target: &mut Cow<'a, str>, more: Cow<'a, str>) {
    if target.is_empty() {
        *target = more;
    } else if !more.is_empty() {
        target.to_mut().push_str(&more);
    }
}

/// The text of a `"…"` token, unescaped.
fn quoted(token: &str) -> Option<Cow<'_, str>> {
    let inner = token.strip_prefix('"')?.strip_suffix('"')?;
    if !inner.contains('\\') {
        return Some(Cow::Borrowed(inner));
    }
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    Some(Cow::Owned(out))
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// `keyword "text"`, split after each line break the way gettext tools do.
pub fn field(keyword: &str, text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.len() <= 1 {
        return format!("{keyword} \"{}\"\n", escape(text));
    }
    let mut out = format!("{keyword} \"\"\n");
    for line in lines {
        out.push_str(&format!("\"{}\"\n", escape(line)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_with_continuations_and_escapes() {
        let text = r#"
# A comment
msgid ""
msgstr ""
"Language: es\n"

#: src/app.rs
msgid "Settings"
msgstr "Configuración"

msgid ""
"Two\n"
"lines"
msgstr "Dos\nlíneas \"aquí\""

#, fuzzy
msgid "Unsure"
msgstr "Dudoso"
"#;
        let entries = parse(text);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].msgid, "");
        assert_eq!(entries[0].msgstr, "Language: es\n");
        assert_eq!(
            (&*entries[1].msgid, &*entries[1].msgstr),
            ("Settings", "Configuración")
        );
        assert!(matches!(entries[1].msgstr, Cow::Borrowed(_)));
        assert_eq!(entries[2].msgid, "Two\nlines");
        assert_eq!(entries[2].msgstr, "Dos\nlíneas \"aquí\"");
        assert!(entries[3].fuzzy && !entries[2].fuzzy);
    }

    #[test]
    fn written_fields_read_back() {
        for text in ["plain", "with \"quotes\" and \\", "two\nlines\n", ""] {
            let po = format!("{}{}", field("msgid", "x"), field("msgstr", text));
            assert_eq!(parse(&po)[0].msgstr, text, "{po}");
        }
        assert_eq!(field("msgid", "a\nb"), "msgid \"\"\n\"a\\n\"\n\"b\"\n");
    }

    #[test]
    fn contexts_and_plurals_are_marked() {
        let text = "msgctxt \"menu\"\nmsgid \"Open\"\nmsgstr \"Abrir\"\n\nmsgid \"file\"\nmsgid_plural \"files\"\nmsgstr[0] \"archivo\"\nmsgstr[1] \"archivos\"\n";
        let entries = parse(text);
        assert_eq!(entries[0].context.as_deref(), Some("menu"));
        assert_eq!(entries[0].msgstr, "Abrir");
        assert!(entries[1].plural);
        assert_eq!(entries[1].msgstr, "archivo");
    }
}
