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

/// `app_status()`: what the top bar and the palette's suggestions need.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    pub vaults: Vec<VaultInfo>,
    pub primary: Option<String>,
    /// Vaults in settings that could not be opened, with the reason (missing, blocked).
    pub problems: Vec<String>,
    /// `"<vault name>: <rel>"` for notes a scan could not read (iCloud can refuse reads).
    pub unreadable: Vec<String>,
    pub needs: u32,
    pub tips: Tips,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Tips {
    pub needs: Vec<String>,
    pub tasks_open: u32,
    pub tasks_overdue: u32,
    pub inbox: u32,
    /// Agent log lines plus tasks done today.
    pub today: u32,
    pub notes: u32,
    pub suggestions: Vec<String>,
}

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
    pub note: NoteSummary,
    /// Frontmatter in file order, without `title`.
    pub props: Vec<(String, String)>,
    /// The body after the frontmatter, minus a leading `# <title>` the reader shows anyway.
    pub body: String,
    pub hash: String,
    pub links: Vec<LinkView>,
    pub backlinks: Vec<NoteSummary>,
}

/// `note_save`: on a conflict nothing was written and `text`/`hash` are what is on disk.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub conflict: bool,
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
    pub tags: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AgentLogLine {
    pub time: Option<String>,
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

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

// ---- W widgets --------------------------------------------------------------------------
