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

/// A folder that looks like a vault, offered by `vault_candidates()`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultCandidate {
    pub path: String,
    pub name: String,
    /// `vault-hq` (has a marker) or `obsidian` (has `.obsidian/`).
    pub kind: String,
    pub added: bool,
}

/// `vault_create(path, name, layout)`: `default` makes the SPEC 3 folders, `empty` none.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum VaultLayout {
    #[default]
    Default,
    Empty,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: NoteId,
    pub vault: String,
    pub vault_name: String,
    pub rel: String,
    pub title: String,
    pub folder: String,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub note_type: Option<String>,
    pub updated: String,
    pub tags: Vec<String>,
}

/// `notes_list(filter)`; every field narrows the list, none means everything.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteFilter {
    #[ts(optional)]
    pub vault: Option<String>,
    /// A folder and everything under it.
    #[ts(optional)]
    pub folder: Option<String>,
    #[serde(rename = "type")]
    #[ts(optional, rename = "type")]
    pub note_type: Option<String>,
    /// Case-insensitive, in the title, path or body.
    #[ts(optional)]
    pub text: Option<String>,
    /// Notes updated on or after this `YYYY-MM-DD`.
    #[ts(optional)]
    pub updated_since: Option<String>,
    #[ts(optional)]
    pub limit: Option<u32>,
}

/// One outgoing link of a note and where it resolves (SPEC 3), if anywhere.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    /// The target as written, without `#heading` or `|alias`.
    pub target: String,
    pub embed: bool,
    pub resolved: Option<NoteId>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    #[serde(flatten)]
    #[ts(flatten)]
    pub note: NoteSummary,
    /// Frontmatter in file order, without `title`.
    pub props: Vec<(String, String)>,
    /// The body after the frontmatter, minus a leading `# <title>` the reader shows anyway.
    pub body: String,
    pub hash: String,
    pub links: Vec<LinkView>,
    pub backlinks: Vec<NoteSummary>,
}

/// `note_save`: what was written. A conflict is the `Conflict` error, carrying what is on disk.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub id: NoteId,
    pub text: String,
    pub hash: String,
}

/// `note_create(input)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteInput {
    pub title: String,
    /// Vault id; the primary vault when absent.
    #[ts(optional)]
    pub vault: Option<String>,
    /// Vault-relative folder; chosen from `type` when absent.
    #[ts(optional)]
    pub folder: Option<String>,
    /// A template's name (`_templates/<name>.md`) or its vault-relative path.
    #[ts(optional)]
    pub template: Option<String>,
    #[serde(rename = "type")]
    #[ts(optional, rename = "type")]
    pub note_type: Option<String>,
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
    /// Text after the heading (a blank note) or after the template's text.
    #[ts(optional)]
    pub body: Option<String>,
    /// `human` when absent; MCP passes the agent's name.
    #[ts(optional)]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    pub vault: String,
    pub vault_name: String,
    pub name: String,
    pub rel: String,
}

/// `note_move`: the new id and the notes whose links were (or, on a dry run, would be) updated.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    pub id: NoteId,
    pub relinked: Vec<NoteId>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum CaptureKind {
    #[default]
    Note,
    Task,
    Bookmark,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub kind: CaptureKind,
    /// The note the text landed in.
    pub id: NoteId,
    pub line: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Dawn,
    Day,
    Dusk,
    Night,
}

/// Where a card's button goes. `view` is one of `tasks`, `needs`, `inbox`, `day`, `note`;
/// `arg` is the date or note id when the view needs one.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CardAction {
    pub label: String,
    pub view: String,
    pub arg: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    /// focus, needs, inbox, done, tomorrow, project, findings.
    pub kind: String,
    pub kicker: String,
    pub title: String,
    pub lines: Vec<String>,
    pub progress: Option<u32>,
    pub hue: u32,
    /// The vault a card is about; absent for cards that cover every vault.
    pub vault: Option<String>,
    pub vault_name: Option<String>,
    pub action: Option<CardAction>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub date: String,
    pub phase: Phase,
    pub greeting: String,
    pub cards: Vec<Card>,
    /// Questions worth asking now, for the Ask bar.
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AgentLogLine {
    pub time: Option<String>,
    /// The writer, from `- HH:MM who · text`.
    pub who: Option<String>,
    pub text: String,
    /// `[[link]]` targets mentioned in the line.
    pub links: Vec<String>,
    pub vault_name: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DaySection {
    pub heading: String,
    pub lines: Vec<String>,
}

/// A checkbox task as the core views read it. Stream B's task model is the full one.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskLine {
    pub note: NoteId,
    pub text: String,
    /// `pending`, `started`, `waiting`, `done`, `cancelled`.
    pub status: String,
    /// `H`, `M` or `L`.
    pub priority: Option<String>,
    pub due: Option<String>,
    pub done: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    pub id: NoteId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub date: String,
    pub exists: bool,
    /// The primary vault's daily note (or the first vault that has one).
    pub note: Option<NoteId>,
    /// Underscored frontmatter keys of the daily notes (`tasks_pending: 12`).
    #[ts(type = "Record<string, number | string>")]
    pub metrics: std::collections::BTreeMap<String, serde_json::Value>,
    pub human: Vec<DaySection>,
    pub agent_log: Vec<AgentLogLine>,
    pub completed: Vec<TaskLine>,
    pub captures: Vec<NoteRef>,
    pub due_tasks: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum NeedKind {
    Overdue,
    DueSoon,
    Capture,
    Proposal,
    StaleProject,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub key: String,
    pub label: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub note: Option<NoteId>,
    pub excerpt: String,
    pub when: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Need {
    pub id: String,
    pub kind: NeedKind,
    pub label: String,
    pub title: String,
    pub detail: String,
    pub vault: String,
    pub vault_name: String,
    pub evidence: Evidence,
    pub outcomes: Vec<Outcome>,
    /// Lower is more urgent.
    pub rank: f64,
    /// The task's `🆔`, for task needs that have one.
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DecideResult {
    pub result: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum InboxKind {
    Capture,
    Session,
    Compact,
    Proposal,
    Dump,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: NoteId,
    pub vault_name: String,
    pub title: String,
    pub kind: InboxKind,
    pub created: String,
    pub age_days: u32,
    pub author: String,
}

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

// ---- W widgets --------------------------------------------------------------------------
