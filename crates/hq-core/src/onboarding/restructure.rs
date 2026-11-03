//! Applying the import plan: only ticked items, each guarded, all recorded in a journal
//! (`.vault-hq/import-<stamp>.json`) so `undo` can put things back.
//!
//! - Edits go through `locks::guarded_edit` with the hash the analysis read: a note changed
//!   since is skipped, never overwritten. Only the frontmatter block changes.
//! - Moves never replace anything: a file whose target exists is skipped; a folder moving
//!   onto an existing folder is merged file by file under the same rule.
//! - Nothing is deleted. Undo removes only files and empty folders this run created, and
//!   only while they are unchanged.
//! - Added `updated:` keeps the file's own date: an import that made every note look edited
//!   today would bury the notes that really were.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::scaffold::{scaffold, Options};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks::{self, sha256};
use crate::types::{ImportPlan, Moved, PlanAction, Prop, RestructureReport, SkippedItem};
use crate::vault::MARKER;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Entry {
    /// A file (with its hash) or a folder (no hash) this run created.
    Created {
        rel: String,
        hash: Option<String>,
    },
    /// The frontmatter prefix `before` became `after`; the note then hashed to `hash`.
    Edited {
        rel: String,
        before: String,
        after: String,
        hash: String,
    },
    Moved {
        from: String,
        to: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    format: u32,
    created: String,
    entries: Vec<Entry>,
}

/// A vault-relative path that stays inside the vault and out of hidden folders.
fn checked(root: &Path, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    let ok = !rel.is_empty()
        && p.components().all(|c| match c {
            Component::Normal(s) => !s.to_string_lossy().starts_with('.'),
            _ => false,
        });
    if !ok {
        return Err(Error::Denied(format!("{rel} is not a path HQ may change")));
    }
    Ok(root.join(p))
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

struct Run<'a> {
    root: &'a Path,
    entries: Vec<Entry>,
    report: RestructureReport,
    /// rel → (hash the analysis saw, hash after our own edit), so two ticked edits to one
    /// note (frontmatter, then tags) don't trip over each other.
    ours: HashMap<String, (String, String)>,
}

pub fn apply(plan: &ImportPlan) -> Result<RestructureReport> {
    let root = Path::new(&plan.root);
    if !root.is_dir() {
        return Err(Error::NotFound(format!("No folder at {}", root.display())));
    }
    let mut run = Run {
        root,
        entries: Vec::new(),
        report: RestructureReport {
            applied: vec![],
            skipped: vec![],
            moved: vec![],
            edited: vec![],
            created: vec![],
            journal: None,
        },
        ours: HashMap::new(),
    };
    let ticked: Vec<_> = plan.items.iter().filter(|i| i.ticked).collect();
    // folders first, then edits at the paths the analysis saw, then moves
    let rank = |a: &PlanAction| match a {
        PlanAction::CreateFolders { .. } => 0,
        PlanAction::AddFrontmatter { .. } => 1,
        PlanAction::AddTags { .. } => 2,
        PlanAction::Move { .. } => 3,
    };
    let mut ordered = ticked;
    ordered.sort_by_key(|i| rank(&i.action));

    for item in ordered {
        let r = match &item.action {
            PlanAction::CreateFolders { .. } => run.create_folders(),
            PlanAction::AddFrontmatter {
                rel,
                base_hash,
                add,
            } => run.edit(rel, base_hash, |text| add_props(text, add)),
            PlanAction::AddTags {
                rel,
                base_hash,
                tags,
            } => run.edit(rel, base_hash, |text| {
                Ok(frontmatter::set(
                    text,
                    "tags",
                    &frontmatter::format_list(tags),
                    true,
                ))
            }),
            PlanAction::Move { from, to } => run.move_item(&item.id, from, to),
        };
        match r {
            Ok(()) => run.report.applied.push(item.id.clone()),
            Err(e) => run.report.skipped.push(SkippedItem {
                id: item.id.clone(),
                reason: match e {
                    Error::Conflict { .. } => "it changed after the analysis; analyse again".into(),
                    other => other.to_string(),
                },
            }),
        }
    }

    if !run.entries.is_empty() {
        run.report.journal = Some(write_journal(root, &run.entries)?);
    }
    Ok(run.report)
}

fn add_props(text: &str, add: &[Prop]) -> Result<String> {
    if frontmatter::parse(text).len == 0 {
        let props: Vec<(&str, String)> = add
            .iter()
            .map(|p| (p.key.as_str(), p.value.clone()))
            .collect();
        return Ok(format!("{}{text}", frontmatter::render(&props)));
    }
    let mut out = text.to_string();
    for p in add {
        out = frontmatter::set(&out, &p.key, &p.value, false);
    }
    Ok(out)
}

impl Run<'_> {
    fn create_folders(&mut self) -> Result<()> {
        let name = self
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "vault".into());
        let r = scaffold(
            self.root,
            &Options {
                name: &name,
                welcome: false,
            },
        )?;
        for rel in r.created {
            // the marker stays: the vault is being added either way
            if rel == MARKER {
                continue;
            }
            let hash = match rel.strip_suffix('/') {
                Some(_) => None,
                None => Some(sha256(&std::fs::read_to_string(self.root.join(&rel))?)),
            };
            self.entries.push(Entry::Created {
                rel: rel.trim_end_matches('/').to_string(),
                hash,
            });
            self.report.created.push(rel);
        }
        Ok(())
    }

    fn edit(
        &mut self,
        rel: &str,
        base_hash: &str,
        change: impl FnOnce(&str) -> Result<String>,
    ) -> Result<()> {
        let path = checked(self.root, rel)?;
        let base = match self.ours.get(rel) {
            Some((orig, now)) if orig == base_hash => now.clone(),
            _ => base_hash.to_string(),
        };
        let mut prefixes: Option<(String, String)> = None;
        let hash = locks::guarded_edit(&path, Some(&base), |cur| {
            let next = change(cur)?;
            let body = frontmatter::parse(cur).body;
            if !next.ends_with(body) {
                return Err(Error::Internal(format!(
                    "the frontmatter edit of {rel} would change its body"
                )));
            }
            prefixes = Some((
                cur[..cur.len() - body.len()].to_string(),
                next[..next.len() - body.len()].to_string(),
            ));
            Ok(next)
        })?;
        if let Some((before, after)) = prefixes {
            if before != after {
                self.entries.push(Entry::Edited {
                    rel: rel.to_string(),
                    before,
                    after,
                    hash: hash.clone(),
                });
                if !self.report.edited.iter().any(|e| e == rel) {
                    self.report.edited.push(rel.to_string());
                }
            }
        }
        let orig = self
            .ours
            .get(rel)
            .map(|(o, _)| o.clone())
            .unwrap_or_else(|| base_hash.to_string());
        self.ours.insert(rel.to_string(), (orig, hash));
        Ok(())
    }

    fn move_item(&mut self, id: &str, from: &str, to: &str) -> Result<()> {
        let src = checked(self.root, from)?;
        let dst = checked(self.root, to)?;
        if Path::new(to).starts_with(from) {
            return Err(Error::Invalid(format!("{from} can't move inside itself")));
        }
        if !src.exists() {
            return Err(Error::NotFound(format!("{from} is no longer there")));
        }
        let before = self.report.moved.len();
        self.move_path(id, &src, &dst)?;
        if self.report.moved.len() == before {
            return Err(Error::Invalid(format!(
                "nothing in {from} could move: everything already exists at {to}"
            )));
        }
        // a merged folder left empty goes too; undo recreates it
        if src.is_dir() && std::fs::read_dir(&src)?.next().is_none() {
            let _ = std::fs::remove_dir(&src);
        }
        Ok(())
    }

    fn move_path(&mut self, id: &str, src: &Path, dst: &Path) -> Result<()> {
        if !dst.exists() {
            if let Some(parent) = dst.parent() {
                self.make_dirs(parent)?;
            }
            std::fs::rename(src, dst)?;
            let (from, to) = (rel_of(self.root, src), rel_of(self.root, dst));
            self.entries.push(Entry::Moved {
                from: from.clone(),
                to: to.clone(),
            });
            self.report.moved.push(Moved { from, to });
            return Ok(());
        }
        if src.is_dir() && dst.is_dir() {
            let mut children: Vec<_> = std::fs::read_dir(src)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            children.sort();
            for c in children {
                self.move_path(id, &src.join(&c), &dst.join(&c))?;
            }
            return Ok(());
        }
        self.report.skipped.push(SkippedItem {
            id: id.to_string(),
            reason: format!(
                "{} already exists, so {} stayed where it is",
                rel_of(self.root, dst),
                rel_of(self.root, src)
            ),
        });
        Ok(())
    }

    /// create_dir_all, journaling each folder it makes so undo can remove them again.
    fn make_dirs(&mut self, dir: &Path) -> Result<()> {
        let mut missing = Vec::new();
        let mut cur = dir;
        while !cur.exists() {
            missing.push(cur.to_path_buf());
            match cur.parent() {
                Some(p) => cur = p,
                None => break,
            }
        }
        for d in missing.into_iter().rev() {
            std::fs::create_dir(&d)?;
            self.entries.push(Entry::Created {
                rel: rel_of(self.root, &d),
                hash: None,
            });
        }
        Ok(())
    }
}

fn write_journal(root: &Path, entries: &[Entry]) -> Result<String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let journal = Journal {
        format: 1,
        created: crate::util::today(),
        entries: entries.to_vec(),
    };
    let text = serde_json::to_string_pretty(&journal)?;
    for n in 0..100 {
        let rel = if n == 0 {
            format!(".vault-hq/import-{stamp}.json")
        } else {
            format!(".vault-hq/import-{stamp}-{n}.json")
        };
        match locks::create_new(&root.join(&rel), &text) {
            Ok(()) => return Ok(rel),
            Err(Error::Invalid(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Error::Internal("could not name the import journal".into()))
}

/// Put back what one `apply` did, newest change first. Anything changed since is left
/// alone and reported as skipped. The journal is then renamed `…-undone.json`.
pub fn undo(root: &Path, journal_rel: &str) -> Result<RestructureReport> {
    let name_ok = journal_rel.starts_with(".vault-hq/import-")
        && journal_rel.ends_with(".json")
        && !journal_rel.ends_with("-undone.json")
        && !journal_rel.contains("..")
        && journal_rel.matches('/').count() == 1;
    if !name_ok {
        return Err(Error::Denied(format!(
            "{journal_rel} is not an import journal"
        )));
    }
    let jpath = root.join(journal_rel);
    let journal: Journal = serde_json::from_str(&std::fs::read_to_string(&jpath)?)?;
    let mut report = RestructureReport {
        applied: vec![],
        skipped: vec![],
        moved: vec![],
        edited: vec![],
        created: vec![],
        journal: None,
    };
    let skip = |report: &mut RestructureReport, rel: &str, reason: String| {
        report.skipped.push(SkippedItem {
            id: rel.to_string(),
            reason,
        })
    };
    for e in journal.entries.iter().rev() {
        match e {
            Entry::Moved { from, to } => {
                let (src, dst) = (checked(root, to)?, checked(root, from)?);
                if !src.exists() {
                    skip(
                        &mut report,
                        to,
                        format!("{to} is gone, so it can't move back"),
                    );
                } else if dst.exists() {
                    skip(
                        &mut report,
                        to,
                        format!("{from} exists again, so {to} stayed"),
                    );
                } else {
                    if let Some(p) = dst.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    std::fs::rename(&src, &dst)?;
                    report.moved.push(Moved {
                        from: to.clone(),
                        to: from.clone(),
                    });
                }
            }
            Entry::Edited {
                rel,
                before,
                after,
                hash,
            } => {
                let path = checked(root, rel)?;
                let r = locks::guarded_edit(&path, Some(hash), |cur| {
                    cur.strip_prefix(after.as_str())
                        .map(|rest| format!("{before}{rest}"))
                        .ok_or_else(|| Error::Invalid(format!("{rel} changed since")))
                });
                match r {
                    Ok(_) => {
                        if !report.edited.contains(rel) {
                            report.edited.push(rel.clone());
                        }
                    }
                    Err(Error::Conflict { .. }) => skip(
                        &mut report,
                        rel,
                        format!("{rel} was edited since; left as it is"),
                    ),
                    Err(e) => skip(&mut report, rel, e.to_string()),
                }
            }
            Entry::Created { rel, hash } => {
                let path = checked(root, rel)?;
                match hash {
                    Some(h) => match std::fs::read_to_string(&path) {
                        Ok(t) if sha256(&t) == *h => {
                            std::fs::remove_file(&path)?;
                            report.created.push(rel.clone());
                        }
                        Ok(_) => skip(&mut report, rel, format!("{rel} was edited since; kept")),
                        Err(_) => {}
                    },
                    None => {
                        // only an empty folder; anything put there since keeps it
                        if std::fs::remove_dir(&path).is_ok() {
                            report.created.push(format!("{rel}/"));
                        }
                    }
                }
            }
        }
    }
    let done = jpath.with_file_name(
        jpath
            .file_name()
            .map(|n| n.to_string_lossy().replace(".json", "-undone.json"))
            .unwrap_or_default(),
    );
    std::fs::rename(&jpath, &done)?;
    report.journal = Some(rel_of(root, &done));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::import::{analyze, tests::messy};
    use super::*;
    use crate::types::PlanItem;

    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| !e.path().starts_with(root.join(".vault-hq")))
            .map(|e| {
                let rel = rel_of(root, e.path());
                let body = if e.file_type().is_file() {
                    std::fs::read_to_string(e.path()).unwrap()
                } else {
                    "/".into()
                };
                (rel, body)
            })
            .collect();
        v.sort();
        v
    }

    fn tick(items: &mut [PlanItem], id: &str, on: bool) {
        items.iter_mut().find(|i| i.id == id).unwrap().ticked = on;
    }

    #[test]
    fn applies_only_ticked_and_undo_restores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        messy(root);
        let before = snapshot(root);
        let mut a = analyze(root, None).unwrap();
        tick(&mut a.plan.items, "tag:Notes/Cooking/Bread.md", true);
        tick(&mut a.plan.items, "fm:README.md", false);
        let r = apply(&a.plan).unwrap();
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);

        // folders merged into the scaffolded ones
        assert!(root.join("01-Projects/Garden.md").exists());
        assert!(root.join("01-Projects/AGENTS.md").exists());
        assert!(!root.join("Projects").exists());
        assert!(root.join("09-Planning/Daily/2026-10-01.md").exists());
        assert!(root.join("00-Inbox/Untitled.md").exists());
        // unticked: the unknown folder stays, README keeps no frontmatter
        assert!(root.join("Notes/Ideas.md").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "# readme stays\n"
        );
        // frontmatter then tags on one note, both applied, body untouched
        let bread = std::fs::read_to_string(root.join("Notes/Cooking/Bread.md")).unwrap();
        let p = frontmatter::parse(&bread);
        assert_eq!(p.props["author"], "human");
        assert_eq!(p.props["tags"], "[notes, cooking]");
        assert!(bread.ends_with("no frontmatter at all, [[Missing Recipe]]\n"));
        let shed = std::fs::read_to_string(root.join("01-Projects/Shed.md")).unwrap();
        assert!(shed.starts_with("---\ntitle: Shed\ntype: project\ncreated: "));
        assert!(r
            .moved
            .iter()
            .any(|m| m.from == "Untitled.md" && m.to == "00-Inbox/Untitled.md"));

        // undo puts every byte back
        let journal = r.journal.unwrap();
        let u = undo(root, &journal).unwrap();
        assert!(u.skipped.is_empty(), "{:?}", u.skipped);
        assert_eq!(snapshot(root), before);
        assert!(undo(root, &journal).is_err(), "a journal undoes once");
    }

    #[test]
    fn changed_notes_and_collisions_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        messy(root);
        let a = analyze(root, None).unwrap();
        // the user edits a note and creates a clash after the analysis
        std::fs::write(root.join("Untitled.md"), "edited meanwhile\n").unwrap();
        std::fs::create_dir_all(root.join("01-Projects")).unwrap();
        std::fs::write(root.join("01-Projects/Garden.md"), "a different garden\n").unwrap();
        let r = apply(&a.plan).unwrap();
        let reasons: HashMap<_, _> = r
            .skipped
            .iter()
            .map(|s| (s.id.clone(), s.reason.clone()))
            .collect();
        assert!(reasons["fm:Untitled.md"].contains("changed after the analysis"));
        assert!(reasons["mv:Projects/"].contains("01-Projects/Garden.md already exists"));
        assert_eq!(
            std::fs::read_to_string(root.join("01-Projects/Garden.md")).unwrap(),
            "a different garden\n"
        );
        // the clashing file stayed behind, the other one moved
        assert!(root.join("Projects/Garden.md").exists());
        assert!(root.join("01-Projects/Shed.md").exists());
        // the edited note still moved, unchanged
        assert_eq!(
            std::fs::read_to_string(root.join("00-Inbox/Untitled.md")).unwrap(),
            "edited meanwhile\n"
        );
    }

    #[test]
    fn undo_leaves_later_edits_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        messy(root);
        let mut a = analyze(root, None).unwrap();
        for i in a.plan.items.iter_mut() {
            i.ticked = i.id == "fm:Notes/Ideas.md" || i.id == "folders";
        }
        let r = apply(&a.plan).unwrap();
        std::fs::write(root.join("Notes/Ideas.md"), "rewritten by the user\n").unwrap();
        std::fs::write(root.join("00-Inbox/AGENTS.md"), "my inbox rules\n").unwrap();
        let u = undo(root, r.journal.as_deref().unwrap()).unwrap();
        assert_eq!(u.skipped.len(), 2, "{:?}", u.skipped);
        assert_eq!(
            std::fs::read_to_string(root.join("Notes/Ideas.md")).unwrap(),
            "rewritten by the user\n"
        );
        assert!(root.join("00-Inbox/AGENTS.md").exists());
        assert!(!root.join("02-Areas").exists(), "empty created folders go");
    }

    #[test]
    fn refuses_paths_outside_or_hidden() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../x", "/etc", ".obsidian/app.json", ""] {
            assert!(checked(dir.path(), bad).is_err(), "{bad}");
        }
        let plan = ImportPlan {
            root: dir.path().to_string_lossy().into_owned(),
            items: vec![PlanItem {
                id: "mv".into(),
                group: "x".into(),
                summary: String::new(),
                ticked: true,
                reversible: true,
                action: PlanAction::Move {
                    from: ".vault-hq".into(),
                    to: "x".into(),
                },
            }],
        };
        let r = apply(&plan).unwrap();
        assert_eq!(r.skipped.len(), 1);
        assert!(r.journal.is_none());
        assert!(undo(dir.path(), "../etc/passwd").is_err());
        assert!(undo(dir.path(), ".vault-hq/import-x-undone.json").is_err());
    }
}
