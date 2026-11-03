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

/// How an agent is connected to `vault-hq mcp`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum ConnectKind {
    /// HQ edits the agent's own config file, after a preview and a yes.
    File,
    /// The agent has its own command for it; HQ shows the command to run.
    Command,
    /// No local MCP: the agent works from its prompt text alone.
    None,
}

/// An agent the onboarding wizard can set up (SPEC 9, 11).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub blurb: String,
    /// Can run `vault-hq mcp` as a local (stdio) MCP server.
    pub mcp: bool,
    pub connect: ConnectKind,
    /// The config file HQ would edit, `~`-relative, when `connect` is `file`.
    pub config_path: Option<String>,
    /// The agent's app or config folder exists on this Mac.
    pub detected: bool,
    /// What the prompt text is: "skill", "rule", "agents-md", "instructions".
    pub prompt_kind: String,
    /// Where the user puts the prompt text, in words.
    pub prompt_target: String,
    /// The `author:` value this agent writes.
    pub author: String,
}

/// What connecting an agent would change, shown before anything is written.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiff {
    pub agent_id: String,
    pub kind: ConnectKind,
    /// The absolute path of the config file (file kind).
    pub path: Option<String>,
    pub exists: bool,
    /// False when the agent is already connected exactly like this.
    pub changed: bool,
    /// Unified diff of the file, empty when unchanged.
    pub diff: String,
    /// The command to run (command kind).
    pub command: Option<String>,
    /// SHA-256 of the file as previewed; pass it back to apply so a changed file is refused.
    pub base_hash: Option<String>,
    /// Anything the user should know (an older vault-hq entry is replaced, …).
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApplied {
    pub agent_id: String,
    pub path: String,
    /// Where the previous file was copied, when there was one.
    pub backup: Option<String>,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScaffoldReport {
    pub root: String,
    pub vault_id: String,
    pub name: String,
    /// Vault-relative paths written (folders end with `/`).
    pub created: Vec<String>,
    /// Paths that were already there and left alone.
    pub kept: Vec<String>,
}

/// A suggested place for a new vault.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LocationSuggestion {
    pub label: String,
    pub path: String,
    /// The folder is synced between Macs (iCloud Drive).
    pub synced: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct KindCount {
    pub kind: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BrokenLink {
    pub from: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Prop {
    pub key: String,
    pub value: String,
}

/// One change the import assistant proposes. Nothing happens unless it is ticked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlanAction {
    /// Add the standard folders, AGENTS.md and templates (never overwrites).
    #[serde(rename_all = "camelCase")]
    CreateFolders { folders: Vec<String> },
    /// Add missing frontmatter keys to a note, guarded by the hash it was analysed at.
    #[serde(rename_all = "camelCase")]
    AddFrontmatter {
        rel: String,
        base_hash: String,
        add: Vec<Prop>,
    },
    /// Move a file or folder; refused when the target exists (folders merge, file by file).
    #[serde(rename_all = "camelCase")]
    Move { from: String, to: String },
    /// Set `tags:` on an untagged note.
    #[serde(rename_all = "camelCase")]
    AddTags {
        rel: String,
        base_hash: String,
        tags: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: String,
    pub group: String,
    pub summary: String,
    pub ticked: bool,
    pub reversible: bool,
    pub action: PlanAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub root: String,
    pub items: Vec<PlanItem>,
}

/// What the import assistant found in an existing (Obsidian) vault.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportAnalysis {
    pub root: String,
    pub notes: u32,
    pub no_frontmatter: u32,
    pub incomplete_frontmatter: u32,
    pub loose_root: Vec<String>,
    pub unknown_folders: Vec<String>,
    /// At most a few hundred, so a very broken vault still answers quickly.
    pub broken_links: Vec<BrokenLink>,
    pub broken_link_count: u32,
    pub untagged: u32,
    pub attachments: u32,
    #[ts(type = "number")]
    pub attachment_bytes: u64,
    pub attachment_kinds: Vec<KindCount>,
    pub unreadable: Vec<String>,
    /// Folders HQ expects that are missing.
    pub missing_folders: Vec<String>,
    pub plan: ImportPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem {
    pub id: String,
    pub reason: String,
}

/// The result of `import_apply` (and `import_undo`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RestructureReport {
    pub applied: Vec<String>,
    pub skipped: Vec<SkippedItem>,
    pub moved: Vec<Moved>,
    pub edited: Vec<String>,
    pub created: Vec<String>,
    /// Vault-relative path of the undo journal (`.vault-hq/import-….json`).
    pub journal: Option<String>,
}

// ---- F bookmarks ------------------------------------------------------------------------

// ---- W widgets --------------------------------------------------------------------------
