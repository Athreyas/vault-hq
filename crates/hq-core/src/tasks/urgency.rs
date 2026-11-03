//! Urgency with Taskwarrior's default coefficients (SPEC 4.2), so the order feels familiar.

use chrono::NaiveDate;

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
    let overdue = (today - due).num_days() as f64;
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
        u += DUE * due_factor(due, today);
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
