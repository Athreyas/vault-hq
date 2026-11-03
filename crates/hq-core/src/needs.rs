//! Needs you: things waiting on a human decision, from fixed rules (no model in the
//! ranking) so the queue is explainable and stable. Rules: overdue tasks, high-priority
//! tasks due within three days, proposals and week-old captures in the inbox, active
//! projects untouched for two weeks. Each decision is appended to
//! `09-Planning/decision-log.md` in the user's own words.

use std::collections::BTreeMap;

use crate::capture;
use crate::days;
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::home;
use crate::inbox;
use crate::locks;
use crate::notes;
use crate::types::{note_id, Evidence, InboxKind, Need, NeedKind, Outcome};
use crate::vault::checkboxes::{self, Checkbox};
use crate::vault::layout;
use crate::vault::registry::Snapshot;

/// Days an inbox item may sit before it asks for a look.
pub const CAPTURE_AGE: u32 = 2;
/// Days an active project may be quiet.
pub const STALE_DAYS: i64 = 14;

fn need_id(parts: &[&str]) -> String {
    locks::sha256(&parts.join("|"))[..12].to_string()
}

fn outcome(key: &str, label: &str, hint: &str) -> Outcome {
    Outcome {
        key: key.into(),
        label: label.into(),
        hint: hint.into(),
    }
}

fn task_outcomes() -> Vec<Outcome> {
    vec![
        outcome("done", "Done", "Tick it off in its note."),
        outcome(
            "redate",
            "Move to next week",
            "Set the due date seven days out.",
        ),
        outcome(
            "drop",
            "Drop it",
            "Cancel the task; the note keeps it struck through.",
        ),
        outcome(
            "snooze",
            "Snooze 3 days",
            "Hide it here until then; nothing else changes.",
        ),
    ]
}

/// A need and what it is about, so a decision can act on it.
struct Item {
    need: Need,
    snap: usize,
    rel: Option<String>,
    task: Option<Checkbox>,
}

fn plural_days(n: i64) -> String {
    format!("{n} day{}", if n == 1 { "" } else { "s" })
}

fn collect(snaps: &[Snapshot], today: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for (si, s) in snaps.iter().enumerate() {
        let vid = s.vault.id.as_str();
        let base = |kind, label: String, title: String, detail: String, rel: &str| Need {
            id: String::new(),
            kind,
            label,
            title,
            detail,
            vault: s.vault.id.clone(),
            vault_name: s.vault.name.clone(),
            evidence: Evidence {
                note: Some(note_id(vid, rel)),
                excerpt: String::new(),
                when: None,
            },
            outcomes: Vec::new(),
            rank: 0.0,
            task_id: None,
        };
        let body_of = |rel: &str| {
            s.scan
                .notes
                .iter()
                .find(|n| n.rel == rel)
                .map(|n| n.body.as_str())
                .unwrap_or("")
        };

        for t in checkboxes::all(&s.scan.notes)
            .into_iter()
            .filter(|t| t.status == "pending")
        {
            let Some(due) = t.due.clone() else { continue };
            let Some(late) = days::days_between(&due, today) else {
                continue;
            };
            let key = t.id.clone().unwrap_or_else(|| t.line.trim().to_string());
            let mut n = if late > 0 {
                let mut n = base(
                    NeedKind::Overdue,
                    "Overdue".into(),
                    t.text.clone(),
                    format!("Due {due}, {} ago.", plural_days(late)),
                    &t.rel,
                );
                n.id = need_id(&["overdue", vid, &t.rel, &key]);
                n.rank = 10.0 - late.min(9) as f64;
                n
            } else if t.priority == Some("H") && -late <= 3 {
                let soon = -late;
                let mut n = base(
                    NeedKind::DueSoon,
                    if soon == 0 {
                        "Due today".into()
                    } else {
                        format!("Due in {}", plural_days(soon))
                    },
                    t.text.clone(),
                    format!("High priority, due {due}."),
                    &t.rel,
                );
                n.id = need_id(&["due-soon", vid, &t.rel, &key]);
                n.rank = 20.0 + soon as f64;
                n
            } else {
                continue;
            };
            n.evidence.excerpt = notes::excerpt(body_of(&t.rel), 400);
            n.evidence.when = Some(due);
            n.outcomes = task_outcomes();
            n.task_id = t.id.clone();
            out.push(Item {
                need: n,
                snap: si,
                rel: Some(t.rel.clone()),
                task: Some(t),
            });
        }

        for n in s
            .scan
            .notes
            .iter()
            .filter(|n| layout::is_in(&n.rel, layout::INBOX))
        {
            if matches!(n.stem.as_str(), "AGENTS" | "README") {
                continue;
            }
            let created = inbox::created(n, today);
            let age = days::days_between(&created, today).unwrap_or(0).max(0);
            let kind = inbox::kind_of(n);
            let mut need = if kind == InboxKind::Proposal {
                let mut need = base(
                    NeedKind::Proposal,
                    "Proposal".into(),
                    n.title().to_string(),
                    "An agent proposes a change. Decide whether it should be applied.".into(),
                    &n.rel,
                );
                need.id = need_id(&["proposal", vid, &n.rel]);
                need.rank = 15.0;
                need.outcomes = vec![
                    outcome(
                        "accept",
                        "Accept",
                        "Record the go; ask an agent to apply it.",
                    ),
                    outcome(
                        "decline",
                        "Decline",
                        "Record the no and archive the proposal.",
                    ),
                    outcome("snooze", "Decide later", "Hide it for 3 days."),
                ];
                need.evidence.excerpt = notes::excerpt(&n.body, 900);
                need
            } else if age >= CAPTURE_AGE as i64 {
                let mut need = base(
                    NeedKind::Capture,
                    "Inbox".into(),
                    n.title().to_string(),
                    format!(
                        "Sitting in the inbox for {}. Curate what matters into a project or memory note, then archive it.",
                        plural_days(age)
                    ),
                    &n.rel,
                );
                need.id = need_id(&["capture", vid, &n.rel]);
                need.rank = 40.0 - age.min(30) as f64 * 0.1;
                need.outcomes = vec![
                    outcome("archive", "Done with it", "Move it to 08-Archive/inbox/."),
                    outcome("snooze", "Snooze 3 days", "Hide it here until then."),
                ];
                need.evidence.excerpt = notes::excerpt(&n.body, 700);
                need
            } else {
                continue;
            };
            need.evidence.when = Some(created);
            out.push(Item {
                need,
                snap: si,
                rel: Some(n.rel.clone()),
                task: None,
            });
        }

        for p in home::projects(s) {
            if p.status != "active" || p.updated.is_empty() {
                continue;
            }
            let Some(idle) = days::days_between(&p.updated, today) else {
                continue;
            };
            if idle < STALE_DAYS {
                continue;
            }
            let mut need = base(
                NeedKind::StaleProject,
                "Quiet project".into(),
                p.title.clone(),
                format!(
                    "Marked active, but untouched for {idle} days. Keep it, or pause it so it stops competing for attention."
                ),
                &p.rel,
            );
            // a new id every two quiet weeks, so "still active" comes back if nothing moves
            need.id = need_id(&["stale", vid, &p.rel, &(idle / STALE_DAYS).to_string()]);
            need.rank = 50.0;
            need.evidence.excerpt =
                home::project_excerpt(&p, s.scan.notes.iter().find(|n| n.rel == p.rel));
            need.evidence.when = Some(p.updated.clone());
            need.outcomes = vec![
                outcome(
                    "keep",
                    "Still active",
                    "Record it; it comes back in two weeks if still quiet.",
                ),
                outcome(
                    "pause",
                    "Pause it",
                    "Set status: paused in the project note.",
                ),
                outcome("snooze", "Snooze 3 days", "Hide it here until then."),
            ];
            out.push(Item {
                need,
                snap: si,
                rel: Some(p.rel.clone()),
                task: None,
            });
        }
    }
    out
}

fn visible(item: &Item, today: &str, snoozed: &BTreeMap<String, String>) -> bool {
    snoozed
        .get(&item.need.id)
        .is_none_or(|until| until.as_str() <= today)
}

/// The queue, most urgent first, without what is snoozed past `today`.
pub fn needs(snaps: &[Snapshot], today: &str, snoozed: &BTreeMap<String, String>) -> Vec<Need> {
    let mut items: Vec<Need> = collect(snaps, today)
        .into_iter()
        .filter(|i| visible(i, today, snoozed))
        .map(|i| i.need)
        .collect();
    items.sort_by(|a, b| a.rank.total_cmp(&b.rank).then(a.id.cmp(&b.id)));
    items
}

/// What a decision did, and the snooze the caller should store (need id, until date).
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub result: String,
    pub snooze: Option<(String, String)>,
}

fn decision_log_header(today: &str) -> String {
    format!(
        "{}# Decision log\n\nDecisions made in Vault HQ's Needs You queue, newest last. The reasoning is the user's own words.\n",
        frontmatter::render(&[
            ("title", "Decision log".into()),
            ("type", "index".into()),
            ("created", today.into()),
            ("updated", today.into()),
            ("author", "human".into()),
            ("tags", "[planning, decisions]".into()),
        ])
    )
}

/// Act on one need. `snoozed` filters the queue the same way the user saw it.
pub fn decide(
    snaps: &[Snapshot],
    id: &str,
    outcome_key: &str,
    why: &str,
    today: &str,
    hhmm: &str,
    snoozed: &BTreeMap<String, String>,
) -> Result<Decision> {
    let item = collect(snaps, today)
        .into_iter()
        .filter(|i| visible(i, today, snoozed))
        .find(|i| i.need.id == id)
        .ok_or_else(|| {
            Error::NotFound(
                "That item is no longer in the queue. Refresh to see the current list.".into(),
            )
        })?;
    let need = &item.need;
    let label = need
        .outcomes
        .iter()
        .find(|o| o.key == outcome_key)
        .map(|o| o.label.clone())
        .ok_or_else(|| Error::Invalid(format!("Unknown outcome for this item: {outcome_key}")))?;
    let snap = &snaps[item.snap];
    let vault = &snap.vault;
    let note_path = |rel: &str| notes::note_path(vault, rel);
    let snooze = |days: i64| Some((need.id.clone(), days::add_days(today, days)));

    let (result, snooze) = match outcome_key {
        "done" | "redate" | "drop" => {
            let task = item
                .task
                .as_ref()
                .ok_or_else(|| Error::Invalid("This item is not a task.".into()))?;
            let path = note_path(&task.rel)?;
            let next_week = days::add_days(today, 7);
            let (edit, result) = match outcome_key {
                "done" => (checkboxes::Edit::Done(today), "Marked done"),
                "redate" => (checkboxes::Edit::Due(&next_week), "Moved to next week"),
                _ => (checkboxes::Edit::Cancel(today), "Dropped"),
            };
            checkboxes::edit(&path, task, edit, today)?;
            (result.to_string(), None)
        }
        "archive" | "decline" => {
            let rel = item
                .rel
                .as_deref()
                .ok_or_else(|| Error::Invalid("Nothing to archive.".into()))?;
            inbox::archive(snap, rel, today)?;
            let r = if outcome_key == "archive" {
                "Archived"
            } else {
                "Declined"
            };
            (r.to_string(), None)
        }
        "pause" => {
            let rel = item
                .rel
                .as_deref()
                .ok_or_else(|| Error::Invalid("No project note to pause.".into()))?;
            locks::guarded_edit(&note_path(rel)?, None, |cur| {
                let t = frontmatter::set(cur, "status", "paused", true);
                Ok(frontmatter::bump_updated(&t, today))
            })?;
            ("Paused".to_string(), None)
        }
        "keep" => ("Kept active".to_string(), snooze(STALE_DAYS)),
        "snooze" => ("Snoozed for 3 days".to_string(), snooze(3)),
        _ => (label.clone(), snooze(1)),
    };

    if outcome_key != "snooze" {
        let evidence = item
            .rel
            .as_deref()
            .map(|r| {
                let stem = r.rsplit('/').next().unwrap_or(r).trim_end_matches(".md");
                format!("[[{stem}]]")
            })
            .unwrap_or_else(|| "none".into());
        let why = why.split_whitespace().collect::<Vec<_>>().join(" ");
        let entry = format!(
            "\n## {today} {hhmm}: {}\n- Kind: {}\n- Outcome: {label}\n- Why: {}\n- Evidence: {evidence}",
            need.title,
            need.label,
            if why.is_empty() { "(not given)" } else { &why }
        );
        let path = vault.resolve_rel(layout::DECISION_LOG)?;
        capture::append_line(&path, || decision_log_header(today), &entry, today)?;
        // the daily trail is a convenience; the decision itself is already recorded
        let _ = days::log(
            vault,
            "hq",
            &format!("{result}: {} → [[decision-log]]", need.title),
            today,
            hhmm,
            today,
        );
    }
    Ok(Decision { result, snooze })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use std::sync::Arc;

    const TODAY: &str = "2026-11-02";

    fn snap(dir: &std::path::Path) -> Snapshot {
        let vault = Vault::open(dir).unwrap();
        Snapshot {
            scan: Arc::new(crate::vault::scan(&vault)),
            vault,
        }
    }

    fn fixture(dir: &std::path::Path) {
        let w = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w(
            "01-Projects/garden.md",
            "---\ntitle: Garden\nstatus: active\nupdated: 2026-11-01\n---\n- [ ] Late one 📅 2026-10-30 🆔 late01\n- [ ] Soon ⏫ 📅 2026-11-03\n- [ ] Later ⏫ 📅 2026-11-20\n",
        );
        w(
            "01-Projects/old.md",
            "---\ntitle: Old\nstatus: active\nupdated: 2026-10-01\n---\n## Description\nAn old idea.\n",
        );
        w(
            "00-Inbox/2026-10-25-capture.md",
            "---\ncreated: 2026-10-25\n---\n- thought\n",
        );
        w(
            "00-Inbox/2026-11-02-capture.md",
            "---\ncreated: 2026-11-02\n---\n- fresh\n",
        );
        w(
            "00-Inbox/2026-11-01-proposal-garden.md",
            "---\ntitle: Move beds\n---\nPlan\n",
        );
    }

    #[test]
    fn rules_and_ranking() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let s = snap(dir.path());
        let q = needs(std::slice::from_ref(&s), TODAY, &BTreeMap::new());
        let kinds: Vec<_> = q.iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            vec![
                NeedKind::Overdue,
                NeedKind::Proposal,
                NeedKind::DueSoon,
                NeedKind::Capture,
                NeedKind::StaleProject
            ]
        );
        assert_eq!(q[0].detail, "Due 2026-10-30, 3 days ago.");
        assert_eq!(q[0].task_id.as_deref(), Some("late01"));
        assert_eq!(q[2].label, "Due in 1 day");
        // a snooze hides it until the date passes
        let mut snoozed = BTreeMap::new();
        snoozed.insert(q[0].id.clone(), "2026-11-05".to_string());
        assert_eq!(needs(std::slice::from_ref(&s), TODAY, &snoozed).len(), 4);
        assert_eq!(
            needs(std::slice::from_ref(&s), "2026-11-05", &snoozed)[0].kind,
            NeedKind::Overdue
        );
    }

    #[test]
    fn decisions_act_and_log() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let none = BTreeMap::new();
        let s = snap(dir.path());
        let q = needs(std::slice::from_ref(&s), TODAY, &none);
        let snaps = std::slice::from_ref(&s);

        let d = decide(
            snaps,
            &q[0].id,
            "redate",
            "  busy \n week ",
            TODAY,
            "09:00",
            &none,
        )
        .unwrap();
        assert_eq!(d.result, "Moved to next week");
        let garden = std::fs::read_to_string(dir.path().join("01-Projects/garden.md")).unwrap();
        assert!(
            garden.contains("- [ ] Late one 📅 2026-11-09 🆔 late01"),
            "{garden}"
        );
        let log = std::fs::read_to_string(dir.path().join(layout::DECISION_LOG)).unwrap();
        assert!(log.contains("## 2026-11-02 09:00: Late one\n- Kind: Overdue\n- Outcome: Move to next week\n- Why: busy week\n- Evidence: [[garden]]"), "{log}");
        let daily = std::fs::read_to_string(dir.path().join(layout::daily_rel(TODAY))).unwrap();
        assert!(daily.contains("- 09:00 hq · Moved to next week: Late one → [[decision-log]]"));

        let snooze = decide(snaps, &q[1].id, "snooze", "", TODAY, "09:01", &none).unwrap();
        assert_eq!(
            snooze.snooze,
            Some((q[1].id.clone(), "2026-11-05".to_string()))
        );

        let s = snap(dir.path());
        let snaps = std::slice::from_ref(&s);
        let q = needs(snaps, TODAY, &none);
        let stale = q.iter().find(|n| n.kind == NeedKind::StaleProject).unwrap();
        decide(snaps, &stale.id, "pause", "", TODAY, "09:02", &none).unwrap();
        let old = std::fs::read_to_string(dir.path().join("01-Projects/old.md")).unwrap();
        assert!(old.contains("status: paused") && old.contains("updated: 2026-11-02"));
        let cap = q.iter().find(|n| n.kind == NeedKind::Capture).unwrap();
        decide(snaps, &cap.id, "archive", "", TODAY, "09:03", &none).unwrap();
        assert!(dir
            .path()
            .join("08-Archive/inbox/2026-10-25-capture.md")
            .exists());

        assert!(matches!(
            decide(snaps, "nope", "done", "", TODAY, "09:04", &none),
            Err(Error::NotFound(_))
        ));
        let due = q.iter().find(|n| n.kind == NeedKind::DueSoon).unwrap();
        assert!(matches!(
            decide(snaps, &due.id, "archive", "", TODAY, "09:04", &none),
            Err(Error::Invalid(_))
        ));
        decide(snaps, &due.id, "done", "", TODAY, "09:05", &none).unwrap();
        let garden = std::fs::read_to_string(dir.path().join("01-Projects/garden.md")).unwrap();
        assert!(
            garden.contains("- [x] Soon ⏫ 📅 2026-11-03 ✅ 2026-11-02"),
            "{garden}"
        );
    }
}
