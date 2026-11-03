//! Home: the same cards all day across every vault, reordered by time of day. Cards about
//! one vault (projects, findings) carry its id and name.

use crate::days;
use crate::types::{note_id, Card, CardAction, Home, Need, Phase};
use crate::vault::checkboxes::{self, Checkbox};
use crate::vault::registry::Snapshot;
use crate::vault::{layout, Note, Vault};
use crate::{inbox, notes};

fn order(p: Phase) -> &'static [&'static str] {
    match p {
        Phase::Dawn => &[
            "focus", "needs", "project", "inbox", "findings", "tomorrow", "done",
        ],
        Phase::Day => &[
            "focus", "project", "needs", "inbox", "findings", "done", "tomorrow",
        ],
        Phase::Dusk => &[
            "needs", "done", "inbox", "project", "focus", "findings", "tomorrow",
        ],
        Phase::Night => &[
            "done", "tomorrow", "needs", "project", "inbox", "findings", "focus",
        ],
    }
}

fn greeting(p: Phase) -> &'static str {
    match p {
        Phase::Dawn => "Early start",
        Phase::Day => "Today",
        Phase::Dusk => "Winding down",
        Phase::Night => "The day so far",
    }
}

/// A stable hue per key, so a project keeps its colour across visits.
pub fn hue(key: &str) -> u32 {
    key.chars().fold(0u32, |h, c| {
        (h.wrapping_mul(31).wrapping_add(c as u32)) % 360
    })
}

/// High priority first, then the nearest due date.
pub fn by_urgency(a: &Checkbox, b: &Checkbox) -> std::cmp::Ordering {
    let rank = |c: &Checkbox| match c.priority {
        Some("H") => 0,
        Some("M") => 1,
        Some("L") => 2,
        _ => 3,
    };
    rank(a).cmp(&rank(b)).then_with(|| {
        a.due
            .as_deref()
            .unwrap_or("9999")
            .cmp(b.due.as_deref().unwrap_or("9999"))
    })
}

#[derive(Debug, Clone)]
pub struct Project {
    pub vault: Vault,
    pub rel: String,
    pub stem: String,
    pub title: String,
    pub status: String,
    pub progress: Option<u32>,
    pub updated: String,
    pub summary: String,
    pub open_tasks: Vec<Checkbox>,
}

fn progress(body: &str) -> Option<u32> {
    let i = body.find("**Progress:**")?;
    let rest = body[i + "**Progress:**".len()..].trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let after = &rest[digits.len()..];
    after
        .starts_with('%')
        .then(|| digits.parse().ok())
        .flatten()
}

/// Wiki links shown as their alias or target, so a summary reads as text.
fn plain_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("]]") {
            Some(j) => {
                let inner = &after[..j];
                let shown = inner
                    .split_once('|')
                    .map(|(_, a)| a)
                    .unwrap_or_else(|| inner.split('#').next().unwrap_or(inner));
                out.push_str(shown);
                rest = &after[j + 2..];
            }
            None => {
                out.push_str("[[");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max - 3).collect();
    let cut = cut.rfind(' ').map(|i| &cut[..i]).unwrap_or(&cut);
    format!("{cut}…")
}

fn project(vault: &Vault, n: &Note, open: &[Checkbox]) -> Project {
    let secs = days::sections(&n.body);
    let desc = secs
        .iter()
        .find(|(h, _)| h == "Description")
        .map(|(_, l)| l.join(" "))
        .unwrap_or_default();
    let summary = plain_links(&desc.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut open_tasks: Vec<Checkbox> = open.iter().filter(|t| t.rel == n.rel).cloned().collect();
    open_tasks.sort_by(by_urgency);
    Project {
        vault: vault.clone(),
        rel: n.rel.clone(),
        stem: n.stem.clone(),
        title: n.title().to_string(),
        status: n
            .props
            .get("status")
            .cloned()
            .unwrap_or_else(|| "active".into()),
        progress: progress(&n.body),
        updated: n.props.get("updated").cloned().unwrap_or_default(),
        summary: shorten(&summary, 220),
        open_tasks,
    }
}

/// Every project note (`01-Projects/`) of one vault with its open tasks.
pub fn projects(snap: &Snapshot) -> Vec<Project> {
    let open: Vec<Checkbox> = checkboxes::all(&snap.scan.notes)
        .into_iter()
        .filter(Checkbox::is_open)
        .collect();
    snap.scan
        .notes
        .iter()
        .filter(|n| layout::is_in(&n.rel, layout::PROJECTS) && n.stem != "AGENTS")
        .map(|n| project(&snap.vault, n, &open))
        .collect()
}

/// `05-References/*-findings.md` with the date of their latest `### YYYY-MM-DD` run.
pub fn findings(snap: &Snapshot) -> Vec<(String, String, Option<String>)> {
    snap.scan
        .notes
        .iter()
        .filter(|n| layout::is_in(&n.rel, layout::REFERENCES) && n.stem.ends_with("-findings"))
        .map(|n| {
            let latest = n
                .body
                .lines()
                .filter_map(|l| l.strip_prefix("### "))
                .rfind(|l| l.get(..10).and_then(days::parse_date).is_some())
                .map(str::to_string);
            (n.rel.clone(), n.title().to_string(), latest)
        })
        .collect()
}

fn action(label: &str, view: &str, arg: Option<String>) -> Option<CardAction> {
    Some(CardAction {
        label: label.into(),
        view: view.into(),
        arg,
    })
}

fn card(id: &str, kind: &str, kicker: String, title: String, lines: Vec<String>, hue: u32) -> Card {
    Card {
        id: id.into(),
        kind: kind.into(),
        kicker,
        title,
        lines,
        progress: None,
        hue,
        vault: None,
        vault_name: None,
        action: None,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Home for `date` (`today` when it is today) at `phase`. `needs` is the current queue.
pub fn home(snaps: &[Snapshot], date: &str, today: &str, phase: Phase, needs: &[Need]) -> Home {
    let is_today = date == today;
    let many = snaps.len() > 1;
    let mut open: Vec<(&Vault, Checkbox)> = snaps
        .iter()
        .flat_map(|s| {
            checkboxes::all(&s.scan.notes)
                .into_iter()
                .filter(Checkbox::is_open)
                .map(move |c| (&s.vault, c))
        })
        .collect();
    open.sort_by(|a, b| by_urgency(&a.1, &b.1));
    let tag = |v: &Vault, text: &str| {
        if many {
            format!("{text} · {}", v.name)
        } else {
            text.to_string()
        }
    };
    let mut cards: Vec<Card> = Vec::new();

    let soon = days::add_days(date, 2);
    let focus: Vec<String> = open
        .iter()
        .filter(|(_, t)| {
            t.due.as_deref().is_some_and(|d| d <= soon.as_str()) || t.priority == Some("H")
        })
        .take(5)
        .map(|(v, t)| tag(v, &t.text))
        .collect();
    let mut c = card(
        "focus",
        "focus",
        plural(open.len(), "open task", "open tasks"),
        if focus.is_empty() {
            "Nothing pressing"
        } else {
            "Worth doing today"
        }
        .into(),
        if focus.is_empty() {
            vec!["No task is due soon or marked high priority.".into()]
        } else {
            focus
        },
        36,
    );
    c.action = action("Open tasks", "tasks", None);
    cards.push(c);

    let mut c = card(
        "needs",
        "needs",
        format!("{} waiting", needs.len()),
        needs
            .first()
            .map(|n| n.title.clone())
            .unwrap_or_else(|| "Nothing needs a decision".into()),
        needs
            .iter()
            .skip(1)
            .take(3)
            .map(|n| format!("{}: {}", n.label, n.title))
            .collect(),
        12,
    );
    if !needs.is_empty() {
        c.action = action("Decide", "needs", None);
    }
    cards.push(c);

    let box_items = inbox::items(snaps, date);
    let mut c = card(
        "inbox",
        "inbox",
        format!("{} in the inbox", box_items.len()),
        if box_items.is_empty() {
            "Inbox is clear"
        } else {
            "Captures to curate"
        }
        .into(),
        box_items
            .iter()
            .take(4)
            .map(|i| format!("{} ({}d)", i.title, i.age_days))
            .collect(),
        260,
    );
    c.action = action("Open inbox", "inbox", None);
    cards.push(c);

    let d = days::day(snaps, date);
    let mut lines: Vec<String> = d
        .completed
        .iter()
        .map(|t| format!("✓ {}", t.text))
        .collect();
    lines.extend(d.agent_log.iter().map(|a| {
        format!("{} {}", a.time.as_deref().unwrap_or(""), a.text)
            .trim()
            .to_string()
    }));
    lines.truncate(5);
    let title = match (d.completed.is_empty(), is_today) {
        (false, true) => "Done today",
        (false, false) => "Done that day",
        (true, true) => "Today's trail",
        (true, false) => "That day's trail",
    };
    let mut c = card(
        "done",
        "done",
        format!(
            "{}, {} done",
            plural(d.agent_log.len(), "agent action", "agent actions"),
            plural(d.completed.len(), "task", "tasks")
        ),
        title.into(),
        lines,
        200,
    );
    c.action = action("Open day", "day", Some(date.into()));
    cards.push(c);

    let tomorrow_date = days::add_days(date, 1);
    let planned = d
        .human
        .iter()
        .find(|s| s.heading == "Tomorrow")
        .map(|s| s.lines.clone())
        .unwrap_or_default();
    let due_tomorrow: Vec<String> = open
        .iter()
        .filter(|(_, t)| t.due.as_deref() == Some(tomorrow_date.as_str()))
        .map(|(v, t)| tag(v, &t.text))
        .collect();
    let title = planned.first().cloned().unwrap_or_else(|| {
        if due_tomorrow.is_empty() {
            "Nothing planned yet".into()
        } else {
            format!("{} due tomorrow", due_tomorrow.len())
        }
    });
    let mut lines: Vec<String> = planned.iter().skip(1).cloned().collect();
    lines.extend(due_tomorrow);
    lines.truncate(4);
    let mut c = card("tomorrow", "tomorrow", "Tomorrow".into(), title, lines, 230);
    c.action = action("Open tomorrow", "day", Some(tomorrow_date));
    cards.push(c);

    let mut active: Vec<Project> = snaps
        .iter()
        .flat_map(projects)
        .filter(|p| p.status == "active")
        .collect();
    active.sort_by(|a, b| {
        b.updated
            .cmp(&a.updated)
            .then(b.open_tasks.len().cmp(&a.open_tasks.len()))
    });
    let mut suggestions: Vec<String> = active
        .iter()
        .take(3)
        .map(|p| format!("What is left on {}?", p.title))
        .collect();
    if !d.agent_log.is_empty() {
        suggestions.push(if is_today {
            "What did the agents do today?".into()
        } else {
            format!("What did the agents do on {date}?")
        });
    }
    if let Some(n) = needs.first() {
        let short: String = n.title.chars().take(70).collect();
        suggestions.push(format!("Why is \"{}\" waiting on me?", short.trim()));
    }
    for p in active.into_iter().take(6) {
        let id = note_id(&p.vault.id, &p.rel);
        let mut c = card(
            &format!("project-{id}"),
            "project",
            format!(
                "{} open, updated {}",
                p.open_tasks.len(),
                if p.updated.is_empty() {
                    "never"
                } else {
                    &p.updated
                }
            ),
            p.title.clone(),
            if p.open_tasks.is_empty() {
                vec![p.summary.clone()]
            } else {
                p.open_tasks
                    .iter()
                    .take(3)
                    .map(|t| t.text.clone())
                    .collect()
            },
            hue(&p.stem),
        );
        c.progress = p.progress;
        c.vault = Some(p.vault.id.clone());
        c.vault_name = Some(p.vault.name.clone());
        c.action = action("Open note", "note", Some(id));
        cards.push(c);
    }

    for s in snaps {
        for (rel, title, latest) in findings(s) {
            let id = note_id(&s.vault.id, &rel);
            let mut c = card(
                &format!("findings-{id}"),
                "findings",
                "Findings".into(),
                title,
                vec![latest
                    .map(|l| format!("Last run: {l}"))
                    .unwrap_or_else(|| "No runs logged yet.".into())],
                hue(&rel),
            );
            c.vault = Some(s.vault.id.clone());
            c.vault_name = Some(s.vault.name.clone());
            c.action = action("Open note", "note", Some(id));
            cards.push(c);
        }
    }

    let ord = order(phase);
    let pos = |k: &str| ord.iter().position(|x| *x == k).unwrap_or(ord.len());
    cards.sort_by_key(|c| pos(&c.kind));
    Home {
        date: date.into(),
        phase,
        greeting: if is_today {
            greeting(phase).into()
        } else {
            date.into()
        },
        cards,
        suggestions,
    }
}

/// Plain text of a project for evidence: its summary, else an excerpt.
pub fn project_excerpt(p: &Project, note: Option<&Note>) -> String {
    if !p.summary.is_empty() {
        return p.summary.clone();
    }
    note.map(|n| notes::excerpt(&n.body, 400))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn snap(dir: &std::path::Path, files: &[(&str, &str)]) -> Snapshot {
        for (rel, text) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let vault = Vault::open(dir).unwrap();
        Snapshot {
            scan: Arc::new(crate::vault::scan(&vault)),
            vault,
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(progress("**Status:** active · **Progress:** 60%"), Some(60));
        assert_eq!(progress("**Progress:** soon"), None);
        assert_eq!(
            plain_links("See [[zone-map|the map]] and [[a#b]]."),
            "See the map and a."
        );
        assert_eq!(hue("garden"), hue("garden"));
        assert!(hue("garden") < 360);
        assert!(shorten(&"word ".repeat(80), 50).ends_with('…'));
    }

    #[test]
    fn cards_across_vaults() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let date = "2026-11-02";
        let sa = snap(
            a.path(),
            &[(
                "01-Projects/garden.md",
                "---\ntitle: Garden\ntype: project\nstatus: active\nupdated: 2026-11-01\n---\n**Progress:** 60%\n## Description\nDrip line per [[zone-map]].\n## Tasks\n- [ ] Lay line ⏫ 📅 2026-11-02\n",
            )],
        );
        let sb = snap(
            b.path(),
            &[
                (
                    "01-Projects/site.md",
                    "---\ntitle: Site\nstatus: paused\n---\n",
                ),
                (
                    "05-References/build-findings.md",
                    "### 2026-10-30 run\nok\n",
                ),
                ("00-Inbox/2026-11-01-capture.md", "x"),
            ],
        );
        let h = home(&[sa, sb], date, date, Phase::Day, &[]);
        assert_eq!(h.greeting, "Today");
        assert_eq!(h.suggestions, vec!["What is left on Garden?"]);
        let kinds: Vec<_> = h.cards.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["focus", "project", "needs", "inbox", "findings", "done", "tomorrow"]
        );
        let focus = &h.cards[0];
        assert!(
            focus.lines[0].starts_with("Lay line · "),
            "tagged with the vault: {:?}",
            focus.lines
        );
        let p = &h.cards[1];
        assert_eq!(p.title, "Garden");
        assert_eq!(p.progress, Some(60));
        assert!(p.vault_name.is_some());
        assert_eq!(h.cards[3].kicker, "1 in the inbox");
        assert_eq!(h.cards[4].lines, vec!["Last run: 2026-10-30 run"]);
        let night = home(&[], "2026-11-01", date, Phase::Night, &[]);
        assert_eq!(night.greeting, "2026-11-01");
        assert_eq!(night.cards[0].kind, "done");
    }
}
