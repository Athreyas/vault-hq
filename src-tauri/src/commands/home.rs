//! Capture, Home, Days, Needs and Inbox commands (SPEC 10).

use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::types::{
    CaptureKind, CaptureResult, Day, DecideResult, Home, InboxItem, MoveResult, Need,
};
use hq_core::Result;
use tauri::State;

use super::app::blocking;

/// `kind`: `note` (default), `task`, `bookmark`; a lone URL is a bookmark when absent.
#[tauri::command]
pub async fn capture(
    hq: State<'_, Arc<Hq>>,
    text: String,
    kind: Option<CaptureKind>,
    vault: Option<String>,
) -> Result<CaptureResult> {
    blocking(&hq, move |hq| hq.capture(&text, kind, vault.as_deref())).await
}

#[tauri::command]
pub async fn home(hq: State<'_, Arc<Hq>>, date: Option<String>) -> Result<Home> {
    blocking(&hq, move |hq| hq.home(date.as_deref())).await
}

#[tauri::command]
pub async fn days(hq: State<'_, Arc<Hq>>, from: String, span: u32) -> Result<Vec<Day>> {
    blocking(&hq, move |hq| hq.days(&from, span)).await
}

#[tauri::command]
pub async fn needs(hq: State<'_, Arc<Hq>>) -> Result<Vec<Need>> {
    blocking(&hq, |hq| Ok(hq.needs())).await
}

#[tauri::command]
pub async fn needs_decide(
    hq: State<'_, Arc<Hq>>,
    id: String,
    outcome: String,
    why: Option<String>,
) -> Result<DecideResult> {
    blocking(&hq, move |hq| {
        hq.needs_decide(&id, &outcome, why.as_deref().unwrap_or(""))
    })
    .await
}

#[tauri::command]
pub async fn inbox(hq: State<'_, Arc<Hq>>) -> Result<Vec<InboxItem>> {
    blocking(&hq, |hq| Ok(hq.inbox())).await
}

#[tauri::command]
pub async fn inbox_archive(hq: State<'_, Arc<Hq>>, id: String) -> Result<MoveResult> {
    blocking(&hq, move |hq| hq.inbox_archive(&id)).await
}
