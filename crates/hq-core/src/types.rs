//! Types that cross to the UI. Each derives `TS` and is exported to web/src/bindings
//! (`cargo test -p hq-core export_bindings`). Streams add types only in their own section.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---- shared (Phase 0) -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub primary: bool,
    pub notes: u32,
    pub unreadable: u32,
}

/// `"<vaultId>:<vault-relative path>"`: how the UI and MCP name a note.
pub type NoteId = String;

pub fn note_id(vault: &str, rel: &str) -> NoteId {
    format!("{vault}:{rel}")
}

pub fn split_note_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':')
        .filter(|(v, r)| !v.is_empty() && !r.is_empty())
}

/// Text plus the hash an editor sends back with its save.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: NoteId,
    pub text: String,
    pub hash: String,
}

// ---- A core -----------------------------------------------------------------------------

// ---- B tasks ----------------------------------------------------------------------------

// ---- C search ---------------------------------------------------------------------------

// ---- D shell ----------------------------------------------------------------------------

/// `app_status()`: what the window shows before any view loads.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: String,
    pub vaults: Vec<VaultInfo>,
    pub primary_vault: Option<String>,
    /// Vaults in settings that could not be opened, with the reason (blocked, folder gone).
    pub problems: Vec<String>,
    /// The command an agent runs to reach this vault over MCP (`<binary> mcp`).
    pub mcp_command: String,
}

/// The `vault://changed` event: notes added, changed or removed in one vault.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultChanged {
    pub vault: String,
    pub rels: Vec<String>,
}

/// The `jobs://progress` event from a background job.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub job: String,
    pub done: u32,
    pub total: u32,
    pub message: String,
}

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

// ---- W widgets --------------------------------------------------------------------------
