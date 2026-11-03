//! Task widgets: burn rate, due soon, focus. They take a flat list of `TaskInput`, which
//! the app builds from the task engine (stream B), so they don't depend on its types.

use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::types::{
    BurnData, BurnDay, TaskListData, WidgetBody, WidgetData, WidgetKind, WidgetStatus, WidgetTask,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Open,
    Started,
    /// `[>]`: hidden from due/focus until its scheduled date (SPEC 4.1).
    Waiting,
    Done,
    Cancelled,
}

impl TaskState {
    fn is_open(self) -> bool {
        matches!(
            self,
            TaskState::Open | TaskState::Started | TaskState::Waiting
        )
    }
}

/// What a task widget needs to know about one task; dates are `YYYY-MM-DD`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskInput {
    pub id: String,
    pub title: String,
    pub state: TaskState,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// "highest" | "high" | "medium" | "low" | "lowest", as the engine names them.
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    /// `➕`, when known.
    #[serde(default)]
    pub created: Option<String>,
    /// `✅` or `❌`, when the task is closed and the date is known.
    #[serde(default)]
    pub ended: Option<String>,
    #[serde(default)]
    pub urgency: f32,
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// Project and tag filters from a widget's config; blank = no filter.
fn keep(t: &TaskInput, cfg: &Map<String, Value>) -> bool {
    let want = |k: &str| {
        cfg.get(k)
            .and_then(Value::as_str)
            .map(|s| s.trim().trim_start_matches('#').to_lowercase())
            .filter(|s| !s.is_empty())
    };
    let project_ok = want("project").is_none_or(|p| {
        t.project
            .as_deref()
            .is_some_and(|tp| tp.to_lowercase() == p)
    });
    let tag_ok = want("tag").is_none_or(|g| {
        t.tags.iter().any(|tg| {
            let tg = tg.trim_start_matches('#').to_lowercase();
            // a nested tag (#a/b) matches its parent filter (a)
            tg == g || tg.starts_with(&format!("{g}/"))
        })
    });
    project_ok && tag_ok
}

fn num(cfg: &Map<String, Value>, key: &str, fallback: u64) -> u64 {
    cfg.get(key).and_then(Value::as_u64).unwrap_or(fallback)
}

/// Open tasks per day over the last `days` days, with created and done counts. Derived
/// only from created/ended dates and today's state (SPEC 4.2 stats), so a task with no
/// creation date counts as always having existed.
pub fn burn(tasks: &[TaskInput], today: NaiveDate, days: u32) -> BurnData {
    let days = days.max(1);
    let spans: Vec<(Option<NaiveDate>, Option<NaiveDate>, TaskState)> = tasks
        .iter()
        .map(|t| {
            (
                t.created.as_deref().and_then(parse_date),
                t.ended.as_deref().and_then(parse_date),
                t.state,
            )
        })
        .collect();
    let mut out = Vec::with_capacity(days as usize);
    for back in (0..days).rev() {
        let d = today - chrono::Duration::days(back as i64);
        let mut day = BurnDay {
            date: d.format("%Y-%m-%d").to_string(),
            open: 0,
            created: 0,
            done: 0,
        };
        for (created, ended, state) in &spans {
            if *created == Some(d) {
                day.created += 1;
            }
            if *state == TaskState::Done && *ended == Some(d) {
                day.done += 1;
            }
            let existed = created.is_none_or(|c| c <= d);
            // a closed task without a date closed before anything we can draw
            let still_open = state.is_open() || ended.is_some_and(|e| e > d);
            if existed && still_open {
                day.open += 1;
            }
        }
        out.push(day);
    }
    let last7 = &out[out.len().saturating_sub(7)..];
    let done7: u32 = last7.iter().map(|d| d.done).sum();
    let created7: u32 = last7.iter().map(|d| d.created).sum();
    BurnData {
        open_now: tasks.iter().filter(|t| t.state.is_open()).count() as u32,
        done_per_day: done7 as f32 / last7.len().max(1) as f32,
        net_7d: created7 as i32 - done7 as i32,
        days: out,
    }
}

/// A growing list is worth a glance, not an alarm.
pub fn burn_status(b: &BurnData) -> WidgetStatus {
    if b.open_now == 0 && b.days.iter().all(|d| d.done == 0) {
        WidgetStatus::Unknown
    } else if b.net_7d > 0 {
        WidgetStatus::Warn
    } else {
        WidgetStatus::Ok
    }
}

fn view(t: &TaskInput, today: NaiveDate) -> WidgetTask {
    let due = t.due.as_deref().and_then(parse_date);
    WidgetTask {
        id: t.id.clone(),
        title: t.title.clone(),
        project: t.project.clone(),
        due: due.map(|d| d.format("%Y-%m-%d").to_string()),
        due_in: due.map(|d| (d - today).num_days() as i32),
        priority: t.priority.clone(),
        urgency: t.urgency,
        started: t.state == TaskState::Started,
    }
}

/// Visible open tasks: waiting ones stay hidden, like in the task list.
fn actionable<'a>(
    tasks: &'a [TaskInput],
    cfg: &'a Map<String, Value>,
) -> impl Iterator<Item = &'a TaskInput> {
    tasks
        .iter()
        .filter(|t| matches!(t.state, TaskState::Open | TaskState::Started))
        .filter(move |t| keep(t, cfg))
}

fn counts(list: &[WidgetTask]) -> (u32, u32) {
    let overdue = list
        .iter()
        .filter(|t| t.due_in.is_some_and(|d| d < 0))
        .count();
    let today = list.iter().filter(|t| t.due_in == Some(0)).count();
    (overdue as u32, today as u32)
}

/// Overdue and due within `daysAhead`, nearest first, cut to `limit`.
pub fn due(tasks: &[TaskInput], cfg: &Map<String, Value>, today: NaiveDate) -> TaskListData {
    let ahead = num(cfg, "daysAhead", 7) as i32;
    let limit = num(cfg, "limit", 5) as usize;
    let mut list: Vec<WidgetTask> = actionable(tasks, cfg)
        .map(|t| view(t, today))
        .filter(|t| t.due_in.is_some_and(|d| d <= ahead))
        .collect();
    list.sort_by(|a, b| {
        a.due_in
            .cmp(&b.due_in)
            .then(b.urgency.total_cmp(&a.urgency))
    });
    let (overdue, due_today) = counts(&list);
    let total = list.len() as u32;
    list.truncate(limit);
    TaskListData {
        tasks: list,
        overdue,
        due_today,
        total,
    }
}

/// The highest-urgency open tasks.
pub fn focus(tasks: &[TaskInput], cfg: &Map<String, Value>, today: NaiveDate) -> TaskListData {
    let limit = num(cfg, "limit", 3) as usize;
    let mut list: Vec<WidgetTask> = actionable(tasks, cfg).map(|t| view(t, today)).collect();
    list.sort_by(|a, b| {
        b.urgency
            .total_cmp(&a.urgency)
            .then(b.started.cmp(&a.started))
            .then(a.title.cmp(&b.title))
    });
    let (overdue, due_today) = counts(&list);
    let total = list.len() as u32;
    list.truncate(limit);
    TaskListData {
        tasks: list,
        overdue,
        due_today,
        total,
    }
}

pub fn due_status(d: &TaskListData) -> WidgetStatus {
    if d.overdue > 0 {
        WidgetStatus::Bad
    } else if d.due_today > 0 {
        WidgetStatus::Warn
    } else {
        WidgetStatus::Ok
    }
}

pub fn widget(
    kind: WidgetKind,
    tasks: &[TaskInput],
    cfg: &Map<String, Value>,
    today: NaiveDate,
    at: i64,
) -> WidgetData {
    let (status, summary, body) = match kind {
        WidgetKind::TasksBurn => {
            let filtered: Vec<TaskInput> = tasks.iter().filter(|t| keep(t, cfg)).cloned().collect();
            let b = burn(&filtered, today, num(cfg, "days", 30) as u32);
            let trend = match b.net_7d {
                n if n > 0 => format!("+{n} this week"),
                0 => "steady".to_string(),
                n => format!("{n} this week"),
            };
            let summary = format!(
                "{} open · {:.1} done/day · {trend}",
                b.open_now, b.done_per_day
            );
            (burn_status(&b), summary, WidgetBody::TasksBurn(b))
        }
        WidgetKind::TasksDue => {
            let d = due(tasks, cfg, today);
            let summary = match (d.overdue, d.due_today, d.total) {
                (0, 0, 0) => "nothing due".to_string(),
                (0, 0, n) => format!("{n} coming up"),
                (o, t, _) => [(o, "overdue"), (t, "today")]
                    .iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, w)| format!("{n} {w}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
            };
            (due_status(&d), summary, WidgetBody::TaskList(d))
        }
        _ => {
            let f = focus(tasks, cfg, today);
            let summary = match f.tasks.first() {
                Some(t) => format!("next: {}", t.title),
                None => "nothing open".to_string(),
            };
            let status = if f.total == 0 {
                WidgetStatus::Unknown
            } else {
                WidgetStatus::Ok
            };
            (status, summary, WidgetBody::TaskList(f))
        }
    };
    WidgetData {
        kind,
        status,
        summary,
        at,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    fn t(id: &str, state: TaskState) -> TaskInput {
        TaskInput {
            id: id.into(),
            title: id.into(),
            state,
            project: None,
            tags: vec![],
            priority: None,
            due: None,
            created: None,
            ended: None,
            urgency: 0.0,
        }
    }

    fn cfg(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn burn_counts_open_created_and_done_per_day() {
        let today = d("2026-11-10");
        let mut a = t("a", TaskState::Done);
        a.created = Some("2026-11-08".into());
        a.ended = Some("2026-11-09".into());
        let mut b = t("b", TaskState::Open);
        b.created = Some("2026-11-09".into());
        let c = t("c", TaskState::Open); // no created date: always there
        let e = t("e", TaskState::Done); // closed, date unknown: never drawn as open

        let burn = burn(&[a, b, c, e], today, 4);
        let open: Vec<u32> = burn.days.iter().map(|x| x.open).collect();
        let created: Vec<u32> = burn.days.iter().map(|x| x.created).collect();
        let done: Vec<u32> = burn.days.iter().map(|x| x.done).collect();
        assert_eq!(burn.days[0].date, "2026-11-07");
        assert_eq!(open, vec![1, 2, 2, 2]);
        assert_eq!(created, vec![0, 1, 1, 0]);
        assert_eq!(done, vec![0, 0, 1, 0]);
        assert_eq!(burn.open_now, 2);
        assert_eq!(burn.net_7d, 1);
        assert_eq!(burn_status(&burn), WidgetStatus::Warn);
    }

    #[test]
    fn burn_of_nothing_is_unknown() {
        let b = burn(&[], d("2026-11-10"), 30);
        assert_eq!(b.days.len(), 30);
        assert_eq!(burn_status(&b), WidgetStatus::Unknown);
    }

    #[test]
    fn due_sorts_nearest_first_and_hides_waiting() {
        let today = d("2026-11-10");
        let mut late = t("late", TaskState::Open);
        late.due = Some("2026-11-08".into());
        let mut now = t("now", TaskState::Started);
        now.due = Some("2026-11-10".into());
        let mut soon = t("soon", TaskState::Open);
        soon.due = Some("2026-11-12".into());
        let mut far = t("far", TaskState::Open);
        far.due = Some("2026-12-30".into());
        let mut wait = t("wait", TaskState::Waiting);
        wait.due = Some("2026-11-09".into());
        let list = [far, soon, now, late, wait];

        let r = due(&list, &cfg(json!({"daysAhead": 7, "limit": 2})), today);
        let ids: Vec<&str> = r.tasks.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["late", "now"]);
        assert_eq!((r.overdue, r.due_today, r.total), (1, 1, 3));
        assert_eq!(r.tasks[0].due_in, Some(-2));
        assert_eq!(due_status(&r), WidgetStatus::Bad);
    }

    #[test]
    fn due_status_thresholds() {
        let mk = |overdue, due_today| TaskListData {
            tasks: vec![],
            overdue,
            due_today,
            total: overdue + due_today,
        };
        assert_eq!(due_status(&mk(0, 0)), WidgetStatus::Ok);
        assert_eq!(due_status(&mk(0, 2)), WidgetStatus::Warn);
        assert_eq!(due_status(&mk(1, 2)), WidgetStatus::Bad);
    }

    #[test]
    fn focus_ranks_by_urgency_and_filters() {
        let today = d("2026-11-10");
        let mut a = t("a", TaskState::Open);
        a.urgency = 3.0;
        a.project = Some("Garden".into());
        let mut b = t("b", TaskState::Open);
        b.urgency = 9.0;
        b.tags = vec!["#outdoor/fence".into()];
        let mut c = t("c", TaskState::Done);
        c.urgency = 20.0;
        let list = [a, b, c];

        let all = focus(&list, &cfg(json!({"limit": 3})), today);
        let ids: Vec<&str> = all.tasks.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);

        let garden = focus(&list, &cfg(json!({"project": "garden"})), today);
        assert_eq!(garden.tasks.len(), 1);
        assert_eq!(garden.tasks[0].id, "a");

        let outdoor = focus(&list, &cfg(json!({"tag": "#outdoor"})), today);
        assert_eq!(outdoor.tasks[0].id, "b");
    }

    #[test]
    fn input_deserializes_from_plain_json() {
        let v = json!({"id": "x1", "title": "t", "state": "started", "urgency": 4.5});
        let t: TaskInput = serde_json::from_value(v).unwrap();
        assert_eq!(t.state, TaskState::Started);
        assert!(t.tags.is_empty());
    }
}
