//! Tasks engine (SPEC 4). Owned by stream B.
//!
//! The checkbox lines in the notes are the database. [`TaskIndex`] is what was read from
//! them, rebuildable at any time: from a scan, note by note as the watcher reports
//! changes, or from `tasks-index.json` in the cache dir to answer before the first scan.
//! Every write goes through [`mutate`], keyed by `🆔`, under `locks::guarded_edit`.

pub mod import_tw;
pub mod model;
pub mod mutate;
pub mod parse;
pub mod query;
pub mod recur;
pub mod stats;
pub mod urgency;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::locks::sha256;
use crate::types::{note_id, Task, TaskStatus};
use crate::util;
use crate::vault::{scan, Note, Vault};

use model::FieldKind;
use parse::Found;

pub const CACHE_FILE: &str = "tasks-index.json";
// 2: tasks carry `dueTime` (⏰)
const CACHE_VERSION: u32 = 2;

pub fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

pub fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `today` from a query, or the local date.
pub fn resolve_today(today: Option<&str>) -> Result<NaiveDate> {
    let s = today.map(str::to_string).unwrap_or_else(util::today);
    date(&s).ok_or_else(|| Error::Invalid(format!("\"{s}\" is not a date (YYYY-MM-DD)")))
}

/// A `YYYY-MM-DD` from the UI or an agent, checked.
pub fn check_date(field: &str, s: &str) -> Result<String> {
    date(s).map(fmt).ok_or_else(|| {
        Error::Invalid(format!(
            "The {field} date \"{s}\" is not a date; use YYYY-MM-DD."
        ))
    })
}

/// Notes whose tasks count (SPEC 4.2): not the archive, templates, or agent session and
/// compaction notes in the inbox.
pub fn in_scope(rel: &str, props: &BTreeMap<String, String>) -> bool {
    if rel.starts_with("08-Archive/") || rel.starts_with("_templates/") {
        return false;
    }
    if rel.starts_with("00-Inbox/") {
        let ty = props.get("type").map(String::as_str).unwrap_or("");
        let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
        if matches!(ty, "session" | "compact")
            || name.contains("session")
            || name.contains("compact")
        {
            return false;
        }
    }
    true
}

/// The key of a task without `🆔`: its note and a hash of the line, numbered when the same
/// line appears more than once in the note.
pub fn content_key(note: &str, raw: &str, occurrence: usize) -> String {
    let h = &sha256(raw.trim_end())[..12];
    if occurrence == 0 {
        format!("{note}#{h}")
    } else {
        format!("{note}#{h}.{occurrence}")
    }
}

/// Keys for every task in a parsed note, in order.
pub fn keys_for(note: &str, found: &[Found]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    found
        .iter()
        .map(|f| match f.task.get(FieldKind::Id) {
            Some(id) => id.to_string(),
            None => {
                let n = seen.entry(f.raw.trim_end()).or_insert(0);
                let k = content_key(note, &f.raw, *n);
                *n += 1;
                k
            }
        })
        .collect()
}

/// What a note says about the tasks in it.
pub struct NoteMeta<'a> {
    pub vault: &'a str,
    pub rel: &'a str,
    pub stem: &'a str,
    pub title: &'a str,
    pub is_project: bool,
}

impl<'a> NoteMeta<'a> {
    pub fn of(vault: &'a str, note: &'a Note) -> Self {
        NoteMeta {
            vault,
            rel: &note.rel,
            stem: &note.stem,
            title: note.title(),
            is_project: note.props.get("type").is_some_and(|t| t == "project"),
        }
    }
}

/// Turn parsed lines into tasks. `blocked`, `blocking`, `hidden` and `urgency` are filled
/// in by [`finish`], which needs every task and today's date.
pub fn to_tasks(meta: &NoteMeta, found: &[Found]) -> Vec<Task> {
    let nid = note_id(meta.vault, meta.rel);
    let keys = keys_for(&nid, found);
    found
        .iter()
        .zip(keys)
        .map(|(f, key)| {
            let t = &f.task;
            let all_tags = t.tags();
            let tag_project = all_tags
                .iter()
                .find_map(|x| x.strip_prefix("project/"))
                .filter(|p| !p.is_empty())
                .map(str::to_string);
            let project = tag_project.or_else(|| meta.is_project.then(|| meta.stem.to_string()));
            let get = |k| t.get(k).map(str::to_string);
            Task {
                key,
                id: get(FieldKind::Id),
                vault: meta.vault.to_string(),
                note: nid.clone(),
                note_title: meta.title.to_string(),
                line: (f.line + 1) as u32,
                status: t.status(),
                description: t.description(),
                tags: all_tags
                    .into_iter()
                    .filter(|x| !x.starts_with("project/"))
                    .collect(),
                project,
                priority: t.priority(),
                created: get(FieldKind::Created),
                start: get(FieldKind::Start),
                scheduled: get(FieldKind::Scheduled),
                due: get(FieldKind::Due),
                due_time: get(FieldKind::DueTime),
                done: get(FieldKind::Done),
                cancelled: get(FieldKind::Cancelled),
                recur: get(FieldKind::Recur),
                depends: t.depends(),
                annotations: f.annotations.clone(),
                blocked: false,
                blocking: false,
                hidden: false,
                urgency: 0.0,
                raw: f.raw.clone(),
            }
        })
        .collect()
}

/// Fill in what depends on other tasks and on today: blocked/blocking (within a vault),
/// hidden waiting tasks, urgency.
pub fn finish(tasks: &mut [Task], today: NaiveDate) {
    let mut open: HashSet<(String, String)> = HashSet::new();
    let mut depended: HashSet<(String, String)> = HashSet::new();
    for t in tasks.iter() {
        if t.status.is_open() {
            if let Some(id) = &t.id {
                open.insert((t.vault.clone(), id.clone()));
            }
            for d in &t.depends {
                depended.insert((t.vault.clone(), d.clone()));
            }
        }
    }
    for t in tasks.iter_mut() {
        let is_open = t.status.is_open();
        t.blocked = is_open
            && t.depends
                .iter()
                .any(|d| open.contains(&(t.vault.clone(), d.clone())));
        t.blocking = is_open
            && t.id
                .as_ref()
                .is_some_and(|id| depended.contains(&(t.vault.clone(), id.clone())));
        t.hidden = t.status == TaskStatus::Waiting
            && t.scheduled
                .as_deref()
                .and_then(date)
                .is_some_and(|s| s > today);
        t.urgency = urgency::urgency(t, today);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct NoteTasks {
    vault: String,
    rel: String,
    hash: String,
    tasks: Vec<Task>,
}

/// Every task HQ knows about, by note. A cache of the notes, never the source of truth.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskIndex {
    version: u32,
    /// By note id.
    notes: BTreeMap<String, NoteTasks>,
}

impl TaskIndex {
    /// Scan every vault and read its tasks.
    pub fn build(vaults: &[Vault]) -> TaskIndex {
        let mut idx = TaskIndex {
            version: CACHE_VERSION,
            ..Default::default()
        };
        for v in vaults {
            idx.add_vault(v, &scan(v).notes);
        }
        idx
    }

    /// (Re)read one vault's tasks from notes it scanned.
    pub fn add_vault(&mut self, vault: &Vault, notes: &[Note]) {
        self.version = CACHE_VERSION;
        self.remove_vault(&vault.id);
        for n in notes {
            if !in_scope(&n.rel, &n.props) {
                continue;
            }
            let mut found = parse::parse_lines(&n.body);
            if found.is_empty() {
                continue;
            }
            // line numbers count from the top of the file, frontmatter included
            if let Ok(text) = std::fs::read_to_string(vault.root.join(&n.rel)) {
                if text.ends_with(&n.body) {
                    let skip = text[..text.len() - n.body.len()].matches('\n').count();
                    for f in &mut found {
                        f.line += skip;
                        f.end += skip;
                    }
                }
            }
            let tasks = to_tasks(&NoteMeta::of(&vault.id, n), &found);
            self.notes.insert(
                note_id(&vault.id, &n.rel),
                NoteTasks {
                    vault: vault.id.clone(),
                    rel: n.rel.clone(),
                    hash: n.hash.clone(),
                    tasks,
                },
            );
        }
    }

    pub fn remove_vault(&mut self, vault_id: &str) {
        self.notes.retain(|_, n| n.vault != vault_id);
    }

    /// Re-read one note after it changed (or drop it if it is gone or out of scope).
    pub fn refresh_note(&mut self, vault: &Vault, rel: &str) {
        self.version = CACHE_VERSION;
        let id = note_id(&vault.id, rel);
        self.notes.remove(&id);
        let Ok(path) = vault.resolve_rel(rel) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let fm = crate::frontmatter::parse(&text);
        if !in_scope(rel, &fm.props) {
            return;
        }
        let found = parse::parse_note(&text);
        if found.is_empty() {
            return;
        }
        let stem = Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = fm
            .props
            .get("title")
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or_else(|| stem.clone());
        let meta = NoteMeta {
            vault: &vault.id,
            rel,
            stem: &stem,
            title: &title,
            is_project: fm.props.get("type").is_some_and(|t| t == "project"),
        };
        let tasks = to_tasks(&meta, &found);
        self.notes.insert(
            id,
            NoteTasks {
                vault: vault.id.clone(),
                rel: rel.to_string(),
                hash: sha256(&text),
                tasks,
            },
        );
    }

    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.notes.values().flat_map(|n| n.tasks.iter())
    }

    /// Every task with blocked/blocking/hidden/urgency filled in for `today`.
    pub fn finished(&self, today: NaiveDate) -> Vec<Task> {
        let mut all: Vec<Task> = self.tasks().cloned().collect();
        finish(&mut all, today);
        all
    }

    /// The `🆔`s already used in a vault.
    pub fn ids(&self, vault_id: &str) -> HashSet<String> {
        self.notes
            .values()
            .filter(|n| n.vault == vault_id)
            .flat_map(|n| n.tasks.iter().filter_map(|t| t.id.clone()))
            .collect()
    }

    /// Where a task key points: `(vault id, rel)`.
    pub fn locate(&self, key: &str) -> Option<(String, String)> {
        let (vault, id) = match split_key(key) {
            Key::Content { note, .. } => {
                let (v, r) = crate::types::split_note_id(note)?;
                return Some((v.to_string(), r.to_string()));
            }
            Key::Id { vault, id } => (vault, id),
        };
        let first = self.notes.values().find(|n| {
            vault.is_none_or(|v| v == n.vault)
                && n.tasks.iter().any(|t| t.id.as_deref() == Some(id))
        })?;
        Some((first.vault.clone(), first.rel.clone()))
    }

    /// Vaults where a bare `🆔` appears (more than one makes the id ambiguous).
    pub fn vaults_with_id(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .notes
            .values()
            .filter(|n| n.tasks.iter().any(|t| t.id.as_deref() == Some(id)))
            .map(|n| n.vault.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    pub fn note_tasks(&self, vault_id: &str, rel: &str) -> &[Task] {
        self.notes
            .get(&note_id(vault_id, rel))
            .map(|n| n.tasks.as_slice())
            .unwrap_or(&[])
    }

    /// The cached index, if there is one this build can read.
    pub fn load(cache_dir: &Path) -> Option<TaskIndex> {
        let text = std::fs::read_to_string(cache_dir.join(CACHE_FILE)).ok()?;
        let idx: TaskIndex = serde_json::from_str(&text).ok()?;
        (idx.version == CACHE_VERSION).then_some(idx)
    }

    pub fn save(&self, cache_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(cache_dir)?;
        let tmp = cache_dir.join(format!("{CACHE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        // the cache is ours alone, so an atomic rename is the right way to replace it
        std::fs::rename(&tmp, cache_dir.join(CACHE_FILE))?;
        Ok(())
    }
}

pub enum Key<'a> {
    /// `🆔`, optionally `<vault>:<id>` to pick a vault.
    Id { vault: Option<&'a str>, id: &'a str },
    /// `<noteId>#<hash>[.<n>]`.
    Content {
        note: &'a str,
        hash: &'a str,
        n: usize,
    },
}

pub fn split_key(key: &str) -> Key<'_> {
    if let Some((note, h)) = key.rsplit_once('#') {
        let (hash, n) = match h.split_once('.') {
            Some((h, n)) => (h, n.parse().unwrap_or(usize::MAX)),
            None => (h, 0),
        };
        return Key::Content { note, hash, n };
    }
    match key.split_once(':') {
        Some((v, id)) => Key::Id { vault: Some(v), id },
        None => Key::Id {
            vault: None,
            id: key,
        },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A task from one line, as if it sat in a plain note.
    pub fn task(line: &str) -> Task {
        let found = parse::parse_lines(line);
        assert_eq!(found.len(), 1, "{line}");
        let meta = NoteMeta {
            vault: "v",
            rel: "n.md",
            stem: "n",
            title: "n",
            is_project: false,
        };
        to_tasks(&meta, &found).remove(0)
    }

    pub fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn scope() {
        let none = BTreeMap::new();
        let mut session = BTreeMap::new();
        session.insert("type".to_string(), "session".to_string());
        assert!(in_scope("01-Projects/a.md", &none));
        assert!(in_scope("00-Inbox/2026-11-01-capture.md", &none));
        assert!(!in_scope("00-Inbox/2026-11-01-claude.md", &session));
        assert!(!in_scope("00-Inbox/2026-11-01-session-x.md", &none));
        assert!(!in_scope("00-Inbox/compact-2026.md", &none));
        assert!(!in_scope("08-Archive/old.md", &none));
    }

    #[test]
    fn index_reads_projects_lines_and_dependencies() {
        let (_d, v) = vault_with(&[
            (
                "01-Projects/garden.md",
                "---\ntitle: Garden\ntype: project\n---\n\n## Tasks\n- [ ] Dig 🆔 aaa111\n- [ ] Plant ⛔ aaa111\n",
            ),
            (
                "09-Planning/Tasks.md",
                "- [ ] Loose #project/house #home\n- [x] Old ✅ 2026-01-01\n",
            ),
            ("08-Archive/x.md", "- [ ] archived\n"),
        ]);
        let idx = TaskIndex::build(std::slice::from_ref(&v));
        let all = idx.finished(date("2026-11-02").unwrap());
        assert_eq!(all.len(), 4);
        let dig = all.iter().find(|t| t.description == "Dig").unwrap();
        assert_eq!(dig.project.as_deref(), Some("garden"));
        assert_eq!(dig.note_title, "Garden");
        assert_eq!(dig.line, 7);
        assert!(dig.blocking && !dig.blocked);
        let plant = all.iter().find(|t| t.description == "Plant").unwrap();
        assert!(plant.blocked);
        assert!(plant
            .key
            .starts_with(&format!("{}:01-Projects/garden.md#", v.id)));
        let loose = all.iter().find(|t| t.description == "Loose").unwrap();
        assert_eq!(loose.project.as_deref(), Some("house"));
        assert_eq!(loose.tags, vec!["home"]);
        assert_eq!(
            idx.locate("aaa111"),
            Some((v.id.clone(), "01-Projects/garden.md".into()))
        );
        assert_eq!(idx.locate(&plant.key).unwrap().1, "01-Projects/garden.md");
        assert_eq!(idx.ids(&v.id).len(), 1);
    }

    #[test]
    fn duplicate_lines_get_distinct_keys() {
        let found = parse::parse_lines("- [ ] same\n- [ ] same\n- [ ] other 🆔 abc123\n");
        let keys = keys_for("v:n.md", &found);
        assert_ne!(keys[0], keys[1]);
        assert!(keys[1].ends_with(".1"));
        assert_eq!(keys[2], "abc123");
    }

    #[test]
    fn cache_round_trip_and_refresh() {
        let (_d, v) = vault_with(&[("a.md", "- [ ] one\n")]);
        let mut idx = TaskIndex::build(std::slice::from_ref(&v));
        let cache = tempfile::tempdir().unwrap();
        idx.save(cache.path()).unwrap();
        assert_eq!(TaskIndex::load(cache.path()), Some(idx.clone()));
        std::fs::write(v.root.join("a.md"), "- [ ] one\n- [ ] two\n").unwrap();
        idx.refresh_note(&v, "a.md");
        assert_eq!(idx.tasks().count(), 2);
        std::fs::remove_file(v.root.join("a.md")).unwrap();
        idx.refresh_note(&v, "a.md");
        assert_eq!(idx.tasks().count(), 0);
        assert!(TaskIndex::load(Path::new("/nonexistent-cache")).is_none());
    }

    #[test]
    fn waiting_is_hidden_until_its_date() {
        let mut ts = vec![
            task("- [>] later ⏳ 2026-12-01"),
            task("- [>] now ⏳ 2026-11-01"),
        ];
        finish(&mut ts, date("2026-11-02").unwrap());
        assert!(ts[0].hidden);
        assert!(!ts[1].hidden);
    }
}
