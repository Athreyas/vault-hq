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

/// A checkbox's state (SPEC 4.1): `[ ]` `[/]` `[x]` `[-]` `[>]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Started,
    Waiting,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub fn is_open(self) -> bool {
        matches!(
            self,
            TaskStatus::Pending | TaskStatus::Started | TaskStatus::Waiting
        )
    }
}

/// 🔺 ⏫ 🔼 🔽 ⏬. Ordered from most to least important.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub text: String,
    /// The `(YYYY-MM-DD)` at the end of the line, if there is one.
    pub date: Option<String>,
}

/// One task line, as the engine read it. Dates are `YYYY-MM-DD`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// What every task command takes: the `🆔` when the line has one, otherwise a
    /// content key (`<noteId>#<hash>`) that the first edit replaces with a new `🆔`.
    pub key: String,
    pub id: Option<String>,
    pub vault: String,
    pub note: NoteId,
    pub note_title: String,
    /// 1-based, for "open in editor" only; edits never use it.
    pub line: u32,
    pub status: TaskStatus,
    /// The text without tags or fields.
    pub description: String,
    pub tags: Vec<String>,
    pub project: Option<String>,
    pub priority: Option<Priority>,
    pub created: Option<String>,
    pub start: Option<String>,
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub done: Option<String>,
    pub cancelled: Option<String>,
    /// The rule after `🔁`, e.g. `every week`.
    pub recur: Option<String>,
    pub depends: Vec<String>,
    pub annotations: Vec<Annotation>,
    /// Depends on a task that is still open.
    pub blocked: bool,
    /// An open task depends on this one.
    pub blocking: bool,
    /// `[>]` with a `⏳` date still in the future.
    pub hidden: bool,
    pub urgency: f64,
    /// The line as it is in the note.
    pub raw: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum TaskSort {
    #[default]
    Urgency,
    Due,
    Priority,
    Created,
    Project,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum TaskGroupBy {
    #[default]
    None,
    Project,
    Tag,
    Due,
    Priority,
    Vault,
    Note,
    Status,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskQuery {
    /// Empty: open tasks (pending, started, and waiting ones whose date has come).
    pub status: Vec<TaskStatus>,
    /// Matches the project and its sub-projects (`home` matches `home/garden`).
    pub project: Option<String>,
    pub tags_all: Vec<String>,
    pub tags_any: Vec<String>,
    pub tags_none: Vec<String>,
    /// Empty: any priority. `noPriority` adds tasks without one.
    pub priority: Vec<Priority>,
    pub no_priority: bool,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
    pub scheduled_from: Option<String>,
    pub scheduled_to: Option<String>,
    /// Words that must all appear in the description, tags, project or annotations.
    pub text: Option<String>,
    pub vaults: Vec<String>,
    /// Also list waiting tasks that are still hidden.
    pub include_hidden: bool,
    pub blocked: Option<bool>,
    pub sort: TaskSort,
    pub group: TaskGroupBy,
    pub limit: Option<u32>,
    /// Defaults to today; set by tests and "what will be due" views.
    pub today: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskGroup {
    pub key: String,
    pub label: String,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskGroups {
    pub groups: Vec<TaskGroup>,
    /// Tasks that matched, before `limit`.
    pub total: u32,
    /// Every project and tag among open tasks, for completion in filters.
    pub projects: Vec<String>,
    pub tags: Vec<String>,
}

/// A new task. Only `description` is required.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskInput {
    pub description: String,
    pub tags: Vec<String>,
    /// A project note's name (its file stem); without such a note the task goes to
    /// `09-Planning/Tasks.md` tagged `#project/<name>`.
    pub project: Option<String>,
    /// Put it in this note instead (its `## Tasks` section).
    pub note: Option<NoteId>,
    pub vault: Option<String>,
    pub priority: Option<Priority>,
    pub start: Option<String>,
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub recur: Option<String>,
    pub depends: Vec<String>,
    /// `[>]`: hidden until `scheduled`.
    pub waiting: bool,
}

/// Fields to change. A missing field is left alone; `null` clears it.
#[derive(Debug, Clone, Default, Serialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    /// Only pending, started or waiting; use done/cancel for the others.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<TaskStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority: Option<Option<Priority>>,
    /// Replaces every tag (except `project/…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub add_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub remove_tags: Option<Vec<String>>,
    /// Sets the `#project/<name>` tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scheduled: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recur: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub depends: Option<Vec<String>>,
    /// Move the task (with its annotations) to this note's `## Tasks` section.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<NoteId>,
}

/// How a patch arrives: like `TaskPatch`, but telling an explicit `null` from a missing key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
struct TaskPatchWire {
    description: Option<String>,
    status: Option<TaskStatus>,
    #[serde(deserialize_with = "double_option")]
    priority: Option<Option<Priority>>,
    tags: Option<Vec<String>>,
    add_tags: Option<Vec<String>>,
    remove_tags: Option<Vec<String>>,
    #[serde(deserialize_with = "double_option")]
    project: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    start: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    scheduled: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    due: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    recur: Option<Option<String>>,
    depends: Option<Vec<String>>,
    note: Option<NoteId>,
}

impl<'de> Deserialize<'de> for TaskPatch {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let w = TaskPatchWire::deserialize(d)?;
        Ok(TaskPatch {
            description: w.description,
            status: w.status,
            priority: w.priority,
            tags: w.tags,
            add_tags: w.add_tags,
            remove_tags: w.remove_tags,
            project: w.project,
            start: w.start,
            scheduled: w.scheduled,
            due: w.due,
            recur: w.recur,
            depends: w.depends,
            note: w.note,
        })
    }
}

/// `Some(None)` for an explicit `null`, so a patch can clear a field.
fn double_option<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct StatsQuery {
    /// How many days back from `today` (default 30, at most 730).
    pub days: Option<u32>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub vaults: Vec<String>,
    pub today: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: String,
    pub created: u32,
    pub done: u32,
    pub cancelled: u32,
    /// Open at the end of that day.
    pub open: u32,
    /// Open at the end of that day with a due date before it.
    pub overdue: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StatBreakdown {
    pub key: String,
    pub open: u32,
    pub done: u32,
    pub cancelled: u32,
    pub overdue: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskStats {
    pub from: String,
    pub to: String,
    /// One per day, oldest first. `open` over these days is the burn-down for the scope.
    pub days: Vec<DayStat>,
    pub open: u32,
    pub overdue: u32,
    pub done_7: u32,
    pub done_30: u32,
    /// Of the tasks created in the last 7 / 30 days, the share now done (0–1).
    pub completion_7: f64,
    pub completion_30: f64,
    /// Done per day, averaged over the last 7 days.
    pub burn_rate: f64,
    /// Open tasks grow by this much per day, averaged over the last 7 days.
    pub net_per_day: f64,
    /// When the open count reaches zero at the current net rate, if it is falling.
    pub projected_zero: Option<String>,
    pub by_project: Vec<StatBreakdown>,
    pub by_tag: Vec<StatBreakdown>,
    pub by_priority: Vec<StatBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportTarget {
    pub note: String,
    pub tasks: u32,
    /// The note does not exist yet and the import creates it.
    pub create: bool,
}

/// What a Taskwarrior import did, or (dry run) would do.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskImportReport {
    pub dry_run: bool,
    /// Tasks in the export.
    pub total: u32,
    pub pending: u32,
    pub waiting: u32,
    pub completed: u32,
    /// Deleted tasks are not imported.
    pub deleted: u32,
    /// Recurrence templates: their pending instance carries the `🔁`.
    pub templates: u32,
    /// Already in the vault (same description and created date).
    pub duplicates: u32,
    /// Lines written (or that would be written).
    pub written: u32,
    pub targets: Vec<ImportTarget>,
    /// The first lines as they would appear in the notes.
    pub sample: Vec<String>,
    pub warnings: Vec<String>,
}

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
