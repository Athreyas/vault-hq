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

/// Which notes a search or a question reads (SPEC 6).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchScope {
    /// Vault ids; empty means every vault.
    pub vaults: Vec<String>,
    /// A window of this many days ending on `until` (0 or absent: all time).
    #[ts(optional)]
    pub days: Option<u32>,
    /// Last day of the window, `YYYY-MM-DD`; today when absent.
    #[ts(optional)]
    pub until: Option<String>,
    pub kinds: ScopeKinds,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKinds {
    #[default]
    Both,
    Notes,
    Bookmarks,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum HitKind {
    Note,
    Bookmark,
}

/// One matching section of a note.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: NoteId,
    pub vault: String,
    pub rel: String,
    pub stem: String,
    pub title: String,
    pub heading: String,
    pub excerpt: String,
    pub score: f64,
    pub kind: HitKind,
    /// The newest date the note carries (updated, created or its file name).
    #[ts(optional)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    /// False when the embedding model was unavailable and only keywords ranked.
    pub semantic: bool,
    /// Chunks in scope, for "searched N sections".
    pub searched: u32,
}

/// What `ask://event` carries: `{ id, kind, … }`. Order: sources, tokens, then done or error.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AskEnvelope {
    pub id: String,
    #[serde(flatten)]
    pub event: AskEvent,
}

#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AskEvent {
    Sources {
        sources: Vec<SearchHit>,
        semantic: bool,
    },
    Token {
        text: String,
    },
    /// `answer` is the final text with citations tidied; it replaces the streamed tokens.
    Done {
        answer: String,
        model: String,
        stopped: bool,
        #[ts(optional)]
        conversation: Option<NoteId>,
    },
    Error {
        /// One of the `HqError` kinds.
        error: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: NoteId,
    pub title: String,
    pub created: String,
    pub updated: String,
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: NoteId,
    pub title: String,
    pub created: String,
    pub updated: String,
    pub turns: Vec<Turn>,
}

/// One question and its answer, as saved in the conversation note.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// `YYYY-MM-DD HH:MM`.
    pub at: String,
    pub question: String,
    pub answer: String,
    pub model: String,
    /// Note stems cited as sources.
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub url: String,
    /// The server answered `/api/tags`.
    pub up: bool,
    /// Where the `ollama` program or app was found, if anywhere.
    #[ts(optional)]
    pub binary: Option<String>,
    pub models: Vec<String>,
    pub chat_model: String,
    pub embed_model: String,
    pub has_chat: bool,
    pub has_embed: bool,
    /// Why it is not usable, in words the user can act on.
    #[ts(optional)]
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// Unified memory, in GB, rounded.
    pub memory_gb: u32,
    pub embed: String,
    /// Chat models that fit, best first.
    pub chat: Vec<String>,
    /// `ollama pull …` lines for the user to copy; HQ never runs them.
    pub commands: Vec<String>,
}

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

/// The file format behind a bookmark source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum BookmarkFormat {
    Chromium,
    Firefox,
    Safari,
    Netscape,
}

/// One browser profile (or an exported HTML file) HQ can read bookmarks from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSource {
    /// `<browser>:<profile folder>` (`chrome:Default`), `safari`, or `html:<path>`.
    /// The key in `Settings.browsers`.
    pub id: String,
    /// `chrome`, `brave`, `edge`, `arc`, `dia`, `vivaldi`, `chromium`, `firefox`, `safari`, `html`.
    pub browser: String,
    /// What the user sees: "Chrome · Default".
    pub label: String,
    pub profile: String,
    pub format: BookmarkFormat,
    pub path: String,
    pub allowed: bool,
    /// Bookmarks in the source; only known once the user allowed it.
    pub count: Option<u32>,
    /// macOS refused the read (Safari without Full Disk Access).
    pub needs_access: bool,
    /// What to do about a source HQ can't read, in steps the user can follow.
    pub guidance: Option<String>,
}

/// Where a link points, for its badge (Daymark idea).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Youtube,
    Github,
    X,
    Reddit,
    Hackernews,
    Instagram,
    Tiktok,
    Pdf,
    Article,
}

/// A bookmark note, as the UI lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: NoteId,
    pub title: String,
    pub url: String,
    pub source: String,
    pub folder: String,
    pub platform: Platform,
    /// `YYYY-MM-DD` the browser says it was saved (or the day it was pasted).
    pub added: String,
    pub tags: Vec<String>,
    /// The paragraph above `## Notes`: the page description or the enriched summary.
    pub summary: Option<String>,
    /// The user wrote something under `## Notes`.
    pub has_notes: bool,
    /// `removed-in-browser` once every browser that had it dropped it.
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct BookmarkFilter {
    pub source: Option<String>,
    pub tag: Option<String>,
    pub platform: Option<Platform>,
    /// Leave out bookmarks no browser has any more.
    pub hide_removed: bool,
    /// At most this many, newest first; 0 means 500.
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Facet {
    pub value: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkPage {
    pub items: Vec<Bookmark>,
    /// Matches before `limit`.
    pub total: u32,
    /// Counts over every bookmark, so the filters show what exists.
    pub sources: Vec<Facet>,
    pub tags: Vec<Facet>,
    pub platforms: Vec<Facet>,
}

/// What an import found and did (or, on a dry run, would do).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkImportReport {
    pub source_id: String,
    pub dry_run: bool,
    /// http(s) bookmarks in the source.
    pub found: u32,
    /// New notes.
    pub added: u32,
    /// Existing notes this source now also lists, or that came back after a removal.
    pub updated: u32,
    pub unchanged: u32,
    /// Notes newly marked `removed-in-browser`.
    pub removed: u32,
    /// The same URL more than once in this source.
    pub duplicates: u32,
    /// A few of the new ones, so a dry run shows what would land where.
    pub sample: Vec<BookmarkPreview>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkPreview {
    pub title: String,
    pub url: String,
    pub folder: String,
    pub rel: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EnrichReport {
    pub enriched: u32,
    pub skipped: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

/// A bookmark brought back on Home (Daymark's daily memory).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: NoteId,
    pub title: String,
    pub url: String,
    pub platform: Platform,
    pub source: String,
    pub added: String,
    pub age_days: u32,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub has_notes: bool,
}

// ---- W widgets --------------------------------------------------------------------------
