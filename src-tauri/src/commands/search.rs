//! Search, Ask and local-AI commands (SPEC 10, "search" and "ai"). Thin: read settings
//! and vaults, call `hq_core`, hand back its types and errors.
//!
//! Every command runs its work on a blocking thread, so a slow model server never holds
//! up the window. `ask_start` returns an id at once and streams `ask://event` envelopes
//! (`AskEnvelope`) from its own thread; `ask_cancel(id)` stops it between tokens.
//!
//! State is kept in a process-wide static rather than Tauri-managed state, so registering
//! these commands is the only wiring `main.rs` needs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use hq_core::ollama::{self, CancelToken, Chat, Models};
use hq_core::search::{self, ask, conversations, Index, Semantic, TOP_K};
use hq_core::settings::{self, Settings, Store};
use hq_core::types::{
    AskEnvelope, AskEvent, Conversation, ConversationSummary, OllamaStatus, Recommendation,
    SearchResult, SearchScope,
};
use hq_core::vault::{scan, Vault};
use hq_core::{util, Error, Result};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const ASK_EVENT: &str = "ask://event";

#[derive(Default)]
struct State {
    /// Swapped, not locked across work: readers clone the Arc and let go of the lock.
    index: Mutex<Arc<Index>>,
    asks: Mutex<HashMap<String, CancelToken>>,
}

fn state() -> &'static State {
    static S: OnceLock<State> = OnceLock::new();
    S.get_or_init(State::default)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // a panic elsewhere must not take search down with it
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Ctx {
    settings: Settings,
    vaults: Vec<Vault>,
    cache: PathBuf,
}

impl Ctx {
    fn load<R: Runtime>(app: &AppHandle<R>) -> Result<Ctx> {
        let path_err = |e: tauri::Error| Error::Internal(format!("No app folder: {e}"));
        let support = app.path().app_data_dir().map_err(path_err)?;
        let cache = app.path().app_cache_dir().map_err(path_err)?;
        let settings = Store::new(&support).load()?;
        let vaults = settings
            .vaults
            .iter()
            .filter(|v| settings::blocked(&settings, std::path::Path::new(&v.path)).is_none())
            .filter_map(|v| Vault::open(std::path::Path::new(&v.path)).ok())
            .collect();
        Ok(Ctx {
            settings,
            vaults,
            cache,
        })
    }

    fn primary(&self) -> Option<&Vault> {
        self.settings
            .primary_vault
            .as_ref()
            .and_then(|p| self.vaults.iter().find(|v| &v.id == p))
            .or_else(|| self.vaults.first())
    }

    /// Re-read the vaults and bring the index up to date; unchanged vaults cost a scan only.
    fn index(&self) -> Arc<Index> {
        let scans: Vec<_> = self
            .vaults
            .iter()
            .map(|v| (v.id.clone(), scan(v)))
            .collect();
        let ids: Vec<String> = scans.iter().map(|(id, _)| id.clone()).collect();
        let mut guard = lock(&state().index);
        let idx = Arc::make_mut(&mut guard);
        idx.retain_vaults(&ids);
        for (id, s) in &scans {
            idx.update_vault(id, &s.notes);
        }
        guard.clone()
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Internal(format!("The search worker stopped: {e}")))?
}

#[tauri::command]
pub async fn search<R: Runtime>(
    app: AppHandle<R>,
    q: String,
    scope: Option<SearchScope>,
) -> Result<SearchResult> {
    blocking(move || {
        let ctx = Ctx::load(&app)?;
        let index = ctx.index();
        let models = Models::new(&ctx.settings.ollama).ok();
        let embedder = models.as_ref().map(Models::embedder);
        let semantic = embedder
            .as_ref()
            .filter(|_| !ctx.settings.ollama.embed_model.is_empty())
            .map(|e| Semantic {
                embedder: e,
                cache_dir: &ctx.cache,
            });
        search::search(
            &index,
            &q,
            &scope.unwrap_or_default(),
            TOP_K * 2,
            semantic,
            &util::today(),
        )
    })
    .await
}

#[tauri::command]
pub fn ask_start<R: Runtime>(
    app: AppHandle<R>,
    question: String,
    scope: Option<SearchScope>,
    conversation: Option<String>,
) -> Result<String> {
    if question.trim().is_empty() {
        return Err(Error::Invalid("Ask a question first.".into()));
    }
    let id = util::random_hex(8);
    let cancel = CancelToken::new();
    lock(&state().asks).insert(id.clone(), cancel.clone());
    let req = id.clone();
    std::thread::Builder::new()
        .name(format!("ask-{id}"))
        .spawn(move || {
            let emit_app = app.clone();
            let mut emit = |event: AskEvent| {
                let _ = emit_app.emit(
                    ASK_EVENT,
                    AskEnvelope {
                        id: req.clone(),
                        event,
                    },
                );
            };
            run_ask(&app, &question, scope, conversation, &cancel, &mut emit);
            lock(&state().asks).remove(&req);
        })
        .map_err(|e| Error::Internal(format!("Could not start the question: {e}")))?;
    Ok(id)
}

fn run_ask<R: Runtime>(
    app: &AppHandle<R>,
    question: &str,
    scope: Option<SearchScope>,
    conversation: Option<String>,
    cancel: &CancelToken,
    emit: &mut dyn FnMut(AskEvent),
) {
    let fail = |emit: &mut dyn FnMut(AskEvent), e: Error| {
        emit(AskEvent::Error {
            error: e.kind().into(),
            message: e.to_string(),
        })
    };
    let ctx = match Ctx::load(app) {
        Ok(c) => c,
        Err(e) => return fail(emit, e),
    };
    // a refused server address is a settings problem: say so instead of searching
    let models = match Models::new(&ctx.settings.ollama) {
        Ok(m) => m,
        Err(e) => return fail(emit, e),
    };
    let index = ctx.index();
    let embedder = models.embedder();
    let chat = models.chat();
    let semantic = (!ctx.settings.ollama.embed_model.is_empty()).then(|| Semantic {
        embedder: &embedder,
        cache_dir: &ctx.cache,
    });
    let chat: Option<&dyn Chat> = (!ctx.settings.ollama.chat_model.is_empty()).then_some(&chat);
    let scope = scope.unwrap_or_default();
    let today = util::today();
    // errors are emitted by `ask` itself
    let _ = ask::ask(
        &index,
        &ask::AskInput {
            question,
            scope: &scope,
            conversation: conversation.as_deref(),
            today: &today,
        },
        &ask::AskVaults {
            all: &ctx.vaults,
            save_to: ctx.primary(),
        },
        semantic,
        chat,
        cancel,
        emit,
    );
}

/// True if a running question was found and told to stop.
#[tauri::command]
pub fn ask_cancel(id: String) -> bool {
    match lock(&state().asks).get(&id) {
        Some(c) => {
            c.cancel();
            true
        }
        None => false,
    }
}

#[tauri::command]
pub async fn conversations_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ConversationSummary>> {
    blocking(move || Ok(conversations::list(&Ctx::load(&app)?.vaults))).await
}

#[tauri::command]
pub async fn conversation_get<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Conversation> {
    blocking(move || conversations::get(&Ctx::load(&app)?.vaults, &id)).await
}

#[tauri::command]
pub async fn ollama_status<R: Runtime>(app: AppHandle<R>) -> Result<OllamaStatus> {
    blocking(move || Ok(ollama::status(&Ctx::load(&app)?.settings.ollama))).await
}

#[tauri::command]
pub fn ollama_recommend() -> Result<Recommendation> {
    let mem = ollama::memsize().ok_or_else(|| {
        Error::Unavailable("macOS did not say how much memory this Mac has.".into())
    })?;
    Ok(ollama::recommend(mem))
}
