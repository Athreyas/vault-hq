//! Filter, sort and group (SPEC 4.2).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use chrono::{Days, NaiveDate};

use super::{check_date, date, resolve_today, TaskIndex};
use crate::error::Result;
use crate::types::{Priority, Task, TaskGroup, TaskGroupBy, TaskGroups, TaskQuery, TaskSort};

/// Run a query over the index.
pub fn run(index: &TaskIndex, q: &TaskQuery) -> Result<TaskGroups> {
    let today = resolve_today(q.today.as_deref())?;
    over(index.finished(today), q, today)
}

/// Run a query over finished tasks (see [`super::finish`]).
pub fn over(all: Vec<Task>, q: &TaskQuery, today: NaiveDate) -> Result<TaskGroups> {
    for (name, d) in [
        ("due from", &q.due_from),
        ("due to", &q.due_to),
        ("scheduled from", &q.scheduled_from),
        ("scheduled to", &q.scheduled_to),
    ] {
        if let Some(d) = d {
            check_date(name, d)?;
        }
    }
    let mut projects = BTreeSet::new();
    let mut tags = BTreeSet::new();
    for t in all.iter().filter(|t| t.status.is_open()) {
        if let Some(p) = &t.project {
            projects.insert(p.clone());
        }
        tags.extend(t.tags.iter().cloned());
    }
    let words: Vec<String> = q
        .text
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut hits: Vec<Task> = all.into_iter().filter(|t| matches(t, q, &words)).collect();
    hits.sort_by(|a, b| compare(a, b, q.sort));
    let total = hits.len() as u32;
    if let Some(limit) = q.limit {
        hits.truncate(limit as usize);
    }
    Ok(TaskGroups {
        groups: group(hits, q.group, today),
        total,
        projects: projects.into_iter().collect(),
        tags: tags.into_iter().collect(),
    })
}

fn eq_tag(a: &str, b: &str) -> bool {
    a.trim_start_matches('#')
        .eq_ignore_ascii_case(b.trim_start_matches('#'))
}

/// `home` matches `home` and `home/garden` (and Taskwarrior-style `home.garden`).
pub fn project_matches(project: Option<&str>, want: &str) -> bool {
    let Some(p) = project else { return false };
    let (p, w) = (p.to_lowercase(), want.to_lowercase());
    p == w || p.starts_with(&format!("{w}/")) || p.starts_with(&format!("{w}."))
}

fn in_range(value: Option<&str>, from: Option<&str>, to: Option<&str>) -> bool {
    if from.is_none() && to.is_none() {
        return true;
    }
    let Some(v) = value.and_then(date) else {
        return false;
    };
    from.and_then(date).is_none_or(|f| v >= f) && to.and_then(date).is_none_or(|t| v <= t)
}

fn matches(t: &Task, q: &TaskQuery, words: &[String]) -> bool {
    if q.status.is_empty() {
        if !t.status.is_open() {
            return false;
        }
    } else if !q.status.contains(&t.status) {
        return false;
    }
    if t.hidden && !q.include_hidden {
        return false;
    }
    if let Some(p) = &q.project {
        if !project_matches(t.project.as_deref(), p) {
            return false;
        }
    }
    let has = |tag: &String| t.tags.iter().any(|x| eq_tag(x, tag));
    if !q.tags_all.iter().all(has) {
        return false;
    }
    if !q.tags_any.is_empty() && !q.tags_any.iter().any(has) {
        return false;
    }
    if q.tags_none.iter().any(has) {
        return false;
    }
    if !q.priority.is_empty() || q.no_priority {
        let ok = match t.priority {
            Some(p) => q.priority.contains(&p),
            None => q.no_priority,
        };
        if !ok {
            return false;
        }
    }
    if !in_range(t.due.as_deref(), q.due_from.as_deref(), q.due_to.as_deref()) {
        return false;
    }
    if !in_range(
        t.scheduled.as_deref(),
        q.scheduled_from.as_deref(),
        q.scheduled_to.as_deref(),
    ) {
        return false;
    }
    if !q.vaults.is_empty() && !q.vaults.contains(&t.vault) {
        return false;
    }
    if let Some(b) = q.blocked {
        if t.blocked != b {
            return false;
        }
    }
    if !words.is_empty() {
        let mut hay = t.description.to_lowercase();
        for x in &t.tags {
            hay.push(' ');
            hay.push_str(&x.to_lowercase());
        }
        if let Some(p) = &t.project {
            hay.push(' ');
            hay.push_str(&p.to_lowercase());
        }
        for a in &t.annotations {
            hay.push(' ');
            hay.push_str(&a.text.to_lowercase());
        }
        if !words.iter().all(|w| hay.contains(w.as_str())) {
            return false;
        }
    }
    true
}

/// `None` sorts last whatever the direction.
fn opt_cmp<T: Ord>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn by_urgency(a: &Task, b: &Task) -> Ordering {
    b.urgency.partial_cmp(&a.urgency).unwrap_or(Ordering::Equal)
}

pub fn compare(a: &Task, b: &Task, sort: TaskSort) -> Ordering {
    let primary = match sort {
        TaskSort::Urgency => by_urgency(a, b),
        // a timed task sorts before an untimed one on the same day (that one is "by end of day")
        TaskSort::Due => opt_cmp(a.due.as_deref(), b.due.as_deref()).then_with(|| {
            opt_cmp(
                a.due
                    .as_ref()
                    .map(|_| a.due_time.as_deref().unwrap_or("24:00")),
                b.due
                    .as_ref()
                    .map(|_| b.due_time.as_deref().unwrap_or("24:00")),
            )
        }),
        TaskSort::Priority => opt_cmp(a.priority, b.priority),
        // newest first
        TaskSort::Created => opt_cmp(
            a.created.as_deref().map(std::cmp::Reverse),
            b.created.as_deref().map(std::cmp::Reverse),
        ),
        TaskSort::Project => opt_cmp(
            a.project.as_ref().map(|p| p.to_lowercase()),
            b.project.as_ref().map(|p| p.to_lowercase()),
        ),
    };
    primary
        .then_with(|| by_urgency(a, b))
        .then_with(|| a.note.cmp(&b.note))
        .then_with(|| a.line.cmp(&b.line))
}

/// The due bucket of a task: overdue, today, week (the next 7 days), later, none.
pub fn due_bucket(due: Option<&str>, today: NaiveDate) -> &'static str {
    let Some(d) = due.and_then(date) else {
        return "none";
    };
    let week = today.checked_add_days(Days::new(7)).unwrap_or(today);
    if d < today {
        "overdue"
    } else if d == today {
        "today"
    } else if d <= week {
        "week"
    } else {
        "later"
    }
}

const DUE_BUCKETS: [(&str, &str); 5] = [
    ("overdue", "Overdue"),
    ("today", "Today"),
    ("week", "This week"),
    ("later", "Later"),
    ("none", "No due date"),
];

fn priority_key(p: Option<Priority>) -> (&'static str, &'static str) {
    match p {
        Some(Priority::Highest) => ("highest", "Highest"),
        Some(Priority::High) => ("high", "High"),
        Some(Priority::Medium) => ("medium", "Medium"),
        Some(Priority::Low) => ("low", "Low"),
        Some(Priority::Lowest) => ("lowest", "Lowest"),
        None => ("none", "No priority"),
    }
}

/// Group sorted tasks. Groups keep the order of their first task, except due buckets and
/// priorities (fixed order) and tags (alphabetical, untagged last). A task with several
/// tags appears under each.
pub fn group(tasks: Vec<Task>, by: TaskGroupBy, today: NaiveDate) -> Vec<TaskGroup> {
    let mut groups: Vec<TaskGroup> = Vec::new();
    let mut push =
        |key: String, label: String, t: Task| match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => g.tasks.push(t),
            None => groups.push(TaskGroup {
                key,
                label,
                tasks: vec![t],
            }),
        };
    for t in tasks {
        match by {
            TaskGroupBy::None => push("all".into(), "All".into(), t),
            TaskGroupBy::Project => {
                let (k, l) = match &t.project {
                    Some(p) => (p.clone(), p.clone()),
                    None => (String::new(), "No project".into()),
                };
                push(k, l, t)
            }
            TaskGroupBy::Tag => {
                if t.tags.is_empty() {
                    push(String::new(), "No tag".into(), t);
                } else {
                    for tag in t.tags.clone() {
                        push(tag.clone(), format!("#{tag}"), t.clone());
                    }
                }
            }
            TaskGroupBy::Due => {
                let b = due_bucket(t.due.as_deref(), today);
                let label = DUE_BUCKETS
                    .iter()
                    .find(|(k, _)| *k == b)
                    .map(|(_, l)| *l)
                    .unwrap_or(b);
                push(b.into(), label.into(), t)
            }
            TaskGroupBy::Priority => {
                let (k, l) = priority_key(t.priority);
                push(k.into(), l.into(), t)
            }
            TaskGroupBy::Vault => push(t.vault.clone(), t.vault.clone(), t),
            TaskGroupBy::Note => push(t.note.clone(), t.note_title.clone(), t),
            TaskGroupBy::Status => {
                let k = serde_json::to_value(t.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let mut l = k.clone();
                if let Some(first) = l.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                push(k, l, t)
            }
        }
    }
    match by {
        TaskGroupBy::Due => groups.sort_by_key(|g| {
            DUE_BUCKETS
                .iter()
                .position(|(k, _)| *k == g.key)
                .unwrap_or(99)
        }),
        TaskGroupBy::Priority => groups.sort_by_key(|g| {
            ["highest", "high", "medium", "low", "lowest", "none"]
                .iter()
                .position(|k| *k == g.key)
                .unwrap_or(99)
        }),
        TaskGroupBy::Tag => groups.sort_by(|a, b| {
            (a.key.is_empty(), a.key.to_lowercase()).cmp(&(b.key.is_empty(), b.key.to_lowercase()))
        }),
        TaskGroupBy::Project => groups.sort_by_key(|g| g.key.is_empty()),
        _ => {}
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::finish;
    use crate::tasks::tests::task;
    use crate::types::TaskStatus;

    fn today() -> NaiveDate {
        date("2026-11-02").unwrap()
    }

    fn sample() -> Vec<Task> {
        let mut v = vec![
            task("- [ ] Overdue fence #garden #outdoor ⏫ 📅 2026-10-30 🆔 aaa111"),
            task("- [ ] Plant bulbs #garden #project/home/garden ⛔ aaa111"),
            task("- [/] Read chapter 7 #learning 🔼 📅 2026-11-02"),
            task("- [ ] Book flights #travel 🔽 📅 2026-11-06"),
            task("- [ ] Taxes #project/home 📅 2027-01-15"),
            task("- [x] Done thing #garden ✅ 2026-11-01"),
            task("- [-] Dropped ❌ 2026-11-01"),
            task("- [>] Wait for parts ⏳ 2026-12-01"),
            task("- [>] Parts arrived? ⏳ 2026-11-01"),
        ];
        finish(&mut v, today());
        v
    }

    fn descs(g: &TaskGroups) -> Vec<&str> {
        g.groups
            .iter()
            .flat_map(|g| g.tasks.iter().map(|t| t.description.as_str()))
            .collect()
    }

    fn q() -> TaskQuery {
        TaskQuery::default()
    }

    #[test]
    fn default_is_open_and_visible_by_urgency() {
        let g = over(sample(), &q(), today()).unwrap();
        assert_eq!(g.total, 6);
        let d = descs(&g);
        assert_eq!(d[0], "Overdue fence", "overdue + high + blocking first");
        assert!(!d.contains(&"Wait for parts"));
        assert!(d.contains(&"Parts arrived?"));
        assert!(!d.contains(&"Done thing"));
        assert_eq!(g.projects, vec!["home", "home/garden"]);
        assert!(g.tags.contains(&"garden".to_string()));
    }

    #[test]
    fn filters() {
        let mut f = q();
        f.project = Some("home".into());
        assert_eq!(
            descs(&over(sample(), &f, today()).unwrap()),
            vec!["Taxes", "Plant bulbs"]
        );
        let mut f = q();
        f.tags_all = vec!["garden".into(), "#OUTDOOR".into()];
        assert_eq!(
            descs(&over(sample(), &f, today()).unwrap()),
            vec!["Overdue fence"]
        );
        let mut f = q();
        f.tags_any = vec!["travel".into(), "learning".into()];
        f.tags_none = vec!["travel".into()];
        assert_eq!(
            descs(&over(sample(), &f, today()).unwrap()),
            vec!["Read chapter 7"]
        );
        let mut f = q();
        f.priority = vec![Priority::Low];
        f.no_priority = true;
        assert_eq!(over(sample(), &f, today()).unwrap().total, 4);
        let mut f = q();
        f.due_from = Some("2026-11-01".into());
        f.due_to = Some("2026-11-30".into());
        assert_eq!(
            descs(&over(sample(), &f, today()).unwrap()),
            vec!["Read chapter 7", "Book flights"]
        );
        let mut f = q();
        f.status = vec![TaskStatus::Done, TaskStatus::Cancelled];
        assert_eq!(over(sample(), &f, today()).unwrap().total, 2);
        let mut f = q();
        f.include_hidden = true;
        assert_eq!(over(sample(), &f, today()).unwrap().total, 7);
        let mut f = q();
        f.blocked = Some(true);
        assert_eq!(
            descs(&over(sample(), &f, today()).unwrap()),
            vec!["Plant bulbs"]
        );
        let mut f = q();
        f.text = Some("CHAPTER read".into());
        assert_eq!(over(sample(), &f, today()).unwrap().total, 1);
        let mut f = q();
        f.text = Some("garden".into());
        assert_eq!(
            over(sample(), &f, today()).unwrap().total,
            2,
            "tags count as text"
        );
        let mut f = q();
        f.vaults = vec!["other".into()];
        assert_eq!(over(sample(), &f, today()).unwrap().total, 0);
        let mut f = q();
        f.limit = Some(2);
        let g = over(sample(), &f, today()).unwrap();
        assert_eq!((g.total, descs(&g).len()), (6, 2));
        let mut f = q();
        f.due_to = Some("soon".into());
        assert!(over(sample(), &f, today()).is_err());
    }

    #[test]
    fn sorts() {
        let mut f = q();
        f.sort = TaskSort::Due;
        let g = over(sample(), &f, today()).unwrap();
        let d = descs(&g);
        assert_eq!(
            &d[..4],
            &["Overdue fence", "Read chapter 7", "Book flights", "Taxes"]
        );
        f.sort = TaskSort::Priority;
        let g = over(sample(), &f, today()).unwrap();
        let d = descs(&g);
        assert_eq!(
            &d[..3],
            &["Overdue fence", "Read chapter 7", "Book flights"]
        );
        f.sort = TaskSort::Project;
        let g = over(sample(), &f, today()).unwrap();
        let d = descs(&g);
        assert_eq!(&d[..2], &["Taxes", "Plant bulbs"]);
    }

    #[test]
    fn groups() {
        let mut f = q();
        f.group = TaskGroupBy::Due;
        let g = over(sample(), &f, today()).unwrap();
        let keys: Vec<_> = g.groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, vec!["overdue", "today", "week", "later", "none"]);
        assert_eq!(g.groups[0].label, "Overdue");
        f.group = TaskGroupBy::Tag;
        let g = over(sample(), &f, today()).unwrap();
        let garden = g.groups.iter().find(|g| g.key == "garden").unwrap();
        assert_eq!(garden.tasks.len(), 2);
        assert_eq!(g.groups.last().unwrap().label, "No tag");
        f.group = TaskGroupBy::Priority;
        let g = over(sample(), &f, today()).unwrap();
        assert_eq!(g.groups[0].key, "high");
        assert_eq!(g.groups.last().unwrap().key, "none");
        f.group = TaskGroupBy::Project;
        let g = over(sample(), &f, today()).unwrap();
        assert_eq!(g.groups.last().unwrap().label, "No project");
        f.group = TaskGroupBy::Status;
        let g = over(sample(), &f, today()).unwrap();
        assert!(g.groups.iter().any(|g| g.label == "Started"));
        f.group = TaskGroupBy::Note;
        assert_eq!(over(sample(), &f, today()).unwrap().groups.len(), 1);
    }

    #[test]
    fn buckets() {
        let t = today();
        assert_eq!(due_bucket(Some("2026-11-01"), t), "overdue");
        assert_eq!(due_bucket(Some("2026-11-02"), t), "today");
        assert_eq!(due_bucket(Some("2026-11-09"), t), "week");
        assert_eq!(due_bucket(Some("2026-11-10"), t), "later");
        assert_eq!(due_bucket(None, t), "none");
        assert!(project_matches(Some("home.garden"), "home"));
        assert!(!project_matches(Some("homework"), "home"));
    }

    #[test]
    fn timed_tasks_sort_before_untimed_on_the_same_day() {
        let mut a = crate::tasks::tests::task("- [ ] all day 📅 2026-11-06");
        let mut b = crate::tasks::tests::task("- [ ] at nine 📅 2026-11-06 ⏰ 09:00");
        let mut c = crate::tasks::tests::task("- [ ] at eight 📅 2026-11-06 ⏰ 08:00");
        let d = crate::tasks::tests::task("- [ ] earlier day 📅 2026-11-05");
        for t in [&mut a, &mut b, &mut c] {
            t.urgency = 0.0;
        }
        let mut v = [a, b, c, d];
        v.sort_by(|x, y| compare(x, y, TaskSort::Due));
        let order: Vec<&str> = v.iter().map(|t| t.description.as_str()).collect();
        assert_eq!(order, ["earlier day", "at eight", "at nine", "all day"]);
    }
}
