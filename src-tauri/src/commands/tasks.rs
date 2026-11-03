//! Task commands (SPEC 10, stream B). Thin: take the state's lock, call `hq_core::tasks`,
//! hand back its types or its error.
//!
//! Wiring (for the shell): `.manage(TasksState::new(cache_dir))` with the app's cache dir,
//! call `set_vaults` whenever the open vaults change and `notes_changed` from the watcher;
//! register the commands below.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use hq_core::tasks::mutate::{self, Ctx};
use hq_core::tasks::{import_tw, query, resolve_today, stats, TaskIndex};
use hq_core::types::{
    StatsQuery, Task, TaskGroups, TaskImportReport, TaskInput, TaskPatch, TaskQuery, TaskStats,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use tauri::State;

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

    /// The open vaults, primary first (new tasks without a vault go there).
    pub fn set_vaults(&self, vaults: Vec<Vault>) {
        if let Ok(mut g) = self.inner.lock() {
            g.vaults = vaults;
            g.rebuild();
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

#[tauri::command]
pub async fn tasks_query(state: State<'_, TasksState>, query: TaskQuery) -> Result<TaskGroups> {
    let mut g = state.lock()?;
    query::run(g.index(), &query)
}

#[tauri::command]
pub async fn task_add(state: State<'_, TasksState>, input: TaskInput) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::add(c, &input))
}

#[tauri::command]
pub async fn task_update(
    state: State<'_, TasksState>,
    id: String,
    patch: TaskPatch,
) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::update(c, &id, &patch))
}

#[tauri::command]
pub async fn task_done(state: State<'_, TasksState>, id: String) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::done(c, &id))
}

#[tauri::command]
pub async fn task_cancel(state: State<'_, TasksState>, id: String) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::cancel(c, &id))
}

#[tauri::command]
pub async fn task_delete(state: State<'_, TasksState>, id: String) -> Result<()> {
    state.lock()?.mutate(|c| mutate::delete(c, &id))
}

#[tauri::command]
pub async fn task_annotate(
    state: State<'_, TasksState>,
    id: String,
    text: String,
) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::annotate(c, &id, &text))
}

/// Starts the task, or stops it if it is already started.
#[tauri::command]
pub async fn task_start(state: State<'_, TasksState>, id: String) -> Result<Task> {
    state.lock()?.mutate(|c| mutate::start(c, &id))
}

#[tauri::command]
pub async fn tasks_stats(state: State<'_, TasksState>, query: StatsQuery) -> Result<TaskStats> {
    let mut g = state.lock()?;
    stats::run(g.index(), &query)
}

/// Read `task export` (only when the user asks: this is the onboarding/settings button)
/// and import it into the primary vault. Run with `dryRun: true` first to show the report.
#[tauri::command]
pub async fn tasks_import_taskwarrior(
    state: State<'_, TasksState>,
    dry_run: bool,
) -> Result<TaskImportReport> {
    let bin = import_tw::find_task_binary().ok_or_else(|| {
        Error::Unavailable("Taskwarrior (the `task` program) is not installed.".into())
    })?;
    let json = import_tw::export_json(&bin)?;
    let mut g = state.lock()?;
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
}
