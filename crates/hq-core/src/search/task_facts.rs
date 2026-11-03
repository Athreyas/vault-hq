//! The live task list, handed to Ask when a question is about tasks.
//!
//! Search finds notes that *talk about* tasks (dashboards, design notes), not the tasks
//! themselves, so "do I have any tasks today?" was answered from prose about task
//! collectors. The task engine already knows the answer; this turns its state into a
//! short, dated digest the model can quote.

use chrono::{Duration, NaiveDate};

use crate::types::{Priority, Task, TaskStatus};

/// Lines per section, so a large vault can't crowd the notes out of the prompt.
const PER_SECTION: usize = 12;

const WORDS: &[&str] = &[
    "task",
    "todo",
    "to-do",
    "to do",
    "due",
    "overdue",
    "late",
    "deadline",
    "remind",
    "agenda",
    "schedule",
    "plan for",
    "my day",
    "my week",
    "what's next",
    "whats next",
    "what is next",
    "what should i",
    "work on",
    "pending",
    "open items",
    "backlog",
];

/// Whether `question` (or the follow-up it continues) asks about tasks.
pub fn about_tasks(question: &str) -> bool {
    let q = question.to_lowercase();
    WORDS.iter().any(|w| q.contains(w))
}

/// Overdue, today, the next 7 days, and the most urgent of the rest, for open tasks.
/// `None` when nothing is open.
pub fn digest(tasks: &[Task], today: NaiveDate) -> Option<String> {
    let open: Vec<&Task> = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Pending | TaskStatus::Started | TaskStatus::Waiting
            ) && !t.hidden
        })
        .collect();
    if open.is_empty() {
        return None;
    }
    let date = |s: &Option<String>| {
        s.as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    };
    let week_end = today + Duration::days(7);
    let (mut overdue, mut on_today, mut week, mut rest) = (vec![], vec![], vec![], vec![]);
    for t in &open {
        let due = date(&t.due);
        let sched = date(&t.scheduled);
        match due {
            Some(d) if d < today => overdue.push(*t),
            Some(d) if d == today => on_today.push(*t),
            _ if sched == Some(today) => on_today.push(*t),
            Some(d) if d <= week_end => week.push(*t),
            _ => rest.push(*t),
        }
    }
    let by_due = |v: &mut Vec<&Task>| {
        v.sort_by(|a, b| {
            (a.due.as_deref(), a.due_time.as_deref())
                .cmp(&(b.due.as_deref(), b.due_time.as_deref()))
                .then(b.urgency.total_cmp(&a.urgency))
        })
    };
    by_due(&mut overdue);
    by_due(&mut on_today);
    by_due(&mut week);
    rest.sort_by(|a, b| b.urgency.total_cmp(&a.urgency));

    let mut out = format!(
        "Live task list from the task engine (today is {today}; {} open tasks in all):",
        open.len()
    );
    let mut section = |title: &str, v: &[&Task], empty: Option<&str>| {
        if v.is_empty() {
            if let Some(e) = empty {
                out.push_str(&format!("\n\n{title}: {e}"));
            }
            return;
        }
        out.push_str(&format!("\n\n{title} ({}):", v.len()));
        for t in v.iter().take(PER_SECTION) {
            out.push_str(&format!("\n- {}", line(t)));
        }
        if v.len() > PER_SECTION {
            out.push_str(&format!("\n- … and {} more", v.len() - PER_SECTION));
        }
    };
    section("Overdue", &overdue, Some("none"));
    section("Due or scheduled today", &on_today, Some("none"));
    section("Due in the next 7 days", &week, Some("none"));
    section("Most urgent without a date this week", &rest, None);
    Some(out)
}

fn line(t: &Task) -> String {
    let mut s = t.description.trim().to_string();
    if s.is_empty() {
        s = "(no description)".into();
    }
    if let Some(d) = &t.due {
        s.push_str(&format!(" — due {d}"));
        if let Some(h) = &t.due_time {
            s.push_str(&format!(" {h}"));
        }
    } else if let Some(d) = &t.scheduled {
        s.push_str(&format!(" — scheduled {d}"));
    }
    match t.priority {
        Some(Priority::Highest) | Some(Priority::High) => s.push_str(" — high priority"),
        _ => {}
    }
    if t.status == TaskStatus::Waiting {
        s.push_str(" — waiting");
    }
    let stem = t
        .note
        .rsplit('/')
        .next()
        .unwrap_or(&t.note)
        .trim_end_matches(".md");
    s.push_str(&format!(" [[{stem}]]"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(desc: &str, due: Option<&str>, status: TaskStatus) -> Task {
        Task {
            key: desc.into(),
            id: None,
            vault: "v".into(),
            note: "v:01-Projects/garden.md".into(),
            note_title: "Garden".into(),
            line: 1,
            status,
            description: desc.into(),
            tags: vec![],
            project: None,
            priority: None,
            created: None,
            start: None,
            scheduled: None,
            due: due.map(String::from),
            due_time: None,
            done: None,
            cancelled: None,
            recur: None,
            depends: vec![],
            annotations: vec![],
            blocked: false,
            blocking: false,
            hidden: false,
            urgency: 1.0,
            raw: String::new(),
        }
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn spots_task_questions_only() {
        assert!(about_tasks("do i have any tasks today ?"));
        assert!(about_tasks("What's due this week?"));
        assert!(about_tasks("what should I work on next"));
        assert!(!about_tasks("how did I set up the reverse proxy?"));
    }

    #[test]
    fn sorts_tasks_into_dated_sections() {
        let mut timed = task("Call the bank", Some("2026-11-03"), TaskStatus::Pending);
        timed.due_time = Some("17:00".into());
        let all = vec![
            task("Lay the main line", Some("2026-11-01"), TaskStatus::Pending),
            timed,
            task("Order fittings", Some("2026-11-06"), TaskStatus::Pending),
            task("Read chapter 7", None, TaskStatus::Pending),
            task("Already done", Some("2026-11-03"), TaskStatus::Done),
        ];
        let d = digest(&all, day("2026-11-03")).unwrap();
        assert!(d.contains("4 open tasks"), "{d}");
        assert!(d.contains("Overdue (1):\n- Lay the main line — due 2026-11-01 [[garden]]"));
        assert!(d.contains("Due or scheduled today (1):\n- Call the bank — due 2026-11-03 17:00"));
        assert!(d.contains("Due in the next 7 days (1):\n- Order fittings"));
        assert!(d.contains("Read chapter 7"));
        assert!(!d.contains("Already done"));
    }

    #[test]
    fn says_none_for_an_empty_day_and_nothing_when_all_done() {
        let all = vec![task("Later", Some("2026-12-01"), TaskStatus::Pending)];
        let d = digest(&all, day("2026-11-03")).unwrap();
        assert!(d.contains("Due or scheduled today: none"), "{d}");
        let done = vec![task("x", None, TaskStatus::Done)];
        assert_eq!(digest(&done, day("2026-11-03")), None);
    }
}
