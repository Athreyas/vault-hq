//! Widget commands (SPEC 10, 12). Owned by stream W. Thin: validate → hq_core → map errors.
//!
//! Register with `.manage(WidgetsState::new(support_dir, sources))` and
//! `widgets_catalog, widgets_layout_get, widgets_layout_set, widget_data` in the handler
//! list. `jobs.rs` starts the 5 s sampler with
//! `hq_core::widgets::system::spawn(state.sampler(), system::TICK, stop)`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hq_core::settings::Store;
use hq_core::types::{WidgetCatalogEntry, WidgetData, WidgetKind, WidgetLayout};
use hq_core::widgets::{self, json::JsonCache, system, Context, NoSources, WidgetSources};
use hq_core::{Error, Result};
use serde_json::{Map, Value};
use tauri::State;

pub struct WidgetsState {
    inner: Arc<Inner>,
}

struct Inner {
    support_dir: PathBuf,
    /// Layout writes are read-modify-write of settings.json; one at a time.
    settings_lock: Mutex<()>,
    sampler: system::SharedSampler,
    json: JsonCache,
    sources: Arc<dyn WidgetSources>,
}

impl WidgetsState {
    /// `sources` gives the task and inbox widgets their data; pass `None` until the task
    /// engine and inbox are wired, and those widgets say they aren't connected.
    pub fn new(support_dir: PathBuf, sources: Option<Arc<dyn WidgetSources>>) -> Self {
        WidgetsState {
            inner: Arc::new(Inner {
                support_dir,
                settings_lock: Mutex::new(()),
                sampler: system::shared(),
                json: JsonCache::default(),
                sources: sources.unwrap_or_else(|| Arc::new(NoSources)),
            }),
        }
    }

    /// For the sampling job.
    pub fn sampler(&self) -> system::SharedSampler {
        self.inner.sampler.clone()
    }
}

/// Run blocking work (settings file, sysinfo, an HTTP fetch) off the main thread.
async fn blocking<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Internal(format!("widget task failed: {e}")))?
}

#[tauri::command]
pub fn widgets_catalog() -> Vec<WidgetCatalogEntry> {
    widgets::catalog()
}

#[tauri::command]
pub async fn widgets_layout_get(state: State<'_, WidgetsState>) -> Result<WidgetLayout> {
    let inner = state.inner.clone();
    blocking(move || widgets::load(&Store::new(&inner.support_dir))).await
}

#[tauri::command]
pub async fn widgets_layout_set(
    state: State<'_, WidgetsState>,
    layout: WidgetLayout,
) -> Result<WidgetLayout> {
    let inner = state.inner.clone();
    blocking(move || {
        let _guard = inner
            .settings_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        widgets::save(&Store::new(&inner.support_dir), layout)
    })
    .await
}

#[tauri::command]
pub async fn widget_data(
    state: State<'_, WidgetsState>,
    kind: WidgetKind,
    config: Map<String, Value>,
) -> Result<WidgetData> {
    let inner = state.inner.clone();
    blocking(move || {
        let ctx = Context {
            sampler: &inner.sampler,
            json: &inner.json,
            sources: inner.sources.as_ref(),
            now: widgets::now(),
        };
        widgets::data(kind, &config, &ctx)
    })
    .await
}
