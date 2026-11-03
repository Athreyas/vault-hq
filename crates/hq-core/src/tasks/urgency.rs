//! Urgency with Taskwarrior's default coefficients (SPEC 4.2), so the order feels familiar.

use chrono::{NaiveDate, NaiveTime};

use super::date;
use crate::types::{Priority, Task, TaskStatus};

pub const DUE: f64 = 12.0;
pub const PRIORITY_H: f64 = 6.0;
pub const PRIORITY_M: f64 = 3.9;
pub const PRIORITY_L: f64 = 1.8;
pub const STARTED: f64 = 4.0;
pub const BLOCKING: f64 = 8.0;
pub const BLOCKED: f64 = -5.0;
pub const SCHEDULED: f64 = 5.0;
pub const AGE: f64 = 2.0;
pub const AGE_MAX_DAYS: f64 = 365.0;
pub const TAG: f64 = 1.0;
pub const PROJECT: f64 = 1.0;
pub const WAITING: f64 = -3.0;
pub const ANNOTATIONS: f64 = 1.0;

/// Taskwarrior's due curve: full when overdue, 0.2 two weeks or more out, linear between.
pub fn due_factor(due: NaiveDate, today: NaiveDate) -> f64 {
    due_curve((today - due).num_days() as f64)
}

/// The due curve for a task with a `⏰` time: `now` is the time of day it is on `today`
/// (`None` when `today` is not the real today, as in tests and "what will be due" views,
/// which count from the start of the day). Once the time has passed it is overdue.
pub fn due_factor_timed(
    due: NaiveDate,
    time: NaiveTime,
    today: NaiveDate,
    now: Option<NaiveTime>,
) -> f64 {
    let day = (today - due).num_days() as f64;
    let now = now.unwrap_or(NaiveTime::MIN);
    let secs = (now - time).num_seconds() as f64;
    due_curve(day + secs / 86_400.0)
}

/// The time of day now, when `today` is the local date; otherwise None.
pub fn now_on(today: NaiveDate) -> Option<NaiveTime> {
    let now = chrono::Local::now().naive_local();
    (now.date() == today).then(|| now.time())
}

fn due_curve(overdue: f64) -> f64 {
    if overdue > 0.0 {
        1.0
    } else if overdue >= -14.0 {
        (overdue + 14.0) * 0.8 / 21.0 + 0.2
    } else {
        0.2
    }
}

/// Urgency of one task; 0 for closed ones. `blocked`, `blocking` and `hidden` must be set.
pub fn urgency(t: &Task, today: NaiveDate) -> f64 {
    if !t.status.is_open() {
        return 0.0;
    }
    let mut u = 0.0;
    if let Some(due) = t.due.as_deref().and_then(date) {
        let time = t
            .due_time
            .as_deref()
            .and_then(|s| NaiveTime::parse_from_str(s, "%H:%M").ok());
        u += DUE
            * match time {
                Some(time) => due_factor_timed(due, time, today, now_on(today)),
                None => due_factor(due, today),
            };
    }
    u += match t.priority {
        // Taskwarrior has three levels; the two extra Obsidian ones sit with their neighbours
        Some(Priority::Highest | Priority::High) => PRIORITY_H,
        Some(Priority::Medium) => PRIORITY_M,
        Some(Priority::Low) => PRIORITY_L,
        Some(Priority::Lowest) | None => 0.0,
    };
    if t.status == TaskStatus::Started {
        u += STARTED;
    }
    if t.blocking {
        u += BLOCKING;
    }
    if t.blocked {
        u += BLOCKED;
    }
    if t.scheduled
        .as_deref()
        .and_then(date)
        .is_some_and(|s| s <= today)
        && t.status != TaskStatus::Waiting
    {
        u += SCHEDULED;
    }
    if let Some(created) = t.created.as_deref().and_then(date) {
        let age = (today - created).num_days().max(0) as f64;
        u += AGE * (age / AGE_MAX_DAYS).min(1.0);
    }
    u += TAG * t.tags.len().min(3) as f64;
    if t.project.is_some() {
        u += PROJECT;
    }
    if t.hidden {
        u += WAITING;
    }
    if !t.annotations.is_empty() {
        u += ANNOTATIONS;
    }
    (u * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::tests::task;

    fn d(s: &str) -> NaiveDate {
        date(s).unwrap()
    }

    #[test]
    fn due_curve() {
        let today = d("2026-11-02");
        assert_eq!(due_factor(d("2026-10-01"), today), 1.0);
        assert_eq!(due_factor(d("2026-11-01"), today), 1.0);
        assert!((due_factor(d("2026-11-02"), today) - (14.0 * 0.8 / 21.0 + 0.2)).abs() < 1e-9);
        assert!((due_factor(d("2026-11-16"), today) - 0.2).abs() < 1e-9);
        assert_eq!(due_factor(d("2027-01-01"), today), 0.2);
        assert!(due_factor(d("2026-11-03"), today) > due_factor(d("2026-11-10"), today));
    }

    #[test]
    fn a_due_time_counts_once_it_has_passed() {
        let today = d("2026-11-02");
        let t = |h: u32, m: u32| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let day = due_factor(today, today);
        // before the time: a little less pressing than "some time today"
        let before = due_factor_timed(today, t(17, 0), today, Some(t(9, 0)));
        assert!(before < day && before > due_factor(d("2026-11-03"), today));
        // after it: overdue
        assert_eq!(
            due_factor_timed(today, t(17, 0), today, Some(t(17, 1))),
            1.0
        );
        // yesterday evening is overdue even at midnight
        assert_eq!(
            due_factor_timed(d("2026-11-01"), t(20, 0), today, None),
            1.0
        );
        let mut timed = task("- [ ] call 📅 2026-11-02 ⏰ 17:00");
        timed.due_time = Some("17:00".into());
        assert!(urgency(&timed, today) > 0.0);
    }

    #[test]
    fn coefficients_add_up() {
        let today = d("2026-11-02");
        let mut t = task("- [/] x #a #b #c #d ⏫ ➕ 2025-01-01 ⏳ 2026-11-01 📅 2026-10-30");
        t.project = Some("p".into());
        t.blocking = true;
        t.annotations.push(crate::types::Annotation {
            text: "n".into(),
            date: None,
        });
        // due 12 + H 6 + started 4 + blocking 8 + scheduled 5 + age 2 + tags 3 + project 1 + annotation 1
        assert_eq!(urgency(&t, today), 42.0);
        t.blocked = true;
        assert_eq!(urgency(&t, today), 37.0);
        let mut w = task("- [>] wait ⏳ 2026-12-01");
        w.hidden = true;
        assert_eq!(urgency(&w, today), -3.0);
        assert_eq!(urgency(&task("- [x] done ⏫"), today), 0.0);
        assert_eq!(urgency(&task("- [ ] low 🔽"), today), 1.8);
        assert_eq!(urgency(&task("- [ ] med 🔼"), today), 3.9);
    }
}
