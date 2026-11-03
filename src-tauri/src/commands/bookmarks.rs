//! Bookmark commands (SPEC 8, 10). Thin: find the settings and the primary vault, call
//! `hq_core::bookmarks`, hand back its types. Long work runs off the main thread and
//! reports through `jobs://progress`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use hq_core::bookmarks::{self, enrich, store};
use hq_core::settings::{self, Settings};
use hq_core::types::{
    Bookmark, BookmarkFilter, BookmarkImportReport, BookmarkPage, BrowserSource, EnrichReport,
    Memory,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

/// Settings are read-modify-written here; one at a time so two clicks can't lose a choice.
static SETTINGS: Mutex<()> = Mutex::new(());
static ENRICH_CANCEL: AtomicBool = AtomicBool::new(false);

fn store_of(app: &AppHandle) -> Result<settings::Store> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::Internal(format!("no app support folder: {e}")))?;
    Ok(settings::Store::new(&dir))
}

fn load_settings(app: &AppHandle) -> Result<Settings> {
    store_of(app)?.load()
}

/// Change settings under the lock and save them.
fn edit_settings<T>(app: &AppHandle, f: impl FnOnce(&mut Settings) -> T) -> Result<T> {
    let _guard = SETTINGS.lock().unwrap_or_else(|p| p.into_inner());
    let store = store_of(app)?;
    let mut s = store.load()?;
    let out = f(&mut s);
    store.save(&s)?;
    Ok(out)
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Unavailable("HOME is not set, so browsers can't be found.".into()))
}

/// Bookmarks land in the primary vault (captures go there too).
fn primary_vault(s: &Settings) -> Result<Vault> {
    let entry = s
        .primary_vault
        .as_ref()
        .and_then(|id| s.vaults.iter().find(|v| &v.id == id))
        .or_else(|| s.vaults.first())
        .ok_or_else(|| {
            Error::Unavailable("Add a vault first: bookmarks are saved as notes in it.".into())
        })?;
    let root = PathBuf::from(&entry.path);
    if let Some(why) = settings::blocked(s, &root) {
        return Err(Error::Denied(why));
    }
    Vault::open(&root)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Internal(format!("background work stopped: {e}")))?
}

fn progress<'a>(app: &'a AppHandle, job: &'static str) -> impl FnMut(u32, u32, &str) + 'a {
    move |done, total, message| {
        let _ = app.emit(
            "jobs://progress",
            json!({ "job": job, "done": done, "total": total, "message": message }),
        );
    }
}

#[tauri::command]
pub async fn browsers_detect(app: AppHandle) -> Result<Vec<BrowserSource>> {
    let s = load_settings(&app)?;
    blocking(move || Ok(bookmarks::detect(&home()?, &s))).await
}

/// Remember the user's yes or no for one browser profile.
#[tauri::command]
pub fn browser_allow(app: AppHandle, source_id: String, allowed: bool) -> Result<()> {
    // refuse ids that don't name a browser profile before storing them
    bookmarks::browsers::resolve(&home()?, &source_id)?;
    if source_id.starts_with("html:") {
        return Err(Error::Invalid(
            "An HTML file is imported directly; it has nothing to allow.".into(),
        ));
    }
    edit_settings(&app, |s| {
        s.browsers.insert(source_id, allowed);
    })
}

#[tauri::command]
pub async fn bookmarks_import(
    app: AppHandle,
    source_id: String,
    dry_run: bool,
) -> Result<BookmarkImportReport> {
    let s = load_settings(&app)?;
    blocking(move || {
        let vault = primary_vault(&s)?;
        bookmarks::import(
            &vault,
            &home()?,
            &s,
            &source_id,
            dry_run,
            &mut progress(&app, "bookmarks-import"),
        )
    })
    .await
}

#[tauri::command]
pub async fn bookmarks_query(
    app: AppHandle,
    q: String,
    filter: Option<BookmarkFilter>,
) -> Result<BookmarkPage> {
    let s = load_settings(&app)?;
    blocking(move || {
        let vault = primary_vault(&s)?;
        Ok(bookmarks::query(&vault, &q, &filter.unwrap_or_default()))
    })
    .await
}

/// Save a pasted link at once; its page title and description are fetched afterwards,
/// for that URL only, and announced with `vault://changed`.
#[tauri::command]
pub fn bookmark_add(app: AppHandle, url: String, note: Option<String>) -> Result<Bookmark> {
    let s = load_settings(&app)?;
    let vault = primary_vault(&s)?;
    let (b, created) = store::add(&vault, &url, note.as_deref())?;
    if created {
        let id = b.id.clone();
        let url = b.url.clone();
        std::thread::spawn(move || {
            let Ok(meta) = enrich::fetch_page(&url) else {
                return; // offline or the page refused: the note keeps the URL as its title
            };
            if let Ok(updated) = enrich::apply_page(&vault, &id, &meta) {
                let rel = hq_core::types::split_note_id(&updated.id)
                    .map(|(_, r)| r.to_string())
                    .unwrap_or_default();
                let _ = app.emit(
                    "vault://changed",
                    json!({ "vault": vault.id, "rels": [rel] }),
                );
            }
        });
    }
    Ok(b)
}

/// The chat model that writes tags and summaries. Stream C's client is wired in here by
/// the integrator; until then enrichment reports the model as unavailable.
fn chat_model(_s: &Settings) -> impl Fn(&str) -> Result<String> {
    |_prompt: &str| {
        Err(Error::Unavailable(
            "The local model isn't connected yet. Set it up under Local AI.".into(),
        ))
    }
}

#[tauri::command]
pub async fn bookmarks_enrich(app: AppHandle, ids: Option<Vec<String>>) -> Result<EnrichReport> {
    let s = load_settings(&app)?;
    if !s.enrich_bookmarks {
        return Err(Error::Denied(
            "Bookmark enrichment is off. Turn it on in Settings to let the local model tag and summarise bookmarks."
                .into(),
        ));
    }
    ENRICH_CANCEL.store(false, Ordering::Relaxed);
    blocking(move || {
        let vault = primary_vault(&s)?;
        let model = chat_model(&s);
        enrich::enrich(
            &vault,
            ids.as_deref(),
            &model,
            &ENRICH_CANCEL,
            &mut progress(&app, "bookmarks-enrich"),
        )
    })
    .await
}

/// Stop a running enrichment after the bookmark it is on.
#[tauri::command]
pub fn bookmarks_enrich_cancel() {
    ENRICH_CANCEL.store(true, Ordering::Relaxed);
}

/// Home's daily memories. `date` is the UI's local `YYYY-MM-DD`; `salt` 0 is today's pick,
/// 1, 2, … are "show different ones".
#[tauri::command]
pub fn memories(app: AppHandle, date: String, salt: Option<u32>) -> Result<Vec<Memory>> {
    let s = load_settings(&app)?;
    let vault = primary_vault(&s)?;
    edit_settings(&app, |s| {
        bookmarks::memories(&vault, &mut s.memories, &date, salt.unwrap_or(0))
    })
}

#[tauri::command]
pub fn memory_dismiss(app: AppHandle, id: String) -> Result<()> {
    edit_settings(&app, |s| bookmarks::memory_dismiss(&mut s.memories, &id))
}
