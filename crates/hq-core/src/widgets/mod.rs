//! Widgets (SPEC 12). Owned by stream W.
//!
//! Home's widget row: a registry of kinds (title, default config, a schema the UI draws
//! as a form), the layout kept in `Settings.widgets`, and one data provider per kind.
//! Providers are plain functions over plain input so they can be tested without a vault,
//! a network or a real machine; the app only supplies the inputs (`WidgetSources`).

pub mod json;
pub mod system;
pub mod tasks;

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::types::{
    ClockData, ConfigField, FieldInput, InboxData, WidgetBody, WidgetCatalogEntry, WidgetData,
    WidgetInstance, WidgetKind, WidgetLayout, WidgetStatus,
};

pub const LAYOUT_VERSION: u32 = 1;
pub const MAX_WIDGETS: usize = 24;
const MAX_ID_LEN: usize = 40;

pub const ALL_KINDS: [WidgetKind; 8] = [
    WidgetKind::Clock,
    WidgetKind::System,
    WidgetKind::Focus,
    WidgetKind::TasksDue,
    WidgetKind::TasksBurn,
    WidgetKind::Inbox,
    WidgetKind::Memories,
    WidgetKind::JsonStatus,
];

/// What a fresh install shows: quiet, useful on day one, nothing that needs setup.
const DEFAULT_KINDS: [WidgetKind; 5] = [
    WidgetKind::Clock,
    WidgetKind::System,
    WidgetKind::Focus,
    WidgetKind::TasksDue,
    WidgetKind::TasksBurn,
];

// ---- registry ---------------------------------------------------------------------------

fn text(key: &str, label: &str, help: &str, placeholder: &str, max_len: u32) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        help: help.into(),
        input: FieldInput::Text {
            placeholder: placeholder.into(),
            max_len,
        },
    }
}

fn number(key: &str, label: &str, help: &str, range: (f64, f64, f64), unit: &str) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        help: help.into(),
        input: FieldInput::Number {
            min: range.0,
            max: range.1,
            step: range.2,
            unit: unit.into(),
        },
    }
}

fn toggle(key: &str, label: &str, help: &str) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        help: help.into(),
        input: FieldInput::Toggle,
    }
}

fn title_field() -> ConfigField {
    text(
        "title",
        "Title",
        "Blank keeps the widget's own name.",
        "",
        40,
    )
}

fn filter_fields() -> Vec<ConfigField> {
    vec![
        text(
            "project",
            "Project",
            "Only tasks in this project; blank = all.",
            "garden",
            60,
        ),
        text(
            "tag",
            "Tag",
            "Only tasks with this tag, without the #.",
            "outdoor",
            60,
        ),
    ]
}

fn obj(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// One kind's catalog entry. The default config always passes `clean_config` unchanged.
pub fn entry(kind: WidgetKind) -> WidgetCatalogEntry {
    let s = |v: &str| Value::String(v.into());
    let (title, description, refresh_secs, multiple, defaults, fields) = match kind {
        WidgetKind::Clock => (
            "Clock",
            "Date, time, ISO week and how far through the year; extra time zones if you want them.",
            60,
            true,
            obj(&[
                ("title", s("")),
                ("seconds", Value::Bool(false)),
                ("hour12", Value::Bool(false)),
                ("zones", Value::Array(vec![])),
            ]),
            vec![
                title_field(),
                toggle("seconds", "Seconds", "Tick every second."),
                toggle("hour12", "12-hour", "am/pm instead of 24-hour time."),
                ConfigField {
                    key: "zones".into(),
                    label: "Other zones".into(),
                    help: "IANA names such as Europe/Berlin.".into(),
                    input: FieldInput::List {
                        fields: vec![
                            text("label", "Label", "", "Berlin", 20),
                            text("zone", "Zone", "", "Europe/Berlin", 60),
                        ],
                        max_items: 4,
                    },
                },
            ],
        ),
        WidgetKind::System => (
            "This Mac",
            "CPU, memory pressure, disk, battery, uptime and the busiest processes.",
            5,
            false,
            obj(&[
                ("title", s("")),
                ("showTop", Value::Bool(true)),
                ("cpuWarn", Value::from(85)),
                ("diskWarnGb", Value::from(15)),
                ("batteryWarn", Value::from(20)),
            ]),
            vec![
                title_field(),
                toggle(
                    "showTop",
                    "Top processes",
                    "List the three busiest processes.",
                ),
                number(
                    "cpuWarn",
                    "CPU warn at",
                    "Tint when CPU stays above this.",
                    (10.0, 100.0, 5.0),
                    "%",
                ),
                number(
                    "diskWarnGb",
                    "Disk warn under",
                    "Tint when free space drops below this.",
                    (1.0, 500.0, 1.0),
                    "GB",
                ),
                number(
                    "batteryWarn",
                    "Battery warn at",
                    "Tint when on battery and at or below this.",
                    (5.0, 80.0, 5.0),
                    "%",
                ),
            ],
        ),
        WidgetKind::TasksBurn => (
            "Burn rate",
            "Open tasks over time and how many get done each day.",
            300,
            true,
            obj(&[
                ("title", s("")),
                ("days", Value::from(30)),
                ("project", s("")),
                ("tag", s("")),
            ]),
            [
                vec![
                    title_field(),
                    number(
                        "days",
                        "Days",
                        "How far back to draw.",
                        (7.0, 90.0, 1.0),
                        "days",
                    ),
                ],
                filter_fields(),
            ]
            .concat(),
        ),
        WidgetKind::TasksDue => (
            "Due soon",
            "Overdue and upcoming tasks, nearest first.",
            300,
            true,
            obj(&[
                ("title", s("")),
                ("daysAhead", Value::from(7)),
                ("limit", Value::from(5)),
                ("project", s("")),
                ("tag", s("")),
            ]),
            [
                vec![
                    title_field(),
                    number(
                        "daysAhead",
                        "Look ahead",
                        "Include tasks due within this many days.",
                        (0.0, 60.0, 1.0),
                        "days",
                    ),
                    number(
                        "limit",
                        "Show",
                        "At most this many.",
                        (1.0, 15.0, 1.0),
                        "tasks",
                    ),
                ],
                filter_fields(),
            ]
            .concat(),
        ),
        WidgetKind::Focus => (
            "Focus",
            "The few tasks with the highest urgency right now.",
            300,
            true,
            obj(&[
                ("title", s("")),
                ("limit", Value::from(3)),
                ("project", s("")),
                ("tag", s("")),
            ]),
            [
                vec![
                    title_field(),
                    number(
                        "limit",
                        "Show",
                        "At most this many.",
                        (1.0, 10.0, 1.0),
                        "tasks",
                    ),
                ],
                filter_fields(),
            ]
            .concat(),
        ),
        WidgetKind::Inbox => (
            "Inbox",
            "How much is waiting to be sorted, and for how long.",
            300,
            false,
            obj(&[("title", s("")), ("limit", Value::from(3))]),
            vec![
                title_field(),
                number(
                    "limit",
                    "Show",
                    "Titles to list.",
                    (0.0, 10.0, 1.0),
                    "items",
                ),
            ],
        ),
        WidgetKind::Memories => (
            "Memories",
            "A bookmark or note from this day in earlier years.",
            3600,
            false,
            obj(&[("title", s(""))]),
            vec![title_field()],
        ),
        WidgetKind::JsonStatus => (
            "JSON status",
            "Any URL that returns JSON: pick fields to show and when each one is ok.",
            300,
            true,
            obj(&[
                ("title", s("")),
                ("url", s("")),
                ("intervalSecs", Value::from(300)),
                ("timeoutSecs", Value::from(5)),
                ("fields", Value::Array(vec![])),
            ]),
            vec![
                title_field(),
                ConfigField {
                    key: "url".into(),
                    label: "URL".into(),
                    help: "http or https; fetched by this Mac only.".into(),
                    input: FieldInput::Url {
                        placeholder: "http://printer.local/status.json".into(),
                    },
                },
                number(
                    "intervalSecs",
                    "Every",
                    "How often to fetch (at least a minute).",
                    (json::MIN_INTERVAL_SECS as f64, 86_400.0, 60.0),
                    "s",
                ),
                number(
                    "timeoutSecs",
                    "Timeout",
                    "Give up after this long.",
                    (1.0, 30.0, 1.0),
                    "s",
                ),
                ConfigField {
                    key: "fields".into(),
                    label: "Fields".into(),
                    help: "Path is a JSON pointer (/a/0/b) or dots (a.0.b). Ok rule: = up, \
                           != down, < 500, >= 0.9, true or false; blank = no rule."
                        .into(),
                    input: FieldInput::List {
                        fields: vec![
                            text("label", "Label", "", "uptime", 30),
                            text("path", "Path", "", "/status", 200),
                            text("ok", "Ok when", "", "= up", 60),
                            text("unit", "Unit", "", "ms", 10),
                        ],
                        max_items: 8,
                    },
                },
            ],
        ),
    };
    WidgetCatalogEntry {
        kind,
        title: title.into(),
        description: description.into(),
        refresh_secs,
        multiple,
        default_config: defaults,
        fields,
    }
}

pub fn catalog() -> Vec<WidgetCatalogEntry> {
    ALL_KINDS.iter().map(|k| entry(*k)).collect()
}

/// A kind from settings, accepting the spellings an older or hand-edited file might use.
pub fn parse_kind(name: &str) -> Option<WidgetKind> {
    let k = name.trim().to_lowercase().replace(['-', ' '], "_");
    Some(match k.as_str() {
        "clock" | "time" => WidgetKind::Clock,
        "system" | "mac" | "machine" => WidgetKind::System,
        "tasks_burn" | "burn" | "burndown" => WidgetKind::TasksBurn,
        "tasks_due" | "due" => WidgetKind::TasksDue,
        "focus" | "tasks_focus" => WidgetKind::Focus,
        "inbox" => WidgetKind::Inbox,
        "memories" | "daymark" => WidgetKind::Memories,
        "json_status" | "json" | "status" => WidgetKind::JsonStatus,
        _ => return None,
    })
}

// ---- config validation ------------------------------------------------------------------

/// How strictly to treat bad config: settings on disk are repaired quietly, but a layout
/// the user just submitted is refused with a message they can act on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Repair,
    Strict,
}

/// `raw` checked field by field against `kind`'s schema: wrong types fall back to the
/// default, numbers are clamped, text is cut to length, unknown keys are dropped.
pub fn clean_config(
    kind: WidgetKind,
    raw: &Map<String, Value>,
    mode: Mode,
) -> Result<Map<String, Value>> {
    let e = entry(kind);
    clean_fields(&e.fields, &e.default_config, raw, mode)
}

fn clean_fields(
    fields: &[ConfigField],
    defaults: &Map<String, Value>,
    raw: &Map<String, Value>,
    mode: Mode,
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for f in fields {
        // list rows have no defaults of their own; an absent text there is empty
        let default = defaults
            .get(&f.key)
            .cloned()
            .unwrap_or_else(|| empty_value(&f.input));
        let cleaned = match (&f.input, raw.get(&f.key)) {
            (_, None | Some(Value::Null)) => default,
            (FieldInput::Text { max_len, .. }, Some(Value::String(s))) => {
                Value::String(s.trim().chars().take(*max_len as usize).collect())
            }
            (FieldInput::Url { .. }, Some(Value::String(s))) => match json::check_url(s) {
                Ok(u) => Value::String(u),
                Err(e) if mode == Mode::Strict => return Err(e),
                Err(_) => default,
            },
            (FieldInput::Number { min, max, step, .. }, Some(v)) => match v.as_f64() {
                Some(n) if n.is_finite() => {
                    let n = n.clamp(*min, *max);
                    if step.fract() == 0.0 {
                        Value::from(n.round() as i64)
                    } else {
                        Value::from(n)
                    }
                }
                _ => default,
            },
            (FieldInput::Toggle, Some(Value::Bool(b))) => Value::Bool(*b),
            (FieldInput::Select { options }, Some(Value::String(s))) => {
                if options.iter().any(|o| &o.value == s) {
                    Value::String(s.clone())
                } else {
                    default
                }
            }
            (
                FieldInput::List {
                    fields: sub,
                    max_items,
                },
                Some(Value::Array(items)),
            ) => {
                let none = Map::new();
                let mut rows = Vec::new();
                for item in items.iter().take(*max_items as usize) {
                    if let Value::Object(m) = item {
                        rows.push(Value::Object(clean_fields(sub, &none, m, mode)?));
                    }
                }
                Value::Array(rows)
            }
            _ => default,
        };
        out.insert(f.key.clone(), cleaned);
    }
    Ok(out)
}

fn empty_value(input: &FieldInput) -> Value {
    match input {
        FieldInput::Text { .. } | FieldInput::Url { .. } => Value::String(String::new()),
        FieldInput::Toggle => Value::Bool(false),
        FieldInput::Number { min, .. } => Value::from(*min),
        FieldInput::Select { options } => options
            .first()
            .map(|o| Value::String(o.value.clone()))
            .unwrap_or(Value::Null),
        FieldInput::List { .. } => Value::Array(vec![]),
    }
}

// ---- layout -----------------------------------------------------------------------------

/// The layout as settings hold it, plus entries this build can't show (an unknown kind
/// from a newer build); those are written back untouched so nothing is lost.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub layout: WidgetLayout,
    pub kept: Vec<Value>,
}

pub fn default_layout() -> WidgetLayout {
    let mut taken = BTreeSet::new();
    WidgetLayout {
        version: LAYOUT_VERSION,
        widgets: DEFAULT_KINDS
            .iter()
            .map(|k| WidgetInstance {
                id: fresh_id(&mut taken),
                kind: *k,
                config: entry(*k).default_config,
            })
            .collect(),
    }
}

fn fresh_id(taken: &mut BTreeSet<String>) -> String {
    loop {
        let id = format!("w{}", crate::util::random_hex(4));
        if taken.insert(id.clone()) {
            return id;
        }
    }
}

fn good_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn default_stored() -> Stored {
    Stored {
        layout: default_layout(),
        kept: vec![],
    }
}

/// Read `Settings.widgets` whatever shape it is in. Never fails: a missing or broken
/// value gives the default row, bad entries are dropped or repaired.
///
/// Shapes accepted: `null` (never set), `{version, layout: [...]}` (current),
/// `{widgets: [...]}` or a bare array (early drafts); entries may be a kind name
/// (`"clock"`) or `{id?, kind|type, config|settings?}`.
pub fn from_settings(v: &Value) -> Stored {
    let entries = match v {
        Value::Array(a) => a,
        Value::Object(o) => match o.get("layout").or_else(|| o.get("widgets")) {
            Some(Value::Array(a)) => a,
            _ => return default_stored(),
        },
        _ => return default_stored(),
    };

    // ids of entries kept for a newer build are reserved first, so nothing shown clashes
    let mut taken: BTreeSet<String> = entries
        .iter()
        .filter(|e| {
            e.get("kind")
                .or_else(|| e.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|k| parse_kind(k).is_none())
        })
        .filter_map(|e| e.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    let mut singles = BTreeSet::new();
    let mut widgets = Vec::new();
    let mut kept = Vec::new();
    for e in entries {
        let (kind_name, id, config) = match e {
            Value::String(s) => (s.as_str(), None, None),
            Value::Object(o) => {
                let Some(k) = o
                    .get("kind")
                    .or_else(|| o.get("type"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let config = o.get("config").or_else(|| o.get("settings"));
                (k, o.get("id").and_then(Value::as_str), config)
            }
            _ => continue,
        };
        let Some(kind) = parse_kind(kind_name) else {
            if e.is_object() && kept.len() < MAX_WIDGETS {
                kept.push(e.clone());
            }
            continue;
        };
        if widgets.len() >= MAX_WIDGETS || (!entry(kind).multiple && !singles.insert(kind)) {
            continue;
        }
        let id = match id {
            Some(id) if good_id(id) && taken.insert(id.to_string()) => id.to_string(),
            _ => fresh_id(&mut taken),
        };
        let raw = match config {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        // Repair mode never fails; the fallback only keeps this function total.
        let config =
            clean_config(kind, &raw, Mode::Repair).unwrap_or_else(|_| entry(kind).default_config);
        widgets.push(WidgetInstance { id, kind, config });
    }
    Stored {
        layout: WidgetLayout {
            version: LAYOUT_VERSION,
            widgets,
        },
        kept,
    }
}

/// What goes back into `Settings.widgets`.
pub fn to_settings(s: &Stored) -> Value {
    let mut layout: Vec<Value> = s
        .layout
        .widgets
        .iter()
        .filter_map(|w| serde_json::to_value(w).ok())
        .collect();
    layout.extend(s.kept.iter().cloned());
    serde_json::json!({ "version": LAYOUT_VERSION, "layout": layout })
}

/// Check a layout the UI sent. Missing or clashing ids get fresh ones (a new widget comes
/// in with `id: ""`); a second copy of a single-use kind, too many widgets or a bad URL are
/// refused with a message.
pub fn validate(layout: WidgetLayout, kept: &[Value]) -> Result<WidgetLayout> {
    if layout.widgets.len() > MAX_WIDGETS {
        return Err(Error::Invalid(format!(
            "Home holds up to {MAX_WIDGETS} widgets; remove one first."
        )));
    }
    let mut taken: BTreeSet<String> = kept
        .iter()
        .filter_map(|k| k.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    let mut singles = BTreeSet::new();
    let mut out = Vec::with_capacity(layout.widgets.len());
    for w in layout.widgets {
        let e = entry(w.kind);
        if !e.multiple && !singles.insert(w.kind) {
            return Err(Error::Invalid(format!(
                "Home shows one {} widget, and it's already there.",
                e.title
            )));
        }
        let id = if good_id(&w.id) && taken.insert(w.id.clone()) {
            w.id
        } else {
            fresh_id(&mut taken)
        };
        let config = clean_config(w.kind, &w.config, Mode::Strict)
            .map_err(|err| Error::Invalid(format!("{}: {err}", e.title)))?;
        out.push(WidgetInstance {
            id,
            kind: w.kind,
            config,
        });
    }
    Ok(WidgetLayout {
        version: LAYOUT_VERSION,
        widgets: out,
    })
}

/// The layout from settings.json (the default row on a first run).
pub fn load(store: &crate::settings::Store) -> Result<WidgetLayout> {
    Ok(from_settings(&store.load()?.widgets).layout)
}

/// Validate `layout` and write it to settings.json, keeping entries a newer build left and
/// every other setting as it was. Returns what was saved (with any new ids filled in).
/// Callers serialise their own settings writes.
pub fn save(store: &crate::settings::Store, layout: WidgetLayout) -> Result<WidgetLayout> {
    let mut settings = store.load()?;
    let kept = from_settings(&settings.widgets).kept;
    let layout = validate(layout, &kept)?;
    settings.widgets = to_settings(&Stored {
        layout: layout.clone(),
        kept,
    });
    store.save(&settings)?;
    Ok(layout)
}

// ---- data -------------------------------------------------------------------------------

/// Where widgets get vault data. The app implements this over the task engine (stream B)
/// and the inbox (stream A); until then every method says the source isn't connected.
pub trait WidgetSources: Send + Sync {
    fn tasks(&self) -> Result<Vec<tasks::TaskInput>> {
        Err(Error::Unavailable(
            "Tasks aren't connected to widgets yet.".into(),
        ))
    }
    fn inbox(&self) -> Result<Vec<InboxInput>> {
        Err(Error::Unavailable(
            "The inbox isn't connected to widgets yet.".into(),
        ))
    }
}

/// No sources at all: every vault widget shows "not connected".
pub struct NoSources;
impl WidgetSources for NoSources {}

/// One inbox item, as the inbox module (stream A) can hand it over.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InboxInput {
    pub title: String,
    /// `YYYY-MM-DD` the item arrived, if known.
    pub date: Option<String>,
}

/// Local wall-clock time, as providers take it (callers needn't depend on chrono).
pub type Now = chrono::DateTime<chrono::Local>;

pub fn now() -> Now {
    chrono::Local::now()
}

pub struct Context<'a> {
    pub sampler: &'a system::SharedSampler,
    pub json: &'a json::JsonCache,
    pub sources: &'a dyn WidgetSources,
    pub now: Now,
}

fn unavailable(kind: WidgetKind, at: i64, message: String) -> WidgetData {
    WidgetData {
        kind,
        status: WidgetStatus::Unknown,
        summary: message.clone(),
        at,
        body: WidgetBody::Unavailable { message },
    }
}

/// Fresh data for one widget. Sources that are missing or down give an `Unavailable`
/// body rather than an error, so one quiet widget never breaks the row.
pub fn data(kind: WidgetKind, config: &Map<String, Value>, ctx: &Context) -> Result<WidgetData> {
    let cfg = clean_config(kind, config, Mode::Repair)?;
    let at = ctx.now.timestamp_millis();
    let today = ctx.now.date_naive();
    let source_err = |e: Error| match e {
        Error::Unavailable(m) | Error::NotFound(m) => Ok(unavailable(kind, at, m)),
        other => Err(other),
    };
    match kind {
        WidgetKind::Clock => Ok(clock(&ctx.now)),
        WidgetKind::System => Ok(system::widget(ctx.sampler, &cfg)),
        WidgetKind::TasksBurn | WidgetKind::TasksDue | WidgetKind::Focus => {
            match ctx.sources.tasks() {
                Ok(list) => Ok(tasks::widget(kind, &list, &cfg, today, at)),
                Err(e) => source_err(e),
            }
        }
        WidgetKind::Inbox => match ctx.sources.inbox() {
            Ok(items) => Ok(inbox(&items, &cfg, today, at)),
            Err(e) => source_err(e),
        },
        WidgetKind::Memories => Ok(unavailable(
            kind,
            at,
            "Memories appear once bookmarks are set up.".into(),
        )),
        WidgetKind::JsonStatus => Ok(json::widget(ctx.json, &cfg, at, json::fetch)),
    }
}

pub fn clock(now: &chrono::DateTime<chrono::Local>) -> WidgetData {
    use chrono::{Datelike, Timelike};
    let date = now.date_naive();
    let days_in_year = if date.leap_year() { 366.0 } else { 365.0 };
    let day_part = now.num_seconds_from_midnight() as f64 / 86_400.0;
    let year_pct = ((date.ordinal0() as f64 + day_part) / days_in_year * 100.0) as f32;
    let body = ClockData {
        date: date.format("%Y-%m-%d").to_string(),
        time: now.format("%H:%M").to_string(),
        weekday: now.format("%A").to_string(),
        iso_week: date.iso_week().week(),
        day_of_year: date.ordinal(),
        year_pct: year_pct.clamp(0.0, 100.0),
    };
    WidgetData {
        kind: WidgetKind::Clock,
        status: WidgetStatus::Ok,
        summary: format!("week {} · day {}", body.iso_week, body.day_of_year),
        at: now.timestamp_millis(),
        body: WidgetBody::Clock(body),
    }
}

pub fn inbox(
    items: &[InboxInput],
    cfg: &Map<String, Value>,
    today: chrono::NaiveDate,
    at: i64,
) -> WidgetData {
    let limit = cfg.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
    let oldest_days = items
        .iter()
        .filter_map(|i| i.date.as_deref().and_then(tasks::parse_date))
        .map(|d| (today - d).num_days().max(0) as u32)
        .max();
    let count = items.len() as u32;
    // two weeks unsorted is the point where an inbox stops being an inbox
    let status = match oldest_days {
        Some(d) if count > 0 && d >= 14 => WidgetStatus::Warn,
        _ => WidgetStatus::Ok,
    };
    let summary = match (count, oldest_days) {
        (0, _) => "inbox zero".to_string(),
        (n, Some(d)) if d > 0 => format!("{n} waiting · oldest {d}d"),
        (n, _) => format!("{n} waiting"),
    };
    WidgetData {
        kind: WidgetKind::Inbox,
        status,
        summary,
        at,
        body: WidgetBody::Inbox(InboxData {
            count,
            oldest_days,
            titles: items.iter().take(limit).map(|i| i.title.clone()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kinds(l: &WidgetLayout) -> Vec<WidgetKind> {
        l.widgets.iter().map(|w| w.kind).collect()
    }

    #[test]
    fn defaults_pass_their_own_schema() {
        for e in catalog() {
            let cleaned = clean_config(e.kind, &e.default_config, Mode::Strict).unwrap();
            assert_eq!(cleaned, e.default_config, "{:?}", e.kind);
        }
    }

    #[test]
    fn never_set_gives_the_default_row() {
        let s = from_settings(&Value::Null);
        assert_eq!(kinds(&s.layout), DEFAULT_KINDS.to_vec());
        assert!(s.kept.is_empty());
        let ids: BTreeSet<_> = s.layout.widgets.iter().map(|w| w.id.clone()).collect();
        assert_eq!(ids.len(), DEFAULT_KINDS.len());
    }

    #[test]
    fn an_empty_row_stays_empty() {
        let s = from_settings(&json!({"version": 1, "layout": []}));
        assert!(s.layout.widgets.is_empty());
    }

    #[test]
    fn garbage_gives_the_default_row() {
        for v in [json!(42), json!("clock"), json!({"layout": "nope"})] {
            assert_eq!(kinds(&from_settings(&v).layout), DEFAULT_KINDS.to_vec());
        }
    }

    #[test]
    fn migrates_old_shapes_and_spellings() {
        let v = json!([
            "clock",
            {"type": "tasks-burn", "settings": {"days": 14}},
            {"kind": "JSON"},
            7,
            null
        ]);
        let s = from_settings(&v);
        assert_eq!(
            kinds(&s.layout),
            vec![
                WidgetKind::Clock,
                WidgetKind::TasksBurn,
                WidgetKind::JsonStatus
            ]
        );
        assert_eq!(s.layout.widgets[1].config["days"], json!(14));
        // missing keys are filled from the defaults
        assert_eq!(s.layout.widgets[2].config["intervalSecs"], json!(300));

        let s = from_settings(&json!({"widgets": [{"kind": "focus"}]}));
        assert_eq!(kinds(&s.layout), vec![WidgetKind::Focus]);
    }

    #[test]
    fn keeps_unknown_kinds_for_a_newer_build() {
        let future = json!({"id": "wfut", "kind": "weather", "config": {"city": "x"}});
        let v = json!({"version": 3, "layout": [{"id": "wfut", "kind": "clock"}, future.clone()]});
        let s = from_settings(&v);
        assert_eq!(kinds(&s.layout), vec![WidgetKind::Clock]);
        assert_ne!(s.layout.widgets[0].id, "wfut", "the kept entry owns its id");
        assert_eq!(s.kept, vec![future.clone()]);
        let back = to_settings(&s);
        assert_eq!(back["layout"][1], future);
        assert_eq!(from_settings(&back), s);
    }

    #[test]
    fn repairs_bad_ids_and_config() {
        let v = json!([
            {"id": "same", "kind": "focus", "config": {"limit": 99, "project": 5, "extra": true}},
            {"id": "same", "kind": "tasks_due", "config": {"limit": "three", "daysAhead": -4.6}},
            {"id": "has space", "kind": "clock",
             "config": {"zones": [{"label": "B", "zone": "Europe/Berlin", "x": 1}, "junk"]}},
            {"id": "s1", "kind": "system"},
            {"id": "s2", "kind": "system"},
            {"kind": "json_status", "config": {"url": "file:///etc/passwd", "intervalSecs": 5}}
        ]);
        let s = from_settings(&v);
        let w = &s.layout.widgets;
        assert_eq!(w.len(), 5, "second system widget dropped");
        assert_eq!(w[0].id, "same");
        assert_ne!(w[1].id, "same");
        assert!(good_id(&w[2].id) && w[2].id != "has space");
        assert_eq!(w[0].config["limit"], json!(10));
        assert_eq!(w[0].config["project"], json!(""));
        assert!(!w[0].config.contains_key("extra"));
        assert_eq!(w[1].config["limit"], json!(5));
        assert_eq!(w[1].config["daysAhead"], json!(0));
        assert_eq!(
            w[2].config["zones"],
            json!([{"label": "B", "zone": "Europe/Berlin"}])
        );
        assert_eq!(w[4].config["url"], json!(""));
        assert_eq!(w[4].config["intervalSecs"], json!(json::MIN_INTERVAL_SECS));
    }

    #[test]
    fn caps_the_row() {
        let v = Value::Array((0..40).map(|_| json!("clock")).collect());
        assert_eq!(from_settings(&v).layout.widgets.len(), MAX_WIDGETS);
    }

    fn inst(id: &str, kind: WidgetKind, config: Value) -> WidgetInstance {
        WidgetInstance {
            id: id.into(),
            kind,
            config: config.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn validate_assigns_ids_and_refuses_what_it_cannot_fix() {
        let kept = vec![json!({"id": "wfut", "kind": "weather"})];
        let l = WidgetLayout {
            version: 0,
            widgets: vec![
                inst("", WidgetKind::Clock, json!({})),
                inst("wfut", WidgetKind::Focus, json!({"limit": 2})),
            ],
        };
        let ok = validate(l, &kept).unwrap();
        assert_eq!(ok.version, LAYOUT_VERSION);
        assert!(good_id(&ok.widgets[0].id));
        assert_ne!(ok.widgets[1].id, "wfut", "ids of kept entries are taken");
        assert_eq!(ok.widgets[1].config["limit"], json!(2));

        let two = WidgetLayout {
            version: 1,
            widgets: vec![
                inst("a", WidgetKind::System, json!({})),
                inst("b", WidgetKind::System, json!({})),
            ],
        };
        assert!(matches!(validate(two, &[]), Err(Error::Invalid(_))));

        let bad_url = WidgetLayout {
            version: 1,
            widgets: vec![inst("a", WidgetKind::JsonStatus, json!({"url": "ftp://x"}))],
        };
        let err = validate(bad_url, &[]).unwrap_err().to_string();
        assert!(err.contains("http"), "{err}");

        let many = WidgetLayout {
            version: 1,
            widgets: (0..30)
                .map(|_| inst("", WidgetKind::Clock, json!({})))
                .collect(),
        };
        assert!(validate(many, &[]).is_err());
    }

    #[test]
    fn unconnected_sources_are_quiet() {
        let sampler = system::shared();
        let cache = json::JsonCache::default();
        let ctx = Context {
            sampler: &sampler,
            json: &cache,
            sources: &NoSources,
            now: chrono::Local::now(),
        };
        for k in [WidgetKind::Focus, WidgetKind::Inbox, WidgetKind::Memories] {
            let d = data(k, &Map::new(), &ctx).unwrap();
            assert_eq!(d.status, WidgetStatus::Unknown);
            assert!(matches!(d.body, WidgetBody::Unavailable { .. }));
        }
    }

    #[test]
    fn clock_shape() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 12, 0, 0)
            .unwrap();
        let d = clock(&now);
        let WidgetBody::Clock(c) = d.body else {
            panic!("clock body")
        };
        assert_eq!(c.date, "2026-01-01");
        assert_eq!(c.day_of_year, 1);
        assert_eq!(c.iso_week, 1);
        assert!(c.year_pct < 1.0);
    }

    #[test]
    fn inbox_counts_and_ages() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let items = vec![
            InboxInput {
                title: "a".into(),
                date: Some("2026-10-01".into()),
            },
            InboxInput {
                title: "b".into(),
                date: None,
            },
        ];
        let cfg = entry(WidgetKind::Inbox).default_config;
        let d = inbox(&items, &cfg, today, 0);
        assert_eq!(d.status, WidgetStatus::Warn);
        let WidgetBody::Inbox(i) = d.body else {
            panic!("inbox body")
        };
        assert_eq!((i.count, i.oldest_days), (2, Some(32)));
        assert_eq!(inbox(&[], &cfg, today, 0).summary, "inbox zero");
    }

    #[test]
    fn save_and_load_through_settings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"theme":"light","widgets":{"version":1,"layout":[{"id":"f","kind":"weather"}]}}"#,
        )
        .unwrap();
        let store = crate::settings::Store::new(dir.path());
        assert!(load(&store).unwrap().widgets.is_empty());
        let saved = save(
            &store,
            WidgetLayout {
                version: 1,
                widgets: vec![inst("", WidgetKind::Clock, json!({"seconds": true}))],
            },
        )
        .unwrap();
        assert_eq!(load(&store).unwrap(), saved);
        let s = store.load().unwrap();
        assert_eq!(s.theme, crate::settings::Theme::Light);
        assert_eq!(s.widgets["layout"][1]["kind"], json!("weather"));
    }
}
