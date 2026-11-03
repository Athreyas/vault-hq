//! Reminders for tasks with a time (`📅 day ⏰ HH:MM`): which ones are due to be shown now,
//! each exactly once. The app's job loop asks every 30 s and shows a notification for each.
//!
//! What has fired is kept in `reminders.json` in the app's support folder (a preference,
//! not knowledge: losing it means at most a repeat). A reminder is keyed by the task and
//! its due moment, so moving a task to another time arms it again.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::Task;

pub const FILE: &str = "reminders.json";
/// How late a reminder may still be shown, e.g. after the Mac was asleep or HQ closed.
/// Older ones are skipped rather than arriving in a burst.
pub const GRACE: Duration = Duration::hours(6);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Fired {
    pub keys: BTreeSet<String>,
    /// Notification permission was asked for (the first time a timed task existed).
    pub asked: bool,
}

/// When the task is due, if it has a day and a time.
pub fn due_at(t: &Task) -> Option<NaiveDateTime> {
    let d = NaiveDate::parse_from_str(t.due.as_deref()?, "%Y-%m-%d").ok()?;
    let time = NaiveTime::parse_from_str(t.due_time.as_deref()?, "%H:%M").ok()?;
    Some(d.and_time(time))
}

/// `<vault>:<id or key>@<day> <time>`.
pub fn key(t: &Task) -> Option<String> {
    let at = due_at(t)?;
    let who = t.id.as_deref().unwrap_or(&t.key);
    Some(format!("{}:{who}@{}", t.vault, at.format("%Y-%m-%d %H:%M")))
}

/// Open tasks with a time, the ones a reminder could be for.
pub fn timed(tasks: &[Task]) -> impl Iterator<Item = &Task> {
    tasks
        .iter()
        .filter(|t| t.status.is_open() && !t.hidden && due_at(t).is_some())
}

/// The tasks to remind about now, marking them fired. Keys of tasks that no longer exist
/// or have moved are forgotten, so the file never grows.
pub fn due_now<'a>(tasks: &'a [Task], now: NaiveDateTime, fired: &mut Fired) -> Vec<&'a Task> {
    let live: BTreeSet<String> = timed(tasks).filter_map(key).collect();
    fired.keys.retain(|k| live.contains(k));
    let mut out = Vec::new();
    for t in timed(tasks) {
        let (Some(at), Some(k)) = (due_at(t), key(t)) else {
            continue;
        };
        if at <= now && now - at <= GRACE && !fired.keys.contains(&k) {
            fired.keys.insert(k);
            out.push(t);
        }
    }
    out
}

/// The notification's words: the task, and where it lives.
pub fn message(t: &Task) -> (String, String) {
    let time = t.due_time.clone().unwrap_or_default();
    (
        t.description.clone(),
        format!("Due {time} · {}", t.note_title),
    )
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(support_dir: &Path) -> Store {
        Store {
            path: support_dir.join(FILE),
        }
    }

    /// What fired before; an unreadable file starts over (at worst a repeat).
    pub fn load(&self) -> Fired {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, f: &Fired) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(f)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::tests::task;
    use crate::types::TaskStatus;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn timed_task(id: &str, due: &str, time: &str) -> Task {
        let mut t = task(&format!("- [ ] call {id} 📅 {due} ⏰ {time} 🆔 {id}"));
        t.id = Some(id.into());
        t.due_time = Some(time.into());
        t
    }

    #[test]
    fn fires_once_at_the_time() {
        let tasks = vec![
            timed_task("aaa111", "2026-11-06", "09:00"),
            timed_task("bbb222", "2026-11-06", "17:00"),
            task("- [ ] no time 📅 2026-11-06"),
        ];
        let mut f = Fired::default();
        assert!(due_now(&tasks, at("2026-11-06 08:59"), &mut f).is_empty());
        let now = due_now(&tasks, at("2026-11-06 09:00"), &mut f);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].id.as_deref(), Some("aaa111"));
        // 30 s later: not again
        assert!(due_now(&tasks, at("2026-11-06 09:01"), &mut f).is_empty());
        // across a restart, from the file
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.save(&f).unwrap();
        let mut f = store.load();
        assert!(due_now(&tasks, at("2026-11-06 09:02"), &mut f).is_empty());
        assert_eq!(due_now(&tasks, at("2026-11-06 17:00"), &mut f).len(), 1);
    }

    #[test]
    fn late_done_and_moved_tasks() {
        let mut f = Fired::default();
        // HQ was closed all day: a reminder 7 hours late is skipped
        let tasks = vec![timed_task("aaa111", "2026-11-06", "09:00")];
        assert!(due_now(&tasks, at("2026-11-06 16:01"), &mut f).is_empty());
        // within the grace period it still shows
        assert_eq!(due_now(&tasks, at("2026-11-06 14:00"), &mut f).len(), 1);
        // moved to a later time: armed again, and the old key forgotten
        let moved = vec![timed_task("aaa111", "2026-11-06", "15:00")];
        assert_eq!(due_now(&moved, at("2026-11-06 15:00"), &mut f).len(), 1);
        assert_eq!(f.keys.len(), 1);
        // done tasks never fire
        let mut done = timed_task("ccc333", "2026-11-06", "15:00");
        done.status = TaskStatus::Done;
        assert!(due_now(&[done], at("2026-11-06 15:00"), &mut f).is_empty());
        let (title, body) = message(&moved[0]);
        assert_eq!(title, "call aaa111");
        assert!(body.starts_with("Due 15:00"));
    }
}
