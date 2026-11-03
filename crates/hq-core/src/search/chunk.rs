//! Notes cut into heading-sized chunks (SPEC 6): the unit that is ranked, embedded and
//! quoted to the model. A chunk carries the note's dates so a search can be scoped by day.

use crate::locks::sha256;
use crate::types::HitKind;
use crate::vault::Note;

/// Longest chunk text, in characters. Long sections are split at line breaks below it.
pub const MAX_CHARS: usize = 1200;
/// Sections shorter than this say too little to be worth ranking on their own.
const MIN_CHARS: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub vault: String,
    pub rel: String,
    pub stem: String,
    pub title: String,
    /// `Title` or `Title › Section`.
    pub heading: String,
    pub text: String,
    /// SHA-256 of what is embedded (heading + text): the vector cache key with rel and model.
    pub hash: String,
    /// Every `YYYY-MM-DD` the note carries: created, updated, added, its file name.
    pub dates: Vec<String>,
    pub kind: HitKind,
    pub tokens: Vec<String>,
    /// The note's frontmatter `tags` and `type`, for filing tasks and suggesting tags.
    pub tags: Vec<String>,
    pub note_type: Option<String>,
}

impl Chunk {
    /// What the embedding model reads.
    pub fn embed_text(&self) -> String {
        format!("{}\n{}", self.heading, self.text)
    }

    pub fn newest_date(&self) -> Option<&str> {
        self.dates.iter().map(String::as_str).max()
    }
}

/// Notes that are about the vault itself, not knowledge in it.
pub fn indexed(rel: &str) -> bool {
    !(rel == "AGENTS.md" || rel.ends_with("/AGENTS.md") || rel.starts_with('_'))
}

pub fn kind_of(note: &Note) -> HitKind {
    if note.rel.starts_with("10-Bookmarks/")
        || note.props.get("type").is_some_and(|t| t == "bookmark")
    {
        HitKind::Bookmark
    } else {
        HitKind::Note
    }
}

/// The first `YYYY-MM-DD` inside `s`, if any.
pub fn find_date(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(9)).find_map(|i| {
        let w = &b[i..i + 10];
        let shape = w.iter().enumerate().all(|(j, c)| match j {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
        (shape && valid_md(&w[5..7], &w[8..10])).then(|| String::from_utf8_lossy(w).into_owned())
    })
}

fn valid_md(m: &[u8], d: &[u8]) -> bool {
    let n = |x: &[u8]| (x[0] - b'0') as u32 * 10 + (x[1] - b'0') as u32;
    (1..=12).contains(&n(m)) && (1..=31).contains(&n(d))
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "i", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "this", "to", "was", "were", "what", "when",
    "where", "which", "who", "why", "with", "how", "do", "does", "did", "my", "me", "we", "our",
    "you", "your", "not", "no", "can", "will", "about", "into", "than", "then", "there", "their",
    "them", "they",
];

/// Lowercase words (letters, digits, inner dots and dashes), stop words dropped.
pub fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .map(|t| t.trim_matches(|c| c == '.' || c == '-'))
        .filter(|t| t.chars().count() > 1 && !STOP.contains(t))
        .map(String::from)
        .collect()
}

/// Cut a note into chunks. Headings `#`..`###` start a section (not inside code fences);
/// deeper headings stay with their parent so sections keep their context.
pub fn chunk_note(vault: &str, note: &Note) -> Vec<Chunk> {
    let title = note.title().to_string();
    let mut dates: Vec<String> = ["created", "updated", "added"]
        .iter()
        .filter_map(|k| note.props.get(*k).and_then(|v| find_date(v)))
        .chain(find_date(&note.stem))
        .collect();
    dates.sort();
    dates.dedup();
    let kind = kind_of(note);

    let mut out = Vec::new();
    let mut heading = title.clone();
    let mut buf: Vec<&str> = Vec::new();
    let mut fenced = false;
    for line in note.body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced {
            if let Some(h) = heading_text(line) {
                flush(&mut out, &buf, &heading, &title, &dates, kind, vault, note);
                buf.clear();
                heading = if h == title {
                    title.clone()
                } else {
                    format!("{title} › {h}")
                };
                continue;
            }
        }
        buf.push(line);
    }
    flush(&mut out, &buf, &heading, &title, &dates, kind, vault, note);
    // a bookmark with no body text is still findable by its title and url
    if out.is_empty() && kind == HitKind::Bookmark {
        let text = note.props.get("url").cloned().unwrap_or_default();
        push(&mut out, text, &title, &title, &dates, kind, vault, note);
    }
    out
}

fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?.trim();
    (!rest.is_empty()).then_some(rest)
}

#[allow(clippy::too_many_arguments)]
fn flush(
    out: &mut Vec<Chunk>,
    lines: &[&str],
    heading: &str,
    title: &str,
    dates: &[String],
    kind: HitKind,
    vault: &str,
    note: &Note,
) {
    let text = lines.join("\n");
    let text = text.trim();
    if text.chars().count() < MIN_CHARS {
        return;
    }
    for part in split(text) {
        push(out, part, heading, title, dates, kind, vault, note);
    }
}

#[allow(clippy::too_many_arguments)]
fn push(
    out: &mut Vec<Chunk>,
    text: String,
    heading: &str,
    title: &str,
    dates: &[String],
    kind: HitKind,
    vault: &str,
    note: &Note,
) {
    let tokens = tokenize(&format!("{title} {heading} {text}"));
    let hash = sha256(&format!("{heading}\n{text}"));
    out.push(Chunk {
        vault: vault.to_string(),
        rel: note.rel.clone(),
        stem: note.stem.clone(),
        title: title.to_string(),
        heading: heading.to_string(),
        text,
        hash,
        dates: dates.to_vec(),
        kind,
        tokens,
        tags: note
            .props
            .get("tags")
            .map(|t| crate::frontmatter::list(t))
            .unwrap_or_default(),
        note_type: note.props.get("type").cloned(),
    });
}

/// Split at line breaks into parts of at most `MAX_CHARS`; a single longer line is cut
/// at a char boundary.
fn split(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let mut line = line;
        while line.chars().count() > MAX_CHARS {
            if !cur.trim().is_empty() {
                parts.push(std::mem::take(&mut cur).trim().to_string());
            }
            cur.clear();
            let cut = line
                .char_indices()
                .nth(MAX_CHARS)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            parts.push(line[..cut].to_string());
            line = &line[cut..];
        }
        if cur.chars().count() + line.chars().count() + 1 > MAX_CHARS && !cur.trim().is_empty() {
            parts.push(std::mem::take(&mut cur).trim().to_string());
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    pub(crate) fn note(rel: &str, props: &[(&str, &str)], body: &str) -> Note {
        let stem = rel
            .rsplit('/')
            .next()
            .unwrap()
            .trim_end_matches(".md")
            .to_string();
        Note {
            rel: rel.into(),
            stem,
            props: props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            body: body.into(),
            hash: String::new(),
            mtime: 0,
        }
    }

    #[test]
    fn sections_by_heading() {
        let n = note(
            "01-Projects/garden.md",
            &[("title", "Garden"), ("updated", "2026-10-30")],
            "# Garden\nIntro text that is long enough to keep.\n## Irrigation\nLay the drip line along the fence.\n#### Detail\nstays with irrigation, fine.\n```\n# not a heading inside a fence\n```\n## Tiny\nshort\n",
        );
        let c = chunk_note("v1", &n);
        assert_eq!(c.len(), 2, "{c:#?}");
        assert_eq!(c[0].heading, "Garden");
        assert_eq!(c[1].heading, "Garden › Irrigation");
        assert!(c[1].text.contains("#### Detail"));
        assert!(c[1].text.contains("# not a heading"));
        assert_eq!(c[1].dates, vec!["2026-10-30"]);
        assert_eq!(c[1].kind, HitKind::Note);
        assert!(c[1].tokens.contains(&"irrigation".to_string()));
    }

    #[test]
    fn long_sections_split_under_the_limit() {
        let para = "word ".repeat(50); // 250 chars per line
        let body = format!("## Long\n{}", [para.as_str(); 12].join("\n"));
        let c = chunk_note("v", &note("05-References/long.md", &[], &body));
        assert!(c.len() >= 3);
        assert!(c.iter().all(|x| x.text.chars().count() <= MAX_CHARS));
        let one_line = "é".repeat(3000);
        let c = chunk_note("v", &note("a.md", &[], &one_line));
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| x.text.chars().count() <= MAX_CHARS));
    }

    #[test]
    fn dates_from_props_and_file_name() {
        let n = note(
            "09-Planning/Daily/2026-10-28.md",
            &[("created", "2026-10-28"), ("updated", "2026-11-01")],
            "Worked on the garden fence all afternoon.",
        );
        let c = chunk_note("v", &n);
        assert_eq!(c[0].dates, vec!["2026-10-28", "2026-11-01"]);
        assert_eq!(c[0].newest_date(), Some("2026-11-01"));
        assert_eq!(
            find_date("x2026-13-01 2026-02-30y"),
            Some("2026-02-30".into())
        );
        assert_eq!(find_date("no date"), None);
    }

    #[test]
    fn bookmarks_and_hashes() {
        let b = note(
            "10-Bookmarks/chrome/rust-book.md",
            &[
                ("title", "The Rust Book"),
                ("type", "bookmark"),
                ("url", "https://doc.rust-lang.org/book/"),
            ],
            "\n## Notes\n",
        );
        let c = chunk_note("v", &b);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, HitKind::Bookmark);
        assert!(c[0].tokens.contains(&"rust".to_string()));
        let a = chunk_note("v", &note("a.md", &[], "same text in both of these notes"));
        let b = chunk_note("v", &note("a.md", &[], "same text in both of these notes!"));
        assert_ne!(a[0].hash, b[0].hash);
        assert!(!indexed("AGENTS.md") && !indexed("_tools/x.md") && indexed("01-Projects/a.md"));
    }

    #[test]
    fn tokens() {
        assert_eq!(
            tokenize("The **v2.0** release-notes, for [[Garden]]!"),
            vec!["v2.0", "release-notes", "garden"]
        );
    }
}
