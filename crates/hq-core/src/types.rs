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
    /// `HH:MM` local time on the due date (`⏰`).
    #[serde(default)]
    pub due_time: Option<String>,
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
    /// `HH:MM` on the due date; needs `due`.
    pub due_time: Option<String>,
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
    /// `HH:MM`; `null` clears it. Clearing `due` clears the time too.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due_time: Option<Option<String>>,
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
    due_time: Option<Option<String>>,
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
            due_time: w.due_time,
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
    /// Done in the last 7 / 30 days over (that done + open now): the share of the
    /// period's work that got done (0–1).
    pub completion_7: f64,
    pub completion_30: f64,
    /// Done per day, averaged over the window (`from`..`to`).
    pub burn_rate: f64,
    /// Open tasks grow by this much per day over the window (negative: shrinking).
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

/// A task read out of a sentence ("remind me to call the bank Friday 9am"), shown to the
/// user as an editable card. Nothing is written until they confirm (`action_add`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct ActionDraft {
    /// False: the text is not an action item (a question, a thought); nothing else is set.
    pub is_action: bool,
    /// What the user wrote.
    pub text: String,
    pub description: String,
    /// `YYYY-MM-DD`.
    pub due: Option<String>,
    /// `HH:MM`, local time.
    pub due_time: Option<String>,
    /// The user asked to be reminded.
    pub reminder: bool,
    pub priority: Option<Priority>,
    /// Tags the user wrote (`#tag`) or the model found.
    pub tags: Vec<String>,
    /// Tags of the matched notes, offered but not ticked.
    pub suggested_tags: Vec<String>,
    /// A project name with no note of its own (written as `#project/<name>`).
    pub project: Option<String>,
    /// The note the task is filed in: a matched project note, else the primary vault's
    /// `09-Planning/Tasks.md` (created on first use).
    pub note: Option<NoteId>,
    pub note_title: String,
    pub vault: Option<String>,
    /// Notes and bookmarks about the same thing, linked from an annotation.
    pub related: Vec<RelatedLink>,
    /// `model` when the local model read it, `rules` when only HQ's own date parser did.
    pub source: String,
    /// Where the model and the rules disagreed, or what was assumed.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RelatedLink {
    pub id: NoteId,
    pub stem: String,
    pub title: String,
    pub kind: HitKind,
    pub score: f64,
    /// Ticked by default (the UI may untick it).
    pub include: bool,
}

/// What `action_add` wrote.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ActionAdded {
    pub task: Task,
    /// The `related:` annotation, if one was written.
    pub related: Option<String>,
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

/// Every widget Home can show. Unknown kinds in settings are kept but never shown
/// (see `widgets::layout`), so a newer build's widgets survive an older build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    Clock,
    System,
    TasksBurn,
    TasksDue,
    Focus,
    Inbox,
    Memories,
    JsonStatus,
}

/// One placed widget: which kind, and its own settings (validated against the schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WidgetInstance {
    pub id: String,
    pub kind: WidgetKind,
    #[ts(type = "Record<string, unknown>")]
    pub config: serde_json::Map<String, serde_json::Value>,
}

/// The Home widget row, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WidgetLayout {
    pub version: u32,
    pub widgets: Vec<WidgetInstance>,
}

/// What the "add widget" picker lists, and how its settings form is drawn.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WidgetCatalogEntry {
    pub kind: WidgetKind,
    pub title: String,
    pub description: String,
    /// How often the UI should ask for fresh data; 0 = only on open and after a change.
    pub refresh_secs: u32,
    /// More than one of this kind on Home makes sense (two JSON endpoints, say).
    pub multiple: bool,
    #[ts(type = "Record<string, unknown>")]
    pub default_config: serde_json::Map<String, serde_json::Value>,
    pub fields: Vec<ConfigField>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub help: String,
    pub input: FieldInput,
}

/// A settings field, drawn by the UI as a form control.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FieldInput {
    Text {
        placeholder: String,
        max_len: u32,
    },
    Url {
        placeholder: String,
    },
    Number {
        min: f64,
        max: f64,
        step: f64,
        unit: String,
    },
    Toggle,
    Select {
        options: Vec<SelectOption>,
    },
    /// A list of small records, each drawn with `fields` (JSON status's field map).
    List {
        fields: Vec<ConfigField>,
        max_items: u32,
    },
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// A widget's overall state, for a quiet tint rather than an alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum WidgetStatus {
    Ok,
    Unknown,
    Warn,
    Bad,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WidgetData {
    pub kind: WidgetKind,
    pub status: WidgetStatus,
    /// One line a glance can take in ("cpu 12% · mem 61%").
    pub summary: String,
    /// Milliseconds since the epoch when this was measured or fetched.
    #[ts(type = "number")]
    pub at: i64,
    pub body: WidgetBody,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WidgetBody {
    Clock(ClockData),
    System(SystemData),
    TasksBurn(BurnData),
    TaskList(TaskListData),
    Inbox(InboxData),
    JsonStatus(JsonStatusData),
    /// The widget's source isn't connected yet (or is down); `message` says what to do.
    Unavailable {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ClockData {
    pub date: String,
    pub time: String,
    pub weekday: String,
    pub iso_week: u32,
    pub day_of_year: u32,
    /// How far through the year, 0–100.
    pub year_pct: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    #[ts(type = "number")]
    pub at: i64,
    pub cpu_pct: f32,
    pub cpu_count: u32,
    #[ts(type = "number")]
    pub mem_used: u64,
    #[ts(type = "number")]
    pub mem_total: u64,
    /// macOS memory pressure: "normal", "warn" or "critical"; None where unknown.
    pub mem_pressure: Option<String>,
    #[ts(type = "number")]
    pub swap_used: u64,
    #[ts(type = "number | null")]
    pub disk_free: Option<u64>,
    #[ts(type = "number | null")]
    pub disk_total: Option<u64>,
    pub battery: Option<Battery>,
    #[ts(type = "number")]
    pub uptime_secs: u64,
    pub top: Vec<ProcessUse>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub pct: u8,
    pub charging: bool,
    /// On mains power (charging, or full and plugged in).
    pub plugged: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProcessUse {
    pub name: String,
    /// Share of one core, as Activity Monitor shows it (can pass 100).
    pub cpu_pct: f32,
    #[ts(type = "number")]
    pub mem: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SystemData {
    pub now: SystemSample,
    /// Recent CPU % samples, oldest first (one per sampler tick).
    pub cpu_history: Vec<f32>,
    /// Recent memory-used % samples, oldest first.
    pub mem_history: Vec<f32>,
    /// What tipped the status, if anything ("disk 4 GB free").
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BurnDay {
    pub date: String,
    pub open: u32,
    pub created: u32,
    pub done: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BurnData {
    pub days: Vec<BurnDay>,
    pub open_now: u32,
    /// Tasks done per day over the whole window (`days`), so it matches the bars.
    pub done_per_day: f32,
    /// Tasks done in the last 7 days.
    pub done_7d: u32,
    /// Created minus done over the last 7 days: above zero, the list is growing.
    pub net_7d: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WidgetTask {
    pub id: String,
    pub title: String,
    pub project: Option<String>,
    pub due: Option<String>,
    /// Days from today to due; negative = overdue.
    pub due_in: Option<i32>,
    pub priority: Option<String>,
    pub urgency: f32,
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TaskListData {
    pub tasks: Vec<WidgetTask>,
    pub overdue: u32,
    pub due_today: u32,
    /// How many matched before the list was cut to the widget's limit.
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InboxData {
    pub count: u32,
    pub oldest_days: Option<u32>,
    pub titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct JsonField {
    pub label: String,
    /// The value as text; None when the pointer found nothing.
    pub value: Option<String>,
    pub unit: String,
    /// The field's own rule, if it has one: true = passes, false = fails.
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct JsonStatusData {
    pub url: String,
    pub http_status: Option<u16>,
    pub fields: Vec<JsonField>,
    /// Why the last fetch failed; the fields are then from the last good fetch, if any.
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub fetched_at: Option<i64>,
    /// Earliest the server will fetch again (ms since epoch); earlier asks get this copy.
    #[ts(type = "number")]
    pub next_at: i64,
}
