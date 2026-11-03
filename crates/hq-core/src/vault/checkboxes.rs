//! The task view Home, Days and Needs read, over stream B's engine (`crate::tasks`).
//!
//! B owns the model: parsing, scope, keys and every mutation. This module only flattens
//! a task into the few fields the core views show, and runs the three edits Needs makes
//! (done, redate, drop) through `tasks::mutate`, so they get B's guarantees (found by
//! `🆔` or content key under the lock, `🆔`/`➕` added, recurrence handled).

use crate::error::Result;
use crate::tasks::{self, mutate, parse, NoteMeta, TaskIndex};
use crate::types::{Priority, TaskPatch, TaskStatus};
use crate::vault::{Note, Vault};

#[derive(Debug, Clone, PartialEq)]
pub struct Checkbox {
    /// Vault-relative path of the note it is in.
    pub rel: String,
    /// B's task key: the `🆔`, or a content key for a line that has none yet.
    pub key: String,
    pub text: String,
    pub status: &'static str,
    /// `H`, `M` or `L`.
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

fn status(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::Started => "started",
        TaskStatus::Waiting => "waiting",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn priority(p: Priority) -> &'static str {
    match p {
        Priority::Highest | Priority::High => "H",
        Priority::Medium => "M",
        Priority::Low | Priority::Lowest => "L",
    }
}

/// Every task in one note (any scope).
pub fn in_note(vault_id: &str, note: &Note) -> Vec<Checkbox> {
    let found = parse::parse_lines(&note.body);
    tasks::to_tasks(&NoteMeta::of(vault_id, note), &found)
        .into_iter()
        .map(|t| Checkbox {
            rel: note.rel.clone(),
            key: t.key,
            text: t.description,
            status: status(t.status),
            priority: t.priority.map(priority),
            due: t.due,
            done: t.done,
            id: t.id,
        })
        .collect()
}

/// Every task in the in-scope notes of one vault's scan (SPEC 4.2 scope, B's rule).
pub fn all(vault_id: &str, notes: &[Note]) -> Vec<Checkbox> {
    notes
        .iter()
        .filter(|n| tasks::in_scope(&n.rel, &n.props))
        .flat_map(|n| in_note(vault_id, n))
        .collect()
}

/// The edits Needs makes on a task.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit<'a> {
    Done,
    Cancel,
    Due(&'a str),
}

/// Apply `edit` to `target` in `vault` through B's mutations. A task that has gone or
/// changed since it was read is refused (NotFound/Conflict from B).
pub fn edit(vault: &Vault, target: &Checkbox, edit: Edit<'_>, today: &str) -> Result<()> {
    let vaults = std::slice::from_ref(vault);
    let mut index = TaskIndex::build(vaults);
    let mut ctx = mutate::Ctx {
        vaults,
        index: &mut index,
        today: tasks::resolve_today(Some(today))?,
    };
    match edit {
        Edit::Done => {
            mutate::done(&mut ctx, &target.key)?;
        }
        Edit::Cancel => {
            mutate::cancel(&mut ctx, &target.key)?;
        }
        Edit::Due(date) => {
            let patch = TaskPatch {
                due: Some(Some(date.to_string())),
                ..Default::default()
            };
            mutate::update(&mut ctx, &target.key, &patch)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_edits_through_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("01-Projects")).unwrap();
        std::fs::create_dir_all(dir.path().join("08-Archive")).unwrap();
        std::fs::write(
            dir.path().join("01-Projects/p.md"),
            "---\ntype: project\n---\n- [ ] One #garden ⏫ 📅 2026-10-01 🆔 aaaaaa\n- [x] Two ✅ 2026-10-30\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("08-Archive/old.md"), "- [ ] archived\n").unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let s = crate::vault::scan(&v);
        let all = all(&v.id, &s.notes);
        assert_eq!(all.len(), 2, "archive is out of scope");
        assert_eq!(all[0].priority, Some("H"));
        assert_eq!(all[0].due.as_deref(), Some("2026-10-01"));
        assert_eq!(all[0].key, "aaaaaa");
        assert!(all[0].is_open() && !all[1].is_open());
        assert_eq!(all[1].done.as_deref(), Some("2026-10-30"));

        edit(&v, &all[0], Edit::Due("2026-11-09"), "2026-11-02").unwrap();
        let text = std::fs::read_to_string(dir.path().join("01-Projects/p.md")).unwrap();
        assert!(text.contains("📅 2026-11-09"), "{text}");
        // a key that no longer matches anything is refused, nothing written
        let gone = Checkbox {
            key: "zzzzzz".into(),
            ..all[0].clone()
        };
        assert!(edit(&v, &gone, Edit::Done, "2026-11-02").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("01-Projects/p.md")).unwrap(),
            text
        );
    }
}
