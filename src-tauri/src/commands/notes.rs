//! Note commands (SPEC 10): list, get, source, save, create, move, archive, templates.

use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::types::{
    MoveResult, NoteFilter, NoteInput, NoteSummary, NoteView, SaveResult, Source, TemplateInfo,
};
use hq_core::Result;
use tauri::State;

use super::app::blocking;

#[tauri::command]
pub async fn notes_list(
    hq: State<'_, Arc<Hq>>,
    filter: Option<NoteFilter>,
) -> Result<Vec<NoteSummary>> {
    blocking(
        &hq,
        move |hq| Ok(hq.notes_list(&filter.unwrap_or_default())),
    )
    .await
}

/// `reference`: a note id, or a wiki-style reference resolved in the primary vault first.
#[tauri::command]
pub async fn note_get(hq: State<'_, Arc<Hq>>, reference: String) -> Result<NoteView> {
    blocking(&hq, move |hq| hq.note_get(&reference)).await
}

#[tauri::command]
pub async fn note_source(hq: State<'_, Arc<Hq>>, id: String) -> Result<Source> {
    blocking(&hq, move |hq| hq.note_source(&id)).await
}

/// `base` is the hash `note_source` gave; a conflict comes back in the result, not as an error.
#[tauri::command]
pub async fn note_save(
    hq: State<'_, Arc<Hq>>,
    id: String,
    text: String,
    base: String,
) -> Result<SaveResult> {
    blocking(&hq, move |hq| hq.note_save(&id, &text, &base)).await
}

#[tauri::command]
pub async fn note_create(hq: State<'_, Arc<Hq>>, input: NoteInput) -> Result<NoteView> {
    blocking(&hq, move |hq| hq.note_create(&input)).await
}

/// Call with `dryRun: true` first to show which notes' links will change, then without.
#[tauri::command]
pub async fn note_move(
    hq: State<'_, Arc<Hq>>,
    id: String,
    to: String,
    dry_run: Option<bool>,
) -> Result<MoveResult> {
    blocking(&hq, move |hq| {
        hq.note_move(&id, &to, dry_run.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn note_archive(hq: State<'_, Arc<Hq>>, id: String) -> Result<MoveResult> {
    blocking(&hq, move |hq| hq.note_archive(&id)).await
}

#[tauri::command]
pub async fn templates_list(hq: State<'_, Arc<Hq>>) -> Result<Vec<TemplateInfo>> {
    blocking(&hq, |hq| Ok(hq.templates_list())).await
}
