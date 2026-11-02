//! Reads the settings pages out of `settings.slint` for the search index, so
//! the index cannot drift from the window. Kept outside `src`, because the
//! catalog tooling would take the sample text in its tests for messages.

const TRANSLATED: &str = "@tr(";
const WINDOW: &str = "export component SettingsWindow";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scanned {
    pub tab: i32,
    pub page: String,
    pub section: String,
    pub title: String,
    /// For `cond ? @tr("a") : @tr("b")`: the condition, and its value for this title.
    pub when: Option<(String, bool)>,
    pub descriptions: Vec<String>,
    /// Other text in the row, such as its choices and buttons.
    pub keywords: Vec<String>,
    pub is_page: bool,
}

enum Frame {
    Page,
    Row,
    Section,
    Text,
    Other,
}

#[derive(Default)]
struct Scan {
    rows: Vec<Scanned>,
    frames: Vec<Frame>,
    page: Option<usize>,
    section: String,
    /// The titles of the open row, with the condition choosing between them.
    titles: Vec<(String, Option<(String, bool)>)>,
    row: Option<Scanned>,
}

impl Scan {
    fn open(&mut self, header: &str) {
        let name = header.rsplit([' ', ':', '=']).next().unwrap_or_default();
        let frame = match name {
            "Page" => match tab_of(header).filter(|&tab| tab >= 0) {
                Some(tab) => {
                    self.page = Some(self.rows.len());
                    self.section.clear();
                    self.rows.push(Scanned {
                        tab,
                        is_page: true,
                        ..Scanned::default()
                    });
                    Frame::Page
                }
                None => Frame::Other,
            },
            "SettingRow" | "OptionalRow" if self.page.is_some() => {
                assert!(self.row.is_none(), "a row inside a row: {header}");
                self.row = Some(Scanned::default());
                Frame::Row
            }
            "SectionTitle" => Frame::Section,
            "Text" => Frame::Text,
            _ => Frame::Other,
        };
        self.frames.push(frame);
    }

    fn close(&mut self) {
        match self.frames.pop() {
            Some(Frame::Page) => self.page = None,
            Some(Frame::Row) => self.finish_row(),
            _ => {}
        }
    }

    fn finish_row(&mut self) {
        let row = self.row.take().expect("a row frame has a row");
        let Some(page) = self.page.map(|i| &self.rows[i]) else {
            return;
        };
        let (tab, page) = (page.tab, page.title.clone());
        for (title, when) in std::mem::take(&mut self.titles) {
            self.rows.push(Scanned {
                tab,
                page: page.clone(),
                section: self.section.clone(),
                title,
                when,
                ..row.clone()
            });
        }
    }

    fn statement(&mut self, text: &str) {
        let text = text.trim();
        let messages = messages_in(text);
        if messages.is_empty() {
            return;
        }
        let top = self.frames.last();
        if let Some(row) = &mut self.row {
            if matches!(top, Some(Frame::Row)) && text.starts_with("title:") {
                self.titles = titles_of(text, messages);
            } else if matches!(top, Some(Frame::Row)) && text.starts_with("description:") {
                row.descriptions.extend(messages);
            } else {
                row.keywords.extend(messages);
            }
            return;
        }
        let Some(page) = self.page else { return };
        match top {
            Some(Frame::Page) if text.starts_with("title:") => {
                let title = messages.into_iter().next().expect("not empty");
                self.rows[page].page.clone_from(&title);
                self.rows[page].title = title;
            }
            Some(Frame::Section) if text.starts_with("text:") => {
                self.section = messages.into_iter().next().expect("not empty");
            }
            Some(Frame::Text) if text.starts_with("text:") => {
                self.rows[page].descriptions.extend(messages);
            }
            _ => {}
        }
    }
}

fn tab_of(header: &str) -> Option<i32> {
    let (_, rest) = header.split_once("root.tab ==")?;
    rest.trim().split(':').next()?.trim().parse().ok()
}

fn titles_of(statement: &str, messages: Vec<String>) -> Vec<(String, Option<(String, bool)>)> {
    let mut messages = messages.into_iter();
    match (messages.next(), messages.next(), messages.next()) {
        (Some(title), None, _) => vec![(title, None)],
        (Some(first), Some(second), None) => {
            let (condition, _) = statement
                .strip_prefix("title:")
                .and_then(|s| s.split_once('?'))
                .unwrap_or_else(|| panic!("a title with two messages and no `?`: {statement}"));
            let condition = condition.trim().to_string();
            vec![
                (first, Some((condition.clone(), true))),
                (second, Some((condition, false))),
            ]
        }
        _ => panic!("a title with more than two messages: {statement}"),
    }
}

fn messages_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(TRANSLATED) {
        rest = rest[i + TRANSLATED.len()..].trim_start();
        let Some(body) = rest.strip_prefix('"') else {
            continue;
        };
        let mut message = String::new();
        let mut chars = body.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => {
                    rest = &body[at + 1..];
                    break;
                }
                '\\' => match chars.next() {
                    Some((_, 'n')) => message.push('\n'),
                    Some((_, other)) => message.push(other),
                    None => {}
                },
                c => message.push(c),
            }
        }
        found.push(message);
    }
    found
}

/// Every page of the window and every row of them with a literal title, in
/// source order.
pub fn scan(source: &str) -> Vec<Scanned> {
    let start = source
        .find(WINDOW)
        .unwrap_or_else(|| panic!("no `{WINDOW}` in the source"));
    let mut scan = Scan::default();
    let mut text = String::new();
    let mut chars = source[start..].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                text.push(c);
                while let Some(c) = chars.next() {
                    text.push(c);
                    match c {
                        '\\' => text.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                let mut previous = ' ';
                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            '{' => {
                scan.open(text.trim());
                text.clear();
            }
            '}' => {
                scan.statement(&text);
                scan.close();
                text.clear();
            }
            ';' => {
                scan.statement(&text);
                text.clear();
            }
            c => text.push(c),
        }
    }
    scan.rows
}

pub fn generate(rows: &[Scanned]) -> String {
    let mut out = String::from("&[\n");
    for row in rows {
        let when = match &row.when {
            Some((condition, value)) => format!("Some(({condition:?}, {value}))"),
            None => "None".to_string(),
        };
        out.push_str(&format!(
            "    IndexRow {{ tab: {}, page: {:?}, section: {:?}, title: {:?}, when: {when}, descriptions: &{:?}, keywords: &{:?}, is_page: {} }},\n",
            row.tab, row.page, row.section, row.title, row.descriptions, row.keywords, row.is_page,
        ));
    }
    out.push_str("]\n");
    out
}
