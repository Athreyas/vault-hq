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
    /// Tasks done per day over the last 7 days.
    pub done_per_day: f32,
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
