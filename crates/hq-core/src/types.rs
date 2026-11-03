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

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

// ---- W widgets --------------------------------------------------------------------------
