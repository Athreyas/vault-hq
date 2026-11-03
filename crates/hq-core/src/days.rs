//! Days (SPEC 3, daily notes): one view per date across every vault, and the agent log
//! line writer (`- HH:MM who · text` under `## Agent log`) that HQ and MCP's `log` share.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};

use crate::capture;
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::types::{note_id, AgentLogLine, Day, DaySection, NoteId, NoteRef, Phase, TaskLine};
use crate::vault::checkboxes::{self, Checkbox};
use crate::vault::registry::Snapshot;
use crate::vault::{layout, Vault};

pub const HUMAN_SECTIONS: &[&str] = &["Quick Wins", "Work", "Learning", "Tomorrow"];
pub const AGENT_LOG: &str = "Agent log";
/// The longest `days(from, span)` request.
pub const MAX_SPAN: u32 = 62;

// ---- dates ----------------------------------------------------------------------------------

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// A `YYYY-MM-DD` from the UI or an agent, or a clear Invalid.
pub fn valid_date(s: &str) -> Result<String> {
    parse_date(s)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .ok_or_else(|| Error::Invalid(format!("\"{s}\" is not a date (YYYY-MM-DD).")))
}

pub fn add_days(date: &str, n: i64) -> String {
    parse_date(date)
        .and_then(|d| d.checked_add_signed(chrono::Duration::days(n)))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| date.to_string())
}

/// Whole days from `a` to `b` (positive when `b` is later); None if either isn't a date.
pub fn days_between(a: &str, b: &str) -> Option<i64> {
    Some((parse_date(b)? - parse_date(a)?).num_days())
}

/// Morning leads with what to do, the day with work in motion, the evening with what
/// still needs you, the night with how the day went.
pub fn phase_at(hour: u32) -> Phase {
    match hour {
        5..=7 => Phase::Dawn,
        8..=16 => Phase::Day,
        17..=20 => Phase::Dusk,
        _ => Phase::Night,
    }
}

pub fn phase_now() -> Phase {
    use chrono::Timelike;
    phase_at(chrono::Local::now().hour())
}

// ---- daily notes ------------------------------------------------------------------------------

/// `## Heading` sections of a body, in order, with their lines.
pub fn sections(body: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            out.push((h.trim().to_string(), Vec::new()));
        } else if let Some((_, lines)) = out.last_mut() {
            lines.push(line.to_string());
        }
    }
    out
}

fn section<'s>(secs: &'s [(String, Vec<String>)], heading: &str) -> &'s [String] {
    secs.iter()
        .find(|(h, _)| h == heading)
        .map(|(_, l)| l.as_slice())
        .unwrap_or(&[])
}

fn agent_log(lines: &[String], vault_name: &str) -> Vec<AgentLogLine> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| {
            let text = l.trim();
            let time = text
                .split_whitespace()
                .next()
                .filter(|t| {
                    let (h, m) = t.split_once(':').unwrap_or(("", ""));
                    (1..=2).contains(&h.len())
                        && m.len() == 2
                        && h.chars().chain(m.chars()).all(|c| c.is_ascii_digit())
                })
                .map(str::to_string);
            let rest = match &time {
                Some(t) => text[t.len()..].trim(),
                None => text,
            };
            AgentLogLine {
                links: crate::notes::links(rest)
                    .into_iter()
                    .filter(|l| l.wiki)
                    .map(|l| l.target)
                    .collect(),
                time,
                text: rest.to_string(),
                vault_name: vault_name.to_string(),
            }
        })
        .collect()
}

pub fn task_line(vault: &Vault, c: &Checkbox) -> TaskLine {
    TaskLine {
        note: note_id(&vault.id, &c.rel),
        text: c.text.clone(),
        status: c.status.into(),
        priority: c.priority.map(str::to_string),
        due: c.due.clone(),
        done: c.done.clone(),
        task_id: c.id.clone(),
    }
}

fn metric(v: &str) -> serde_json::Value {
    v.parse::<i64>()
        .map(serde_json::Value::from)
        .or_else(|_| {
            v.parse::<f64>()
                .ok()
                .filter(|f| f.is_finite())
                .map(serde_json::Value::from)
                .ok_or(())
        })
        .unwrap_or_else(|_| serde_json::Value::from(v))
}

/// One date across every vault, primary first: its daily notes' sections, metrics and
/// agent log, the tasks done that day, the captures made that day.
pub fn day(snaps: &[Snapshot], date: &str) -> Day {
    let rel = layout::daily_rel(date);
    let mut d = Day {
        date: date.to_string(),
        exists: false,
        note: None,
        metrics: BTreeMap::new(),
        human: HUMAN_SECTIONS
            .iter()
            .map(|h| DaySection {
                heading: h.to_string(),
                lines: Vec::new(),
            })
            .collect(),
        agent_log: Vec::new(),
        completed: Vec::new(),
        captures: Vec::new(),
        due_tasks: 0,
    };
    for s in snaps {
        if let Some(n) = s.scan.notes.iter().find(|n| n.rel == rel) {
            d.exists = true;
            d.note.get_or_insert_with(|| note_id(&s.vault.id, &n.rel));
            for (k, v) in &n.props {
                // metrics are the only underscored properties a daily note has
                if k.contains('_') {
                    d.metrics.entry(k.clone()).or_insert_with(|| metric(v));
                }
            }
            let secs = sections(&n.body);
            for h in d.human.iter_mut() {
                h.lines.extend(
                    section(&secs, &h.heading)
                        .iter()
                        .map(|l| {
                            l.trim()
                                .strip_prefix('-')
                                .unwrap_or(l.trim())
                                .trim()
                                .to_string()
                        })
                        .filter(|l| !l.is_empty()),
                );
            }
            d.agent_log
                .extend(agent_log(section(&secs, AGENT_LOG), &s.vault.name));
        }
        for c in checkboxes::all(&s.scan.notes) {
            if c.done.as_deref() == Some(date) && c.status == "done" {
                d.completed.push(task_line(&s.vault, &c));
            } else if c.is_open() && c.due.as_deref() == Some(date) {
                d.due_tasks += 1;
            }
        }
        for n in &s.scan.notes {
            let name = n.rel.rsplit('/').next().unwrap_or(&n.rel);
            if layout::is_in(&n.rel, layout::INBOX) && name.starts_with(date) {
                d.captures.push(NoteRef {
                    id: note_id(&s.vault.id, &n.rel),
                    title: n.title().to_string(),
                });
            }
        }
    }
    d
}

/// `span` consecutive days starting at `from`.
pub fn days(snaps: &[Snapshot], from: &str, span: u32) -> Result<Vec<Day>> {
    let from = valid_date(from)?;
    if span == 0 || span > MAX_SPAN {
        return Err(Error::Invalid(format!(
            "Ask for 1 to {MAX_SPAN} days at a time."
        )));
    }
    Ok((0..span as i64)
        .map(|i| day(snaps, &add_days(&from, i)))
        .collect())
}

/// A new daily note in the SPEC 3 layout.
pub fn new_daily(date: &str) -> String {
    let title = parse_date(date)
        .map(|d| format!("{}, {} {}", d.format("%A"), d.format("%B"), d.day()))
        .unwrap_or_else(|| date.to_string());
    format!(
        "{}# {title}\n\n## Quick Wins\n-\n\n## Work\n-\n\n## Learning\n-\n\n## Tomorrow\n-\n\n## {AGENT_LOG}\n\n\
         _Machine zone: agents and scripts append one line per action. Do not edit above this heading._\n",
        frontmatter::render(&[
            ("title", format!("\"{title}\"")),
            ("type", "daily".into()),
            ("created", date.into()),
            ("updated", date.into()),
            ("author", "human".into()),
            ("tags", "[daily]".into()),
        ])
    )
}

/// Insert `line` at the end of the `## Agent log` section (adding the section if a note
/// has none), keeping everything else as it was.
fn insert_log_line(text: &str, line: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let heading = format!("## {AGENT_LOG}");
    let Some(start) = lines.iter().position(|l| l.trim_end() == heading.as_str()) else {
        let mut out = text.to_string();
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("\n{heading}\n\n{line}\n"));
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    // after the section's last non-blank line
    let mut at = end;
    while at > start + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    for l in &lines[..at] {
        out.push_str(l);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    for l in &lines[at..] {
        out.push_str(l);
    }
    out
}

/// Append `- HH:MM who · text` to the agent log of `date`'s daily note, creating the note
/// if it doesn't exist yet.
pub fn log(
    vault: &Vault,
    who: &str,
    text: &str,
    date: &str,
    hhmm: &str,
    today: &str,
) -> Result<NoteId> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(Error::Invalid("Nothing to log.".into()));
    }
    let who = who.split_whitespace().collect::<Vec<_>>().join("-");
    let who = if who.is_empty() { "hq".into() } else { who };
    let rel = layout::daily_rel(&valid_date(date)?);
    let path = vault.resolve_rel(&rel)?;
    let line = format!("- {hhmm} {who} · {text}");
    capture::edit_or_create(&path, |cur| {
        let base = if cur.is_empty() {
            new_daily(date)
        } else {
            cur.to_string()
        };
        Ok(frontmatter::bump_updated(
            &insert_log_line(&base, &line),
            today,
        ))
    })?;
    Ok(note_id(&vault.id, &rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn dates() {
        assert_eq!(add_days("2026-10-31", 2), "2026-11-02");
        assert_eq!(days_between("2026-10-30", "2026-11-02"), Some(3));
        assert_eq!(days_between("x", "2026-11-02"), None);
        assert!(valid_date("2026-13-01").is_err());
        assert_eq!(phase_at(6), Phase::Dawn);
        assert_eq!(phase_at(12), Phase::Day);
        assert_eq!(phase_at(19), Phase::Dusk);
        assert_eq!(phase_at(23), Phase::Night);
    }

    #[test]
    fn log_lines() {
        let t = new_daily("2026-11-02");
        assert!(t.contains("# Monday, November 2\n"));
        let once = insert_log_line(&t, "- 09:00 claude · one");
        let twice = insert_log_line(&once, "- 09:05 hq · two");
        assert!(
            twice.ends_with(
                "Do not edit above this heading._\n- 09:00 claude · one\n- 09:05 hq · two\n"
            ),
            "{twice}"
        );
        // a section in the middle: the line goes before the next heading
        let mid = insert_log_line("## Agent log\n- a\n\n## Later\nx\n", "- b");
        assert_eq!(mid, "## Agent log\n- a\n- b\n\n## Later\nx\n");
        let none = insert_log_line("# Day\n", "- c");
        assert_eq!(none, "# Day\n\n## Agent log\n\n- c\n");
    }

    #[test]
    fn day_across_vaults() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let date = "2026-11-02";
        let va = Vault::open(a.path()).unwrap();
        let vb = Vault::open(b.path()).unwrap();
        log(&va, "claude", "Filed [[garden]] notes", date, "09:10", date).unwrap();
        log(&vb, "codex", "Ran tests", date, "10:00", date).unwrap();
        let daily = a.path().join(layout::daily_rel(date));
        let text = std::fs::read_to_string(&daily)
            .unwrap()
            .replace("## Work\n-\n", "## Work\n- Wrote the plan\n")
            .replace(
                "author: human\n",
                "author: human\ntasks_pending: 12\nmood_note: fine\n",
            );
        std::fs::write(&daily, text).unwrap();
        std::fs::create_dir_all(a.path().join("01-Projects")).unwrap();
        std::fs::write(
            a.path().join("01-Projects/p.md"),
            format!("- [x] Shipped ✅ {date}\n- [ ] Due now 📅 {date}\n"),
        )
        .unwrap();
        std::fs::create_dir_all(b.path().join("00-Inbox")).unwrap();
        std::fs::write(b.path().join(format!("00-Inbox/{date}-capture.md")), "x").unwrap();
        let snaps: Vec<Snapshot> = [va, vb]
            .into_iter()
            .map(|v| Snapshot {
                scan: Arc::new(crate::vault::scan(&v)),
                vault: v,
            })
            .collect();
        let d = day(&snaps, date);
        assert!(d.exists);
        assert_eq!(d.metrics["tasks_pending"], serde_json::json!(12));
        assert_eq!(d.metrics["mood_note"], serde_json::json!("fine"));
        assert_eq!(d.human[1].lines, vec!["Wrote the plan"]);
        assert_eq!(d.agent_log.len(), 2);
        assert_eq!(d.agent_log[0].time.as_deref(), Some("09:10"));
        assert_eq!(d.agent_log[0].text, "claude · Filed [[garden]] notes");
        assert_eq!(d.agent_log[0].links, vec!["garden"]);
        assert_eq!(d.completed.len(), 1);
        assert_eq!(d.due_tasks, 1);
        assert_eq!(d.captures.len(), 1);
        assert_eq!(days(&snaps, date, 3).unwrap().len(), 3);
        assert!(days(&snaps, date, 0).is_err());
        assert!(!days(&snaps, "2026-11-03", 1).unwrap()[0].exists);
    }
}
