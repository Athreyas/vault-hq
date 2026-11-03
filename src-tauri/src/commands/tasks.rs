//! Task commands (SPEC 10, stream B). Thin: take the state's lock, call `hq_core::tasks`,
//! hand back its types or its error. Work runs on a blocking thread so a large vault
//! never holds up the window.
//!
//! Wiring: main.rs manages `TasksState::new(cache_dir)`; each command first lines the
//! state's vaults up with settings (primary first), and jobs.rs passes watcher changes
//! to `notes_changed`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use hq_core::settings::{self, Settings};
use hq_core::tasks::mutate::{self, Ctx};
use hq_core::tasks::{import_tw, query, resolve_today, stats, TaskIndex};
use hq_core::types::{
    StatsQuery, Task, TaskGroups, TaskImportReport, TaskInput, TaskPatch, TaskQuery, TaskStats,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use tauri::{AppHandle, Manager, Runtime};

use crate::state::Shell;

pub struct TasksState {
    inner: Mutex<Inner>,
}

struct Inner {
    vaults: Vec<Vault>,
    /// `None` until the first scan; the cache answers until then.
    index: Option<TaskIndex>,
    cache_dir: PathBuf,
}

impl TasksState {
    /// Starts from the cached index, so the first list shows before any scan.
    pub fn new(cache_dir: PathBuf) -> Self {
        let index = TaskIndex::load(&cache_dir);
        TasksState {
            inner: Mutex::new(Inner {
                vaults: Vec::new(),
                index,
                cache_dir,
            }),
        }
    }

    /// The open vaults, primary first (new tasks without a vault go there). Rescans only
    /// when the list actually changed.
    pub fn set_vaults(&self, vaults: Vec<Vault>) {
        if let Ok(mut g) = self.inner.lock() {
            let same = g.vaults.len() == vaults.len()
                && g.vaults
                    .iter()
                    .zip(&vaults)
                    .all(|(a, b)| a.id == b.id && a.root == b.root);
            if !same {
                g.vaults = vaults;
                g.rebuild();
            }
        }
    }

    /// The watcher saw these notes change.
    pub fn notes_changed(&self, vault_id: &str, rels: &[String]) {
        let Ok(mut g) = self.inner.lock() else { return };
        let Some(vault) = g.vaults.iter().find(|v| v.id == vault_id).cloned() else {
            return;
        };
        let index = g.index.get_or_insert_with(TaskIndex::default);
        for rel in rels {
            index.refresh_note(&vault, rel);
        }
        g.save();
    }

    /// Every task in the open vaults (W's task widgets read this), scanning first if
    /// nothing is indexed yet.
    pub fn all_tasks(&self) -> Result<Vec<Task>> {
        let mut g = self.lock()?;
        if g.vaults.is_empty() {
            return Err(Error::Unavailable("No vault is open.".into()));
        }
        Ok(g.index().tasks().cloned().collect())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| Error::Internal("The task index is unavailable; restart HQ.".into()))
    }
}

impl Inner {
    fn rebuild(&mut self) {
        self.index = Some(TaskIndex::build(&self.vaults));
        self.save();
    }

    /// The index, scanning first if there is none or the cache had other vaults.
    fn index(&mut self) -> &mut TaskIndex {
        if self.index.is_none() {
            self.rebuild();
        }
        self.index.get_or_insert_with(TaskIndex::default)
    }

    fn save(&self) {
        if let Some(i) = &self.index {
            // the cache only speeds up the next start; failing to write it loses nothing
            let _ = i.save(&self.cache_dir);
        }
    }

    fn mutate<T>(&mut self, f: impl FnOnce(&mut Ctx) -> Result<T>) -> Result<T> {
        if self.vaults.is_empty() {
            return Err(Error::Unavailable("No vault is open.".into()));
        }
        let today = resolve_today(None)?;
        let vaults = self.vaults.clone();
        let index = self.index();
        let mut ctx = Ctx {
            vaults: &vaults,
            index,
            today,
        };
        let out = f(&mut ctx);
        self.save();
        out
    }
}

/// The open vaults from settings, primary first, blocked ones never opened.
pub fn vaults_from(s: &Settings) -> Vec<Vault> {
    let mut out: Vec<Vault> = s
        .vaults
        .iter()
        .filter(|v| settings::blocked(s, Path::new(&v.path)).is_none())
        .filter_map(|v| Vault::open(Path::new(&v.path)).ok())
        .collect();
    if let Some(p) = &s.primary_vault {
        if let Some(i) = out.iter().position(|v| &v.id == p) {
            let v = out.remove(i);
            out.insert(0, v);
        }
    }
    out
}

/// Run `f` on a blocking thread with the task state, its vaults matching settings.
async fn with<R: Runtime, T: Send + 'static>(
    app: AppHandle<R>,
    f: impl FnOnce(&mut Inner) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let vaults = vaults_from(&app.state::<Shell>().settings());
        let state = app.state::<TasksState>();
        state.set_vaults(vaults);
        let mut g = state.lock()?;
        f(&mut g)
    })
    .await
    .map_err(|e| Error::Internal(format!("task worker stopped: {e}")))?
}

#[tauri::command]
pub async fn tasks_query<R: Runtime>(app: AppHandle<R>, query: TaskQuery) -> Result<TaskGroups> {
    with(app, move |g| query::run(g.index(), &query)).await
}

#[tauri::command]
pub async fn task_add<R: Runtime>(app: AppHandle<R>, input: TaskInput) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::add(c, &input))).await
}

#[tauri::command]
pub async fn task_update<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    patch: TaskPatch,
) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::update(c, &id, &patch))).await
}

#[tauri::command]
pub async fn task_done<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::done(c, &id))).await
}

#[tauri::command]
pub async fn task_cancel<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::cancel(c, &id))).await
}

#[tauri::command]
pub async fn task_delete<R: Runtime>(app: AppHandle<R>, id: String) -> Result<()> {
    with(app, move |g| g.mutate(|c| mutate::delete(c, &id))).await
}

#[tauri::command]
pub async fn task_annotate<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    text: String,
) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::annotate(c, &id, &text))).await
}

/// Starts the task, or stops it if it is already started.
#[tauri::command]
pub async fn task_start<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Task> {
    with(app, move |g| g.mutate(|c| mutate::start(c, &id))).await
}

#[tauri::command]
pub async fn tasks_stats<R: Runtime>(app: AppHandle<R>, query: StatsQuery) -> Result<TaskStats> {
    with(app, move |g| stats::run(g.index(), &query)).await
}

/// Read `task export` (only when the user asks: the onboarding/settings button) and
/// import it into the primary vault. Run with `dryRun: true` first to show the report.
#[tauri::command]
pub async fn tasks_import_taskwarrior<R: Runtime>(
    app: AppHandle<R>,
    dry_run: bool,
) -> Result<TaskImportReport> {
    with(app, move |g| {
        let bin = import_tw::find_task_binary().ok_or_else(|| {
            Error::Unavailable("Taskwarrior (the `task` program) is not installed.".into())
        })?;
        let json = import_tw::export_json(&bin)?;
        let vault = g
            .vaults
            .first()
            .cloned()
            .ok_or_else(|| Error::Unavailable("No vault is open.".into()))?;
        let today = resolve_today(None)?;
        let report = import_tw::import(&vault, g.index(), &json, dry_run, today)?;
        if !dry_run {
            g.save();
        }
        Ok(report)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_follows_notes_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let vdir = dir.path().join("v");
        std::fs::create_dir_all(&vdir).unwrap();
        std::fs::write(vdir.join("a.md"), "- [ ] one\n").unwrap();
        let v = Vault::open(&vdir).unwrap();
        let cache = dir.path().join("cache");
        let st = TasksState::new(cache.clone());
        st.set_vaults(vec![v.clone()]);
        assert!(cache.join(hq_core::tasks::CACHE_FILE).exists());
        std::fs::write(vdir.join("a.md"), "- [ ] one\n- [ ] two\n").unwrap();
        st.notes_changed(&v.id, &["a.md".into()]);
        let mut g = st.lock().unwrap();
        assert_eq!(g.index().tasks().count(), 2);
        let t = g
            .mutate(|c| {
                mutate::add(
                    c,
                    &TaskInput {
                        description: "three".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(t.note, format!("{}:09-Planning/Tasks.md", v.id));
    }

    #[test]
    fn primary_vault_comes_first_and_blocked_ones_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |name: &str| {
            let p = dir.path().join(name);
            std::fs::create_dir_all(&p).unwrap();
            let v = Vault::open(&p).unwrap();
            settings::VaultEntry {
                id: v.id,
                path: p.to_string_lossy().into_owned(),
                name: name.into(),
            }
        };
        let (a, b, c) = (mk("a"), mk("b"), mk("private-c"));
        let s = Settings {
            vaults: vec![a.clone(), b.clone(), c],
            primary_vault: Some(b.id.clone()),
            blocked_vault_names: vec!["private-c".into()],
            ..Default::default()
        };
        let ids: Vec<String> = vaults_from(&s).into_iter().map(|v| v.id).collect();
        assert_eq!(ids, vec![b.id, a.id]);
    }
}
