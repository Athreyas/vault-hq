//! Widget commands (SPEC 10, 12). Owned by stream W. Thin: validate → hq_core → map errors.
//!
//! The layout lives in `Settings.widgets` and is read and written through the shell's
//! `Shell` state, so settings.json has one writer. Data requests use `WidgetsState`
//! (the system sampler, the JSON status cache and the vault sources), managed in main.rs;
//! `jobs::start_widget_sampler` ticks the sampler every 5 s.

use std::sync::Arc;

use hq_core::types::{WidgetCatalogEntry, WidgetData, WidgetKind, WidgetLayout};
use hq_core::widgets::{self, json::JsonCache, system, Context, NoSources, WidgetSources};
use hq_core::{Error, Result};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::commands::tasks::{vaults_from, TasksState};
use crate::state::Shell;

/// The widgets' window on the vault: the task index stream B keeps, and the inbox
/// across every open vault from the core (`Hq`).
pub struct AppSources<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> AppSources<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        AppSources { app }
    }
}

impl<R: Runtime> WidgetSources for AppSources<R> {
    fn tasks(&self) -> Result<Vec<widgets::tasks::TaskInput>> {
        let state = self.app.state::<TasksState>();
        state.set_vaults(vaults_from(&self.app.state::<Shell>().settings()));
        Ok(state
            .all_tasks()?
            .iter()
            .map(widgets::tasks::from_task)
            .collect())
    }

    fn inbox(&self) -> Result<Vec<widgets::InboxInput>> {
        let hq = self.app.state::<Arc<hq_core::hq::Hq>>();
        Ok(hq
            .inbox()
            .into_iter()
            .map(|i| widgets::InboxInput {
                title: i.title,
                date: Some(i.created),
            })
            .collect())
    }
}

pub struct WidgetsState {
    inner: Arc<Inner>,
}

struct Inner {
    sampler: system::SharedSampler,
    json: JsonCache,
    sources: Arc<dyn WidgetSources>,
}

impl WidgetsState {
    /// `sources` gives the task and inbox widgets their data (`AppSources` in the app);
    /// with `None` those widgets say they aren't connected.
    pub fn new(sources: Option<Arc<dyn WidgetSources>>) -> Self {
        WidgetsState {
            inner: Arc::new(Inner {
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

#[tauri::command]
pub fn widgets_catalog() -> Vec<WidgetCatalogEntry> {
    widgets::catalog()
}

#[tauri::command]
pub fn widgets_layout_get(shell: State<'_, Shell>) -> WidgetLayout {
    widgets::from_settings(&shell.settings().widgets).layout
}

#[tauri::command]
pub fn widgets_layout_set(shell: State<'_, Shell>, layout: WidgetLayout) -> Result<WidgetLayout> {
    let mut saved = None;
    shell.update_settings(|s| {
        saved = Some(widgets::apply(s, layout)?);
        Ok(())
    })?;
    saved.ok_or_else(|| Error::Internal("the widget layout was not saved".into()))
}

/// Runs on a blocking thread: sampling and a JSON fetch (up to its timeout) must never
/// hold up the window.
#[tauri::command]
pub async fn widget_data(
    state: State<'_, WidgetsState>,
    kind: WidgetKind,
    config: Map<String, Value>,
) -> Result<WidgetData> {
    let inner = state.inner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = Context {
            sampler: &inner.sampler,
            json: &inner.json,
            sources: inner.sources.as_ref(),
            now: widgets::now(),
        };
        widgets::data(kind, &config, &ctx)
    })
    .await
    .map_err(|e| Error::Internal(format!("widget task failed: {e}")))?
}
