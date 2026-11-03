//! A minimal reading of checkbox tasks (SPEC 4.1) for Home, Days and Needs.
//!
//! This is a seam, not the task engine: stream B's `tasks` module owns the full model,
//! urgency, recurrence and every task mutation. Until it lands, the core views read only
//! status, priority, due/done dates and the `🆔`, and Needs uses the three small edits at
//! the bottom. Swap callers over to `tasks` when it exists; nothing else depends on this.

use std::path::Path;

use crate::error::{Error, Result};
use crate::locks;
use crate::vault::{layout, Note};

#[derive(Debug, Clone, PartialEq)]
pub struct Checkbox {
    /// Vault-relative path of the note it is in.
    pub rel: String,
    /// The whole line as written, for matching a task that has no `🆔` yet.
    pub line: String,
    pub text: String,
    pub status: &'static str,
    pub priority: Option<&'static str>,
    pub due: Option<String>,
    pub done: Option<String>,
    pub id: Option<String>,
}

impl Checkbox {
    pub fn is_open(&self) -> bool {
        matches!(self.status, "pending" | "started" | "waiting")
    }
}

const DATE_FIELDS: &[&str] = &["📅", "⏳", "🛫", "✅", "❌", "➕"];
const PRIORITIES: &[(&str, &str)] = &[
    ("🔺", "H"),
    ("⏫", "H"),
    ("🔼", "M"),
    ("🔽", "L"),
    ("⏬", "L"),
];

/// Notes whose checkboxes count as tasks (SPEC 4.2): not the archive, not agent session
/// or compaction notes in the inbox.
pub fn in_scope(rel: &str) -> bool {
    if layout::is_in(rel, layout::ARCHIVE) || layout::is_in(rel, layout::TEMPLATES) {
        return false;
    }
    if layout::is_in(rel, layout::INBOX) {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        return !(name.contains("-session-") || name.contains("-compact-"));
    }
    true
}

/// Parse one line; None if it is not a checkbox task.
pub fn parse_line(line: &str, rel: &str) -> Option<Checkbox> {
    let t = line.trim_start();
    let rest = t.strip_prefix("- [").or_else(|| t.strip_prefix("* ["))?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    let rest = chars.as_str().strip_prefix("] ")?;
    let status = match mark {
        ' ' => "pending",
        '/' => "started",
        '>' => "waiting",
        'x' | 'X' => "done",
        '-' => "cancelled",
        _ => return None,
    };
    let mut c = Checkbox {
        rel: rel.to_string(),
        line: line.to_string(),
        text: String::new(),
        status,
        priority: None,
        due: None,
        done: None,
        id: None,
    };
    let mut words: Vec<&str> = Vec::new();
    let mut tokens = rest.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        if let Some((_, p)) = PRIORITIES.iter().find(|(e, _)| *e == tok) {
            c.priority = Some(p);
            continue;
        }
        if let Some(field) = DATE_FIELDS
            .iter()
            .chain(["🆔", "⛔"].iter())
            .find(|f| tok.starts_with(**f))
        {
            let attached = &tok[field.len()..];
            let value = if attached.is_empty() {
                tokens.next().unwrap_or("").to_string()
            } else {
                attached.to_string()
            };
            match *field {
                "📅" => c.due = Some(value),
                "✅" => c.done = Some(value),
                "🆔" => c.id = Some(value),
                _ => {}
            }
            continue;
        }
        if tok.starts_with('🔁') {
            // the rule runs until the next field emoji
            while let Some(next) = tokens.peek() {
                if DATE_FIELDS
                    .iter()
                    .chain(["🆔", "⛔"].iter())
                    .any(|f| next.starts_with(*f))
                    || PRIORITIES.iter().any(|(e, _)| e == next)
                {
                    break;
                }
                tokens.next();
            }
            continue;
        }
        words.push(tok);
    }
    c.text = words.join(" ");
    if c.text.is_empty() {
        return None;
    }
    Some(c)
}

/// Every checkbox in a note's body.
pub fn in_note(note: &Note) -> Vec<Checkbox> {
    note.body
        .lines()
        .filter_map(|l| parse_line(l, &note.rel))
        .collect()
}

/// Every checkbox in the in-scope notes of a scan.
pub fn all(notes: &[Note]) -> Vec<Checkbox> {
    notes
        .iter()
        .filter(|n| in_scope(&n.rel))
        .flat_map(in_note)
        .collect()
}

/// The three edits Needs makes on a task until stream B's mutations replace them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit<'a> {
    Done(&'a str),
    Cancel(&'a str),
    Due(&'a str),
}

/// Apply `edit` to the task matching `target` (by `🆔` when it has one, else by the exact
/// line), under the note's lock. A task that has gone or changed is NotFound.
pub fn edit(path: &Path, target: &Checkbox, edit: Edit<'_>, today: &str) -> Result<()> {
    locks::guarded_edit(path, None, |cur| {
        let mut found = false;
        let mut out = String::with_capacity(cur.len() + 32);
        for line in cur.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            let ending = &line[bare.len()..];
            let hit = !found
                && match (&target.id, parse_line(bare, &target.rel)) {
                    (Some(id), Some(c)) => c.id.as_deref() == Some(id.as_str()),
                    (None, Some(_)) => bare == target.line,
                    _ => false,
                };
            if hit {
                found = true;
                out.push_str(&apply(bare, edit));
                out.push_str(ending);
            } else {
                out.push_str(line);
            }
        }
        if !found {
            return Err(Error::NotFound(format!(
                "The task \"{}\" is no longer in {}; it may have been edited.",
                target.text, target.rel
            )));
        }
        Ok(crate::frontmatter::bump_updated(&out, today))
    })?;
    Ok(())
}

fn apply(line: &str, edit: Edit<'_>) -> String {
    let set_mark = |line: &str, mark: char| -> String {
        match line.find("- [") {
            Some(i) if line.len() > i + 4 => {
                format!("{}- [{mark}]{}", &line[..i], &line[i + 5..])
            }
            _ => line.to_string(),
        }
    };
    match edit {
        Edit::Done(date) => format!("{} ✅ {date}", set_mark(line, 'x').trim_end()),
        Edit::Cancel(date) => format!("{} ❌ {date}", set_mark(line, '-').trim_end()),
        Edit::Due(date) => {
            let tokens: Vec<&str> = line.split(' ').collect();
            let mut out: Vec<String> = Vec::with_capacity(tokens.len() + 2);
            let mut replaced = false;
            let mut skip_next = false;
            for tok in tokens {
                if skip_next {
                    skip_next = false;
                    out.push(date.to_string());
                    continue;
                }
                if tok == "📅" {
                    replaced = true;
                    skip_next = true;
                    out.push(tok.to_string());
                } else if tok.starts_with("📅") {
                    replaced = true;
                    out.push(format!("📅 {date}"));
                } else {
                    out.push(tok.to_string());
                }
            }
            let joined = out.join(" ");
            if replaced {
                joined
            } else {
                format!("{} 📅 {date}", joined.trim_end())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields() {
        let c = parse_line(
            "- [ ] Lay the main line #garden ⏫ ➕ 2026-10-20 📅 2026-11-01 🔁 every week 🆔 a1b2c3",
            "01-Projects/g.md",
        )
        .unwrap();
        assert_eq!(c.text, "Lay the main line #garden");
        assert_eq!(c.priority, Some("H"));
        assert_eq!(c.due.as_deref(), Some("2026-11-01"));
        assert_eq!(c.id.as_deref(), Some("a1b2c3"));
        assert!(c.is_open());
        let d = parse_line("  - [x] Done thing ✅ 2026-10-30", "a.md").unwrap();
        assert_eq!(d.status, "done");
        assert_eq!(d.done.as_deref(), Some("2026-10-30"));
        assert!(parse_line("- [ ] ", "a.md").is_none());
        assert!(parse_line("- plain bullet", "a.md").is_none());
        assert!(parse_line("- [?] odd", "a.md").is_none());
    }

    #[test]
    fn scope() {
        assert!(in_scope("01-Projects/a.md"));
        assert!(in_scope("00-Inbox/2026-11-01-capture.md"));
        assert!(!in_scope("00-Inbox/2026-11-01-session-claude.md"));
        assert!(!in_scope("08-Archive/old.md"));
    }

    #[test]
    fn edits_by_id_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(
            &p,
            "---\nupdated: 2026-01-01\n---\n- [ ] One 📅 2026-10-01 🆔 aaaaaa\n- [ ] Two\n",
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let tasks: Vec<_> = text.lines().filter_map(|l| parse_line(l, "t.md")).collect();
        edit(&p, &tasks[0], Edit::Due("2026-11-09"), "2026-11-02").unwrap();
        edit(&p, &tasks[1], Edit::Done("2026-11-02"), "2026-11-02").unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(
            after.contains("- [ ] One 📅 2026-11-09 🆔 aaaaaa\n"),
            "{after}"
        );
        assert!(after.contains("- [x] Two ✅ 2026-11-02\n"), "{after}");
        assert!(after.contains("updated: 2026-11-02"));
        // the line no longer matches: refused, nothing written
        assert!(matches!(
            edit(&p, &tasks[1], Edit::Cancel("2026-11-02"), "2026-11-02"),
            Err(Error::NotFound(_))
        ));
    }
}
