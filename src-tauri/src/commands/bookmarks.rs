//! Bookmark commands (SPEC 8, 10). Thin: take the settings from the shell, find the primary
//! vault, call `hq_core::bookmarks`, hand back its types. Long work runs off the main
//! thread and reports through `jobs://progress`. Every settings change (a browser's yes or
//! no, the memories state) goes through `Shell::update_settings`, so it is saved under
//! the shell's lock and never races another command's write.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use hq_core::bookmarks::{self, enrich, store};
use hq_core::ollama::{CancelToken, Chat, ChatMessage, Models};
use hq_core::settings::Settings;
use hq_core::types::{
    split_note_id, Bookmark, BookmarkFilter, BookmarkImportReport, BookmarkPage, BrowserSource,
    EnrichReport, JobProgress, Memory, VaultChanged,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::state::Shell;

static ENRICH_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Unavailable("HOME is not set, so browsers can't be found.".into()))
}

/// Bookmarks land in the primary vault (captures go there too). The core registry picks
/// it, with its blocked-name check on the whole path before anything inside is read.
pub fn primary_vault(s: &Settings) -> Result<Vault> {
    hq_core::vault::registry::Registry::load(s)
        .primary()
        .cloned()
        .map_err(|e| match e {
            Error::Unavailable(_) => {
                Error::Unavailable("Add a vault first: bookmarks are saved as notes in it.".into())
            }
            e => e,
        })
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Internal(format!("background work stopped: {e}")))?
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    job: &str,
    done: u32,
    total: u32,
    message: String,
) {
    let _ = app.emit(
        "jobs://progress",
        JobProgress {
            job: job.into(),
            done,
            total,
            message,
        },
    );
}

fn progress<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    job: &'static str,
) -> impl FnMut(u32, u32, &str) + 'a {
    move |done, total, message| {
        emit_progress(app, job, done, total, message.to_string());
    }
}

#[tauri::command]
pub async fn browsers_detect(shell: State<'_, Shell>) -> Result<Vec<BrowserSource>> {
    let s = shell.settings();
    blocking(move || Ok(bookmarks::detect(&home()?, &s))).await
}

/// Remember the user's yes or no for one browser profile.
#[tauri::command]
pub fn browser_allow(shell: State<'_, Shell>, source_id: String, allowed: bool) -> Result<()> {
    if source_id.starts_with("html:") {
        return Err(Error::Invalid(
            "An HTML file is imported directly; it has nothing to allow.".into(),
        ));
    }
    // refuse ids that don't name a browser profile before storing them
    bookmarks::browsers::resolve(&home()?, &source_id)?;
    shell.update_settings(|s| {
        s.browsers.insert(source_id, allowed);
        Ok(())
    })?;
    Ok(())
}

#[tauri::command]
pub async fn bookmarks_import<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    source_id: String,
    dry_run: bool,
) -> Result<BookmarkImportReport> {
    let s = shell.settings();
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
    shell: State<'_, Shell>,
    q: String,
    filter: Option<BookmarkFilter>,
) -> Result<BookmarkPage> {
    let s = shell.settings();
    blocking(move || {
        let vault = primary_vault(&s)?;
        Ok(bookmarks::query(&vault, &q, &filter.unwrap_or_default()))
    })
    .await
}

/// Save a pasted link at once; its page title and description are fetched afterwards,
/// for that URL only, and announced with `vault://changed`.
#[tauri::command]
pub fn bookmark_add<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    url: String,
    note: Option<String>,
) -> Result<Bookmark> {
    let vault = primary_vault(&shell.settings())?;
    let (b, created) = store::add(&vault, &url, note.as_deref())?;
    if created {
        let id = b.id.clone();
        let url = b.url.clone();
        std::thread::spawn(move || {
            let Ok(meta) = enrich::fetch_page(&url) else {
                return; // offline or the page refused: the note keeps the URL as its title
            };
            if let Ok(updated) = enrich::apply_page(&vault, &id, &meta) {
                if let Some((_, rel)) = split_note_id(&updated.id) {
                    crate::jobs::vault_changed(
                        &app,
                        VaultChanged {
                            vault: vault.id.clone(),
                            rels: vec![rel.to_string()],
                        },
                    );
                }
            }
        });
    }
    Ok(b)
}

/// One prompt in, the whole reply out, from the user's chat model (stream C's client,
/// which refuses any server that isn't this Mac or the home network).
fn ask_model(models: &Models, prompt: &str) -> Result<String> {
    let messages = [ChatMessage::new("user", prompt)];
    let done = models
        .chat()
        .chat_stream(&messages, &CancelToken::new(), &mut |_| {})?;
    Ok(done.text)
}

#[tauri::command]
pub async fn bookmarks_enrich<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    ids: Option<Vec<String>>,
) -> Result<EnrichReport> {
    let s = shell.settings();
    if !s.enrich_bookmarks {
        return Err(Error::Denied(
            "Bookmark enrichment is off. Turn it on in Settings to let the local model tag and summarise bookmarks."
                .into(),
        ));
    }
    if s.ollama.chat_model.trim().is_empty() {
        return Err(Error::Unavailable(
            "Pick a chat model under Local AI first.".into(),
        ));
    }
    ENRICH_CANCEL.store(false, Ordering::Relaxed);
    blocking(move || {
        let vault = primary_vault(&s)?;
        let models = Models::new(&s.ollama)?;
        let model = |prompt: &str| ask_model(&models, prompt);
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
/// 1, 2, … are "show different ones". What was shown is recorded in settings.
#[tauri::command]
pub fn memories(shell: State<'_, Shell>, date: String, salt: Option<u32>) -> Result<Vec<Memory>> {
    let vault = primary_vault(&shell.settings())?;
    let mut picked = Vec::new();
    shell.update_settings(|s| {
        picked = bookmarks::memories(&vault, &mut s.memories, &date, salt.unwrap_or(0));
        Ok(())
    })?;
    Ok(picked)
}

#[tauri::command]
pub fn memory_dismiss(shell: State<'_, Shell>, id: String) -> Result<()> {
    shell.update_settings(|s| {
        bookmarks::memory_dismiss(&mut s.memories, &id);
        Ok(())
    })?;
    Ok(())
}

/// The hourly re-scan (SPEC 8): every allowed browser profile, into the primary vault.
/// Quiet when nothing is allowed or there is no vault yet.
pub fn rescan<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    if !s.browsers.values().any(|on| *on) {
        return;
    }
    let (Ok(vault), Ok(home)) = (primary_vault(s), home()) else {
        return;
    };
    for r in bookmarks::rescan_allowed(&vault, &home, s) {
        for e in &r.errors {
            eprintln!("vault-hq: bookmark re-scan of {}: {e}", r.source_id);
        }
        if r.added + r.updated + r.removed > 0 {
            let message = format!(
                "{}: {} new, {} updated, {} gone",
                r.source_id, r.added, r.updated, r.removed
            );
            emit_progress(app, "bookmarks-rescan", 1, 1, message);
        }
    }
}
