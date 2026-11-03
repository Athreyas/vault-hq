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

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

// ---- W widgets --------------------------------------------------------------------------
