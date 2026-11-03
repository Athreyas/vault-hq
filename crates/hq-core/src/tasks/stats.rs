//! Task statistics (SPEC 4.2), derived only from `➕`, `✅`, `❌` and the current state.
//!
//! A task with no `➕` counts as open since before the window; a closed task with no
//! end date counts as closed before it. So history is never invented, only left out.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};

use super::query::project_matches;
use super::{date, fmt, resolve_today, TaskIndex};
use crate::error::Result;
use crate::types::{DayStat, Priority, StatBreakdown, StatsQuery, Task, TaskStats, TaskStatus};

pub fn run(index: &TaskIndex, q: &StatsQuery) -> Result<TaskStats> {
    let today = resolve_today(q.today.as_deref())?;
    Ok(over(&index.finished(today), q, today))
}

struct Life {
    created: Option<NaiveDate>,
    /// When it was done or cancelled; `None` while open (or closed at an unknown date).
    end: Option<NaiveDate>,
    status: TaskStatus,
    due: Option<NaiveDate>,
}

impl Life {
    fn of(t: &Task) -> Life {
        let end = match t.status {
            TaskStatus::Done => t.done.as_deref().and_then(date),
            TaskStatus::Cancelled => t.cancelled.as_deref().and_then(date),
            _ => None,
        };
        Life {
            created: t.created.as_deref().and_then(date),
            end,
            status: t.status,
            due: t.due.as_deref().and_then(date),
        }
    }

    /// Open at the end of `d`.
    fn open_on(&self, d: NaiveDate) -> bool {
        if self.created.is_some_and(|c| c > d) {
            return false;
        }
        if self.status.is_open() {
            return true;
        }
        self.end.is_some_and(|e| e > d)
    }

    fn overdue_on(&self, d: NaiveDate) -> bool {
        self.open_on(d) && self.due.is_some_and(|due| due < d)
    }
}

fn back(today: NaiveDate, n: u64) -> NaiveDate {
    today.checked_sub_days(Days::new(n)).unwrap_or(today)
}

fn in_scope(t: &Task, q: &StatsQuery) -> bool {
    (q.vaults.is_empty() || q.vaults.contains(&t.vault))
        && q.project
            .as_deref()
            .is_none_or(|p| project_matches(t.project.as_deref(), p))
        && q.tag.as_deref().is_none_or(|tag| {
            let tag = tag.trim_start_matches('#');
            t.tags.iter().any(|x| x.eq_ignore_ascii_case(tag))
        })
}

pub fn over(all: &[Task], q: &StatsQuery, today: NaiveDate) -> TaskStats {
    let n = q.days.unwrap_or(30).clamp(1, 730) as u64;
    let from = back(today, n - 1);
    let scoped: Vec<&Task> = all.iter().filter(|t| in_scope(t, q)).collect();
    let lives: Vec<Life> = scoped.iter().map(|t| Life::of(t)).collect();

    let mut days = Vec::with_capacity(n as usize);
    let mut d = from;
    while d <= today {
        let mut s = DayStat {
            date: fmt(d),
            created: 0,
            done: 0,
            cancelled: 0,
            open: 0,
            overdue: 0,
        };
        for l in &lives {
            if l.created == Some(d) {
                s.created += 1;
            }
            if l.end == Some(d) {
                match l.status {
                    TaskStatus::Done => s.done += 1,
                    TaskStatus::Cancelled => s.cancelled += 1,
                    _ => {}
                }
            }
            if l.open_on(d) {
                s.open += 1;
                if l.overdue_on(d) {
                    s.overdue += 1;
                }
            }
        }
        days.push(s);
        d = match d.succ_opt() {
            Some(x) => x,
            None => break,
        };
    }

    let done_since = |since: NaiveDate| {
        lives
            .iter()
            .filter(|l| l.status == TaskStatus::Done && l.end.is_some_and(|e| e >= since))
            .count() as u32
    };
    let completion = |since: NaiveDate| {
        let made: Vec<&Life> = lives
            .iter()
            .filter(|l| l.created.is_some_and(|c| c >= since && c <= today))
            .collect();
        if made.is_empty() {
            0.0
        } else {
            let done = made.iter().filter(|l| l.status == TaskStatus::Done).count();
            round(done as f64 / made.len() as f64)
        }
    };
    let open_on = |d: NaiveDate| lives.iter().filter(|l| l.open_on(d)).count() as f64;

    let open = lives.iter().filter(|l| l.status.is_open()).count() as u32;
    let overdue = lives
        .iter()
        .filter(|l| l.status.is_open() && l.due.is_some_and(|d| d < today))
        .count() as u32;
    let done_7 = done_since(back(today, 6));
    let net_per_day = round((open_on(today) - open_on(back(today, 7))) / 7.0);
    let projected_zero = (net_per_day < 0.0 && open > 0).then(|| {
        let days = (open as f64 / -net_per_day).ceil() as u64;
        fmt(today.checked_add_days(Days::new(days)).unwrap_or(today))
    });

    let window = |l: &Life| l.end.is_some_and(|e| e >= from);
    let mut by_project: BTreeMap<String, StatBreakdown> = BTreeMap::new();
    let mut by_tag: BTreeMap<String, StatBreakdown> = BTreeMap::new();
    let mut by_priority: BTreeMap<u8, StatBreakdown> = BTreeMap::new();
    for (t, l) in scoped.iter().zip(&lives) {
        let add = |b: &mut StatBreakdown| {
            if l.status.is_open() {
                b.open += 1;
                if l.due.is_some_and(|d| d < today) {
                    b.overdue += 1;
                }
            } else if window(l) {
                match l.status {
                    TaskStatus::Done => b.done += 1,
                    _ => b.cancelled += 1,
                }
            }
        };
        let pk = t.project.clone().unwrap_or_default();
        add(by_project
            .entry(pk.clone())
            .or_insert_with(|| breakdown(&pk)));
        for tag in &t.tags {
            add(by_tag.entry(tag.clone()).or_insert_with(|| breakdown(tag)));
        }
        let (rank, name) = priority_name(t.priority);
        add(by_priority.entry(rank).or_insert_with(|| breakdown(name)));
    }
    let keep = |b: &StatBreakdown| b.open + b.done + b.cancelled > 0;
    let mut by_project: Vec<_> = by_project.into_values().filter(keep).collect();
    by_project.sort_by_key(|b| std::cmp::Reverse(b.open + b.done));
    let mut by_tag: Vec<_> = by_tag.into_values().filter(keep).collect();
    by_tag.sort_by_key(|b| std::cmp::Reverse(b.open + b.done));

    TaskStats {
        from: fmt(from),
        to: fmt(today),
        days,
        open,
        overdue,
        done_7,
        done_30: done_since(back(today, 29)),
        completion_7: completion(back(today, 6)),
        completion_30: completion(back(today, 29)),
        burn_rate: round(done_7 as f64 / 7.0),
        net_per_day,
        projected_zero,
        by_project,
        by_tag,
        by_priority: by_priority.into_values().filter(keep).collect(),
    }
}

fn breakdown(key: &str) -> StatBreakdown {
    StatBreakdown {
        key: key.to_string(),
        open: 0,
        done: 0,
        cancelled: 0,
        overdue: 0,
    }
}

fn priority_name(p: Option<Priority>) -> (u8, &'static str) {
    match p {
        Some(Priority::Highest) => (0, "highest"),
        Some(Priority::High) => (1, "high"),
        Some(Priority::Medium) => (2, "medium"),
        Some(Priority::Low) => (3, "low"),
        Some(Priority::Lowest) => (4, "lowest"),
        None => (5, "none"),
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::finish;
    use crate::tasks::tests::task;

    fn today() -> NaiveDate {
        date("2026-11-10").unwrap()
    }

    fn sample() -> Vec<Task> {
        let mut v = vec![
            task("- [ ] a #garden ⏫ ➕ 2026-11-01 📅 2026-11-05"),
            task("- [ ] b #garden ➕ 2026-11-08"),
            task("- [x] c #garden ➕ 2026-11-02 ✅ 2026-11-09"),
            task("- [x] d #home ➕ 2026-11-04 ✅ 2026-11-10"),
            task("- [-] e #home ➕ 2026-11-04 ❌ 2026-11-06"),
            task("- [ ] f no dates"),
            task("- [x] g old, no end date"),
        ];
        finish(&mut v, today());
        v
    }

    fn day<'a>(s: &'a TaskStats, d: &str) -> &'a DayStat {
        s.days.iter().find(|x| x.date == d).unwrap()
    }

    #[test]
    fn per_day_counts_and_open_over_time() {
        let q = StatsQuery {
            days: Some(14),
            ..Default::default()
        };
        let s = over(&sample(), &q, today());
        assert_eq!(s.days.len(), 14);
        assert_eq!(
            (s.from.as_str(), s.to.as_str()),
            ("2026-10-28", "2026-11-10")
        );
        // before any ➕: only the undated open task
        assert_eq!(day(&s, "2026-10-30").open, 1);
        assert_eq!(day(&s, "2026-11-04").created, 2);
        assert_eq!(day(&s, "2026-11-04").open, 5);
        assert_eq!(day(&s, "2026-11-06").cancelled, 1);
        assert_eq!(day(&s, "2026-11-06").open, 4, "a c d f");
        assert_eq!(day(&s, "2026-11-06").overdue, 1, "a was due on the 5th");
        assert_eq!(day(&s, "2026-11-10").done, 1);
        assert_eq!(day(&s, "2026-11-10").open, 3, "a b f");
        assert_eq!(s.open, 3);
        assert_eq!(s.overdue, 1);
        assert_eq!(s.done_7, 2);
        assert_eq!(s.done_30, 2);
        // created in the last 7 days (from the 4th): b, d, e → one done
        assert!((s.completion_7 - 0.333).abs() < 1e-9);
        assert!((s.completion_30 - 0.4).abs() < 1e-9);
        assert!((s.burn_rate - 0.286).abs() < 1e-9);
    }

    #[test]
    fn breakdowns_and_scope() {
        let s = over(&sample(), &StatsQuery::default(), today());
        let garden = s.by_tag.iter().find(|b| b.key == "garden").unwrap();
        assert_eq!((garden.open, garden.done, garden.overdue), (2, 1, 1));
        let home = s.by_tag.iter().find(|b| b.key == "home").unwrap();
        assert_eq!((home.done, home.cancelled), (1, 1));
        assert_eq!(s.by_priority[0].key, "high");
        assert_eq!(s.by_project.len(), 1, "no projects: one empty key");

        let q = StatsQuery {
            tag: Some("#garden".into()),
            days: Some(7),
            ..Default::default()
        };
        let s = over(&sample(), &q, today());
        assert_eq!(s.open, 2);
        assert_eq!(s.days.len(), 7);
        // open on the 3rd (a, c) vs the 10th (a, b): flat, so no projection
        assert_eq!(s.net_per_day, 0.0);
        assert_eq!(s.projected_zero, None);
    }

    #[test]
    fn burn_down_projection() {
        let mut v = vec![
            task("- [ ] left ➕ 2026-11-01"),
            task("- [x] one ➕ 2026-11-01 ✅ 2026-11-05"),
            task("- [x] two ➕ 2026-11-01 ✅ 2026-11-06"),
            task("- [x] three ➕ 2026-11-01 ✅ 2026-11-07"),
            task("- [x] four ➕ 2026-11-01 ✅ 2026-11-08"),
        ];
        finish(&mut v, today());
        let s = over(&v, &StatsQuery::default(), today());
        assert_eq!(s.days.len(), 30);
        assert!((s.net_per_day - (-4.0 / 7.0)).abs() < 0.001);
        assert_eq!(s.projected_zero.as_deref(), Some("2026-11-12"));
        assert_eq!(over(&[], &StatsQuery::default(), today()).open, 0);
    }
}
