//! A task line held losslessly: every byte of the line lives in exactly one segment, so
//! printing an unedited line gives back the same bytes, and an edit touches only the
//! segment it changes. Unknown emoji and free text stay as text segments.

use crate::types::{Priority, TaskStatus};

/// The fields of SPEC 4.1, in the order HQ writes them when it adds one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Priority,
    Created,
    Start,
    Scheduled,
    Due,
    Recur,
    Done,
    Cancelled,
    Id,
    Depends,
}

impl FieldKind {
    /// Where a newly added field goes relative to the ones already on the line.
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// The emoji HQ writes for this field (priority has one per level instead).
    pub fn signifier(self) -> &'static str {
        match self {
            FieldKind::Priority => "",
            FieldKind::Created => "➕",
            FieldKind::Start => "🛫",
            FieldKind::Scheduled => "⏳",
            FieldKind::Due => "📅",
            FieldKind::Recur => "🔁",
            FieldKind::Done => "✅",
            FieldKind::Cancelled => "❌",
            FieldKind::Id => "🆔",
            FieldKind::Depends => "⛔",
        }
    }

    pub fn is_date(self) -> bool {
        matches!(
            self,
            FieldKind::Created
                | FieldKind::Start
                | FieldKind::Scheduled
                | FieldKind::Due
                | FieldKind::Done
                | FieldKind::Cancelled
        )
    }
}

/// Every emoji the parser understands, including the alternates Obsidian Tasks accepts.
pub const SIGNIFIERS: &[(&str, FieldKind)] = &[
    ("🔺", FieldKind::Priority),
    ("⏫", FieldKind::Priority),
    ("🔼", FieldKind::Priority),
    ("🔽", FieldKind::Priority),
    ("⏬", FieldKind::Priority),
    ("➕", FieldKind::Created),
    ("🛫", FieldKind::Start),
    ("⏳", FieldKind::Scheduled),
    ("⌛", FieldKind::Scheduled),
    ("📅", FieldKind::Due),
    ("📆", FieldKind::Due),
    ("🗓", FieldKind::Due),
    ("🔁", FieldKind::Recur),
    ("✅", FieldKind::Done),
    ("❌", FieldKind::Cancelled),
    ("🆔", FieldKind::Id),
    ("⛔", FieldKind::Depends),
];

pub fn priority_emoji(p: Priority) -> &'static str {
    match p {
        Priority::Highest => "🔺",
        Priority::High => "⏫",
        Priority::Medium => "🔼",
        Priority::Low => "🔽",
        Priority::Lowest => "⏬",
    }
}

pub fn priority_from_emoji(e: &str) -> Option<Priority> {
    Some(match e.trim_end_matches('\u{fe0f}') {
        "🔺" => Priority::Highest,
        "⏫" => Priority::High,
        "🔼" => Priority::Medium,
        "🔽" => Priority::Low,
        "⏬" => Priority::Lowest,
        _ => return None,
    })
}

pub fn status_from_char(c: char) -> TaskStatus {
    match c {
        '/' => TaskStatus::Started,
        'x' | 'X' => TaskStatus::Done,
        '-' => TaskStatus::Cancelled,
        '>' => TaskStatus::Waiting,
        // ' ' and any custom status a theme uses ('!', '?', …) read as open
        _ => TaskStatus::Pending,
    }
}

pub fn status_char(s: TaskStatus) -> char {
    match s {
        TaskStatus::Pending => ' ',
        TaskStatus::Started => '/',
        TaskStatus::Done => 'x',
        TaskStatus::Cancelled => '-',
        TaskStatus::Waiting => '>',
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Text(String),
    Field {
        /// The whitespace before the emoji, so removing a field removes its gap too.
        lead: String,
        kind: FieldKind,
        /// The emoji as written (it may carry a variation selector).
        emoji: String,
        /// Whitespace between the emoji and the value.
        sep: String,
        value: String,
    },
}

impl Seg {
    fn print(&self, out: &mut String) {
        match self {
            Seg::Text(t) => out.push_str(t),
            Seg::Field {
                lead,
                emoji,
                sep,
                value,
                ..
            } => {
                out.push_str(lead);
                out.push_str(emoji);
                out.push_str(sep);
                out.push_str(value);
            }
        }
    }

    fn new_field(kind: FieldKind, value: &str) -> Seg {
        Seg::Field {
            lead: " ".into(),
            kind,
            emoji: kind.signifier().into(),
            sep: " ".into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLine {
    /// Leading whitespace (and `> ` quote markers).
    pub indent: String,
    /// `-`, `*`, `+`, `1.` or `1)`.
    pub bullet: String,
    /// The whitespace between the bullet and `[`.
    pub gap: String,
    pub status: char,
    /// Everything after `]`.
    pub body: Vec<Seg>,
}

impl TaskLine {
    pub fn print(&self) -> String {
        let mut s = String::with_capacity(64);
        s.push_str(&self.indent);
        s.push_str(&self.bullet);
        s.push_str(&self.gap);
        s.push('[');
        s.push(self.status);
        s.push(']');
        for seg in &self.body {
            seg.print(&mut s);
        }
        s
    }

    pub fn status(&self) -> TaskStatus {
        status_from_char(self.status)
    }

    pub fn set_status(&mut self, s: TaskStatus) {
        self.status = status_char(s);
    }

    pub fn get(&self, kind: FieldKind) -> Option<&str> {
        self.body.iter().find_map(|s| match s {
            Seg::Field { kind: k, value, .. } if *k == kind => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn priority(&self) -> Option<Priority> {
        self.body.iter().find_map(|s| match s {
            Seg::Field {
                kind: FieldKind::Priority,
                emoji,
                ..
            } => priority_from_emoji(emoji),
            _ => None,
        })
    }

    pub fn depends(&self) -> Vec<String> {
        self.get(FieldKind::Depends)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Set a field's value (replacing the first, dropping repeats) or remove it (`None`).
    /// A new field goes before the first field that HQ writes after it.
    pub fn set(&mut self, kind: FieldKind, value: Option<&str>) {
        let Some(value) = value else {
            self.body
                .retain(|s| !matches!(s, Seg::Field { kind: k, .. } if *k == kind));
            return;
        };
        let mut seen = false;
        self.body.retain_mut(|s| match s {
            Seg::Field {
                kind: k, value: v, ..
            } if *k == kind => {
                if seen {
                    false
                } else {
                    seen = true;
                    *v = value.to_string();
                    true
                }
            }
            _ => true,
        });
        if !seen {
            self.insert_field(Seg::new_field(kind, value), kind);
        }
    }

    pub fn set_priority(&mut self, p: Option<Priority>) {
        let Some(p) = p else {
            self.set(FieldKind::Priority, None);
            return;
        };
        let emoji = priority_emoji(p);
        let mut seen = false;
        self.body.retain_mut(|s| match s {
            Seg::Field {
                kind: FieldKind::Priority,
                emoji: e,
                ..
            } => {
                if seen {
                    false
                } else {
                    seen = true;
                    *e = emoji.to_string();
                    true
                }
            }
            _ => true,
        });
        if !seen {
            let seg = Seg::Field {
                lead: " ".into(),
                kind: FieldKind::Priority,
                emoji: emoji.into(),
                sep: String::new(),
                value: String::new(),
            };
            self.insert_field(seg, FieldKind::Priority);
        }
    }

    fn insert_field(&mut self, seg: Seg, kind: FieldKind) {
        let at = self
            .body
            .iter()
            .position(|s| matches!(s, Seg::Field { kind: k, .. } if k.rank() > kind.rank()));
        match at {
            Some(i) => self.body.insert(i, seg),
            None => {
                // trailing whitespace stays at the very end of the line
                if let Some(Seg::Text(t)) = self.body.last_mut() {
                    let keep = t.len() - t.trim_end().len();
                    if keep > 0 {
                        let tail = t.split_off(t.len() - keep);
                        let empty = t.is_empty();
                        if empty {
                            self.body.pop();
                        }
                        self.body.push(seg);
                        self.body.push(Seg::Text(tail));
                        return;
                    }
                }
                self.body.push(seg);
            }
        }
    }

    /// All tags in the text, in order, without `#`.
    pub fn tags(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.body {
            if let Seg::Text(t) = s {
                for (_, tag) in tag_spans(t) {
                    if !out.iter().any(|x: &String| x == tag) {
                        out.push(tag.to_string());
                    }
                }
            }
        }
        out
    }

    /// The text without tags and fields, whitespace collapsed.
    pub fn description(&self) -> String {
        let mut words: Vec<&str> = Vec::new();
        for s in &self.body {
            if let Seg::Text(t) = s {
                let spans = tag_spans(t);
                let mut last = 0;
                for (start, tag) in spans {
                    words.extend(t[last..start].split_whitespace());
                    last = start + 1 + tag.len();
                }
                words.extend(t[last..].split_whitespace());
            }
        }
        words.join(" ")
    }

    /// Replace the text, keeping the tags (appended after the new text).
    pub fn set_description(&mut self, desc: &str) {
        let tags = self.tags();
        self.rewrite_text(desc.trim(), &tags);
    }

    pub fn add_tag(&mut self, tag: &str) {
        let tag = clean_tag(tag);
        if tag.is_empty() || self.tags().iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            return;
        }
        let first_field = self
            .body
            .iter()
            .position(|s| matches!(s, Seg::Field { .. }))
            .unwrap_or(self.body.len());
        // the last text segment before the fields holds the description
        match first_field
            .checked_sub(1)
            .and_then(|i| match &mut self.body[i] {
                Seg::Text(t) => Some(t),
                _ => None,
            }) {
            Some(t) => {
                // the tag goes after the words, before any trailing whitespace
                let keep = t.len() - t.trim_end().len();
                let tail = t.split_off(t.len() - keep);
                t.push_str(" #");
                t.push_str(&tag);
                t.push_str(&tail);
            }
            None => self.body.insert(first_field, Seg::Text(format!(" #{tag}"))),
        }
    }

    /// Remove a tag wherever it is in the text. True if it was there.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        let tag = tag.trim_start_matches('#');
        let mut found = false;
        for s in &mut self.body {
            if let Seg::Text(t) = s {
                while let Some((start, _)) = tag_spans(t)
                    .into_iter()
                    .find(|(_, x)| x.eq_ignore_ascii_case(tag))
                {
                    found = true;
                    let end = start + 1 + tag.len();
                    // take one neighbouring space with it so no double gap is left
                    let (a, b) = if start > 0 && t[..start].ends_with(' ') {
                        (start - 1, end)
                    } else if t[end..].starts_with(' ') {
                        (start, end + 1)
                    } else {
                        (start, end)
                    };
                    t.replace_range(a..b, "");
                }
            }
        }
        if found {
            self.body
                .retain(|s| !matches!(s, Seg::Text(t) if t.is_empty()));
        }
        found
    }

    fn rewrite_text(&mut self, desc: &str, tags: &[String]) {
        self.body.retain(|s| matches!(s, Seg::Field { .. }));
        let mut text = String::from(" ");
        text.push_str(desc);
        for t in tags {
            if !text.ends_with(' ') {
                text.push(' ');
            }
            text.push('#');
            text.push_str(t);
        }
        self.body.insert(0, Seg::Text(text));
    }
}

/// A tag as written: no `#`, no spaces; anything else that isn't a tag character dropped.
pub fn clean_tag(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('#')
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| is_tag_char(*c))
        .collect()
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// `(byte offset of '#', tag)` for each tag: `#` at the start or after whitespace, then tag
/// characters with at least one that is not a digit (`#123` is an issue number, not a tag).
pub fn tag_spans(t: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut prev_ws = true;
    let mut iter = t.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if c == '#' && prev_ws {
            let start = i + 1;
            let mut end = start;
            while let Some(&(j, d)) = iter.peek() {
                if is_tag_char(d) {
                    end = j + d.len_utf8();
                    iter.next();
                } else {
                    break;
                }
            }
            let tag = t[start..end].trim_end_matches('/');
            if !tag.is_empty() && !tag.chars().all(|c| c.is_ascii_digit()) {
                out.push((i, tag));
            }
            prev_ws = false;
            continue;
        }
        prev_ws = c.is_whitespace();
    }
    out
}
