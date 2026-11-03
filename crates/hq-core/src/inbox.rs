//! The inbox (SPEC 3, `00-Inbox/`): captures, agent session and compaction notes,
//! proposals. Nothing is filed without a human look; archiving moves an item to
//! `08-Archive/inbox/`.

use crate::days;
use crate::error::{Error, Result};
use crate::notes;
use crate::types::{note_id, InboxItem, InboxKind, MoveResult};
use crate::vault::registry::Snapshot;
use crate::vault::{layout, Note};

pub fn kind_of(note: &Note) -> InboxKind {
    let t = note.props.get("type").map(String::as_str).unwrap_or("");
    let s = note.stem.as_str();
    if s.contains("-compact-") {
        InboxKind::Compact
    } else if s.contains("-session-") || t == "session" {
        InboxKind::Session
    } else if s.contains("-proposal-") || t == "proposal" {
        InboxKind::Proposal
    } else if s.ends_with("-capture") || t == "capture" {
        InboxKind::Capture
    } else {
        InboxKind::Dump
    }
}

/// `created:`, else the date a file name starts with, else `today`.
pub fn created(note: &Note, today: &str) -> String {
    if let Some(c) = note
        .props
        .get("created")
        .and_then(|c| days::parse_date(c).map(|_| c.clone()))
    {
        return c;
    }
    note.stem
        .get(..10)
        .filter(|d| days::parse_date(d).is_some())
        .map(str::to_string)
        .unwrap_or_else(|| today.to_string())
}

fn is_item(note: &Note) -> bool {
    layout::is_in(&note.rel, layout::INBOX) && !matches!(note.stem.as_str(), "AGENTS" | "README")
}

/// Every inbox note across the vaults, newest first.
pub fn items(snaps: &[Snapshot], today: &str) -> Vec<InboxItem> {
    let mut out: Vec<InboxItem> = snaps
        .iter()
        .flat_map(|s| {
            s.scan.notes.iter().filter(|n| is_item(n)).map(|n| {
                let created = created(n, today);
                InboxItem {
                    id: note_id(&s.vault.id, &n.rel),
                    vault_name: s.vault.name.clone(),
                    title: n.title().to_string(),
                    kind: kind_of(n),
                    age_days: days::days_between(&created, today).unwrap_or(0).max(0) as u32,
                    created,
                    author: n
                        .props
                        .get("author")
                        .cloned()
                        .unwrap_or_else(|| "unknown".into()),
                }
            })
        })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
    out
}

/// Move an inbox note to `08-Archive/inbox/`, updating links to it. Only inbox notes.
pub fn archive(snap: &Snapshot, rel: &str, today: &str) -> Result<MoveResult> {
    if !layout::is_in(rel, layout::INBOX) {
        return Err(Error::Invalid(format!(
            "Only notes in {}/ can be archived from the inbox.",
            layout::INBOX
        )));
    }
    notes::archive(snap, rel, today)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use std::sync::Arc;

    #[test]
    fn lists_and_archives() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("00-Inbox")).unwrap();
        std::fs::write(r.join("00-Inbox/2026-10-30-capture.md"), "x").unwrap();
        std::fs::write(
            r.join("00-Inbox/2026-11-01-session-claude.md"),
            "---\nauthor: claude\ncreated: 2026-11-01\n---\n",
        )
        .unwrap();
        std::fs::write(r.join("00-Inbox/2026-11-01-proposal-garden.md"), "x").unwrap();
        std::fs::write(r.join("00-Inbox/AGENTS.md"), "x").unwrap();
        std::fs::write(r.join("00-Inbox/loose idea.md"), "x").unwrap();
        let v = Vault::open(r).unwrap();
        let snap = Snapshot {
            scan: Arc::new(crate::vault::scan(&v)),
            vault: v,
        };
        let items = items(std::slice::from_ref(&snap), "2026-11-02");
        assert_eq!(items.len(), 4);
        let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&InboxKind::Session));
        assert!(kinds.contains(&InboxKind::Proposal));
        assert!(kinds.contains(&InboxKind::Dump));
        let cap = items.iter().find(|i| i.kind == InboxKind::Capture).unwrap();
        assert_eq!(cap.age_days, 3);
        assert_eq!(items.last().unwrap().created, "2026-10-30");
        archive(&snap, "00-Inbox/2026-10-30-capture.md", "2026-11-02").unwrap();
        assert!(r.join("08-Archive/inbox/2026-10-30-capture.md").exists());
        assert!(archive(&snap, "01-Projects/x.md", "d").is_err());
    }
}
