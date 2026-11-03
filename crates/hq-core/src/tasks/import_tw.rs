//! Import from Taskwarrior's `task export` JSON (SPEC 4.2). A dry run first reports what
//! would be written; the real run writes each project's tasks into its project note
//! (created if missing) and the rest into `09-Planning/Tasks.md`. Taskwarrior's own data
//! is only ever read: the export runs with hooks, garbage collection and recurrence off.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use super::model::{clean_tag, FieldKind};
use super::mutate::{append_lines, find_project_note, render_new, LOOSE_TASKS, PROJECTS_DIR};
use super::{fmt, parse, TaskIndex};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{ImportTarget, Priority, TaskImportReport, TaskInput, TaskStatus};
use crate::util;
use crate::vault::Vault;

#[derive(Debug, Deserialize)]
struct TwTask {
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: String,
    entry: Option<String>,
    end: Option<String>,
    due: Option<String>,
    scheduled: Option<String>,
    wait: Option<String>,
    start: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    annotations: Vec<TwAnnotation>,
    /// A comma-separated string in Taskwarrior 2, an array in 3.
    depends: Option<serde_json::Value>,
    recur: Option<String>,
    parent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwAnnotation {
    entry: Option<String>,
    #[serde(default)]
    description: String,
}

/// Taskwarrior dates are UTC (`20261020T120000Z`); a task's day is the local one.
fn tw_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let utc = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ"))
        .ok()?;
    Some(
        Utc.from_utc_datetime(&utc)
            .with_timezone(&Local)
            .date_naive(),
    )
}

/// Taskwarrior's recurrence periods as `🔁` rules.
pub fn tw_recur(s: &str) -> Option<String> {
    let s = s.trim().to_lowercase();
    let named = match s.as_str() {
        "daily" | "day" => Some("every day"),
        "weekly" | "week" => Some("every week"),
        "weekdays" => Some("every weekday"),
        "biweekly" | "fortnight" => Some("every 2 weeks"),
        "monthly" | "month" => Some("every month"),
        "bimonthly" => Some("every 2 months"),
        "quarterly" => Some("every 3 months"),
        "semiannual" => Some("every 6 months"),
        "annual" | "yearly" | "year" => Some("every year"),
        "biannual" | "biyearly" => Some("every 2 years"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    // ISO 8601 periods (P1W, P3D, P2M, P1Y) and the short forms (3d, 2w, 2wk, 6mo, 1y)
    let body = s.strip_prefix('p').unwrap_or(&s);
    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u32 = if digits.is_empty() {
        1
    } else {
        digits.parse().ok()?
    };
    let unit = match &body[digits.len()..] {
        "d" | "day" | "days" => "day",
        "w" | "wk" | "wks" | "week" | "weeks" => "week",
        "m" | "mo" | "mos" | "month" | "months" => "month",
        "y" | "yr" | "yrs" | "year" | "years" => "year",
        _ => return None,
    };
    Some(if n == 1 {
        format!("every {unit}")
    } else {
        format!("every {n} {unit}s")
    })
}

fn depends_of(v: &Option<serde_json::Value>) -> Vec<String> {
    match v {
        Some(serde_json::Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).collect(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

/// A project's note: an existing project note by name, or the one the import creates.
fn project_rel(vault: &Vault, project: &str) -> (String, bool) {
    if let Some(rel) = find_project_note(vault, project) {
        return (rel, false);
    }
    let name: String = project
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches(['.', ' ']).to_string();
    let name = if name.is_empty() {
        util::slug(project, 60)
    } else {
        name
    };
    let rel = format!("{PROJECTS_DIR}/{name}.md");
    let exists = vault.root.join(&rel).exists();
    (rel, !exists)
}

fn project_note(title: &str, today: &str) -> String {
    frontmatter::render(&[
        ("title", title.to_string()),
        ("type", "project".into()),
        ("status", "active".into()),
        ("created", today.into()),
        ("updated", today.into()),
        ("author", "hq".into()),
    ]) + &format!("# {title}\n\n## Tasks\n")
}

/// Import an export into `vault`. With `dry_run` nothing is written.
pub fn import(
    vault: &Vault,
    index: &mut TaskIndex,
    json: &str,
    dry_run: bool,
    today: NaiveDate,
) -> Result<TaskImportReport> {
    let tasks: Vec<TwTask> = serde_json::from_str(json)
        .map_err(|e| Error::Invalid(format!("That is not Taskwarrior export JSON: {e}")))?;
    let today_s = fmt(today);
    let mut report = TaskImportReport {
        dry_run,
        total: tasks.len() as u32,
        ..Default::default()
    };

    // templates whose pending instance is in the export carry their 🔁 through it
    let instanced: HashSet<&str> = tasks
        .iter()
        .filter(|t| t.status == "pending" || t.status == "waiting")
        .filter_map(|t| t.parent.as_deref())
        .collect();
    let existing: HashSet<(String, Option<String>)> = index
        .tasks()
        .filter(|t| t.vault == vault.id)
        .map(|t| (t.description.clone(), t.created.clone()))
        .collect();

    let mut chosen: Vec<&TwTask> = Vec::new();
    for t in &tasks {
        match t.status.as_str() {
            "deleted" => report.deleted += 1,
            "recurring" => {
                report.templates += 1;
                if !instanced.contains(t.uuid.as_str()) {
                    chosen.push(t);
                }
            }
            "completed" => {
                report.completed += 1;
                chosen.push(t);
            }
            "waiting" => {
                report.waiting += 1;
                chosen.push(t);
            }
            "pending" => {
                if t.wait
                    .as_deref()
                    .and_then(tw_date)
                    .is_some_and(|w| w > today)
                {
                    report.waiting += 1;
                } else {
                    report.pending += 1;
                }
                chosen.push(t);
            }
            other => report.warnings.push(format!(
                "Skipped \"{}\": unknown status \"{other}\".",
                t.description
            )),
        }
    }
    chosen.sort_by_key(|t| t.entry.as_deref().and_then(tw_date));

    let mut used = index.ids(&vault.id);
    let ids: HashMap<&str, String> = chosen
        .iter()
        .map(|t| {
            let id = loop {
                let id = util::task_id();
                if used.insert(id.clone()) {
                    break id;
                }
            };
            (t.uuid.as_str(), id)
        })
        .collect();

    // note → (create?, lines), in first-seen order
    let mut by_note: BTreeMap<String, (bool, Option<String>, Vec<String>)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut seen_desc: HashSet<(String, Option<String>)> = HashSet::new();
    for t in chosen {
        let desc = t
            .description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if desc.is_empty() {
            report
                .warnings
                .push(format!("Skipped task {} with no description.", t.uuid));
            continue;
        }
        let created = t.entry.as_deref().and_then(tw_date).map(fmt);
        // what the description reads as once written (tags in it are pulled out)
        let probe = parse::parse_line(&format!("- [ ] {desc}"))
            .map(|l| l.description())
            .unwrap_or_else(|| desc.clone());
        let key = (probe, created.clone());
        if existing.contains(&key) || !seen_desc.insert(key) {
            report.duplicates += 1;
            continue;
        }
        let wait = t.wait.as_deref().and_then(tw_date).filter(|w| *w > today);
        let waiting = t.status == "waiting" || (t.status == "pending" && wait.is_some());
        let recur = match t.recur.as_deref() {
            Some(r) => match tw_recur(r) {
                Some(rule) => Some(rule),
                None => {
                    report.warnings.push(format!(
                        "\"{desc}\": the repeat \"{r}\" has no 🔁 equivalent, so it was left off."
                    ));
                    None
                }
            },
            None => None,
        };
        let priority = match t.priority.as_deref() {
            Some("H") => Some(Priority::High),
            Some("M") => Some(Priority::Medium),
            Some("L") => Some(Priority::Low),
            _ => None,
        };
        let deps: Vec<String> = depends_of(&t.depends)
            .iter()
            .filter_map(|u| ids.get(u.as_str()).cloned())
            .collect();
        let input = TaskInput {
            description: desc.clone(),
            tags: t
                .tags
                .iter()
                .map(|x| clean_tag(x))
                .filter(|x| !x.is_empty())
                .collect(),
            priority,
            start: t.start.as_deref().and_then(tw_date).map(fmt),
            scheduled: if waiting {
                wait.or_else(|| t.wait.as_deref().and_then(tw_date))
                    .map(fmt)
            } else {
                t.scheduled.as_deref().and_then(tw_date).map(fmt)
            },
            due: t.due.as_deref().and_then(tw_date).map(fmt),
            recur,
            depends: deps,
            waiting,
            ..Default::default()
        };
        let project = t
            .project
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let (rel, create, title) = match project {
            Some(p) => {
                let (rel, create) = project_rel(vault, p);
                (rel, create, Some(p.to_string()))
            }
            None => (LOOSE_TASKS.to_string(), false, None),
        };
        let id = &ids[t.uuid.as_str()];
        let line = render_new(&input, id, &today_s, None)?;
        let mut line = parse::parse_line(&line)
            .ok_or_else(|| Error::Internal("could not read back an imported line".into()))?;
        line.set(FieldKind::Created, created.as_deref());
        let end = t.end.as_deref().and_then(tw_date).map(fmt);
        match t.status.as_str() {
            "completed" => {
                line.set_status(TaskStatus::Done);
                line.set(FieldKind::Done, Some(end.as_deref().unwrap_or(&today_s)));
            }
            _ if t.start.is_some() && !waiting => line.set_status(TaskStatus::Started),
            _ => {}
        }
        let mut lines = vec![line.print()];
        for a in &t.annotations {
            let text = a
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                continue;
            }
            match a.entry.as_deref().and_then(tw_date) {
                Some(d) => lines.push(format!("    - {text} ({})", fmt(d))),
                None => lines.push(format!("    - {text}")),
            }
        }
        if report.sample.len() < 10 {
            report.sample.push(lines[0].clone());
        }
        report.written += 1;
        let entry = by_note.entry(rel.clone()).or_insert_with(|| {
            order.push(rel.clone());
            (create, title, Vec::new())
        });
        entry.2.extend(lines);
    }

    report.targets = order
        .iter()
        .map(|rel| {
            let (create, _, lines) = &by_note[rel];
            ImportTarget {
                note: rel.clone(),
                tasks: lines.iter().filter(|l| !l.starts_with("    ")).count() as u32,
                create: *create || (rel == LOOSE_TASKS && !vault.root.join(rel).exists()),
            }
        })
        .collect();
    if dry_run {
        return Ok(report);
    }
    for rel in &order {
        let (create, title, lines) = &by_note[rel];
        if *create {
            let path = vault.resolve_rel(rel)?;
            let title = title.clone().unwrap_or_else(|| rel.clone());
            match locks::create_new(&path, &project_note(&title, &today_s)) {
                Ok(()) | Err(Error::Invalid(_)) => {}
                Err(e) => return Err(e),
            }
        }
        append_lines(vault, rel, lines, &today_s)?;
        index.refresh_note(vault, rel);
    }
    Ok(report)
}

/// Where the `task` program is, if it is installed.
pub fn find_task_binary() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![
        "/opt/homebrew/bin/task".into(),
        "/usr/local/bin/task".into(),
        "/usr/bin/task".into(),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join("task")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Run `task export` read-only and return its JSON.
pub fn export_json(bin: &Path) -> Result<String> {
    let out = Command::new(bin)
        .args([
            "rc.hooks=off",
            "rc.gc=off",
            "rc.recurrence=off",
            "rc.confirmation=off",
            "rc.verbose=nothing",
            "export",
        ])
        .output()
        .map_err(|e| Error::Unavailable(format!("Could not run {}: {e}", bin.display())))?;
    if !out.status.success() {
        return Err(Error::Unavailable(format!(
            "task export failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    String::from_utf8(out.stdout)
        .map_err(|_| Error::Invalid("task export did not print UTF-8.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::date;
    use crate::tasks::tests::vault_with;

    const EXPORT: &str = r#"[
      {"uuid":"u-dig","description":"Dig the bed","status":"pending","entry":"20261001T120000Z",
       "project":"garden","priority":"H","due":"20261105T120000Z","tags":["outdoor","weekend"],
       "annotations":[{"entry":"20261002T120000Z","description":"spade in the shed"}]},
      {"uuid":"u-plant","description":"Plant bulbs","status":"pending","entry":"20261002T120000Z",
       "project":"garden","depends":"u-dig"},
      {"uuid":"u-read","description":"Read chapter 7","status":"pending","entry":"20261003T120000Z",
       "start":"20261030T120000Z","priority":"M"},
      {"uuid":"u-wait","description":"Call about parts","status":"waiting","entry":"20261004T120000Z",
       "wait":"20261201T120000Z"},
      {"uuid":"u-wait3","description":"TW3 style wait","status":"pending","entry":"20261004T120000Z",
       "wait":"20261210T120000Z","depends":["u-dig","u-gone"]},
      {"uuid":"u-done","description":"Measure beds","status":"completed","entry":"20260901T120000Z",
       "end":"20260920T120000Z","project":"garden"},
      {"uuid":"u-del","description":"Oops","status":"deleted","entry":"20260901T120000Z"},
      {"uuid":"u-tpl","description":"Water plants","status":"recurring","recur":"weekly",
       "entry":"20261001T120000Z","due":"20261101T120000Z"},
      {"uuid":"u-inst","description":"Water plants","status":"pending","recur":"weekly",
       "parent":"u-tpl","entry":"20261001T120000Z","due":"20261101T120000Z"},
      {"uuid":"u-tpl2","description":"Pay rent","status":"recurring","recur":"monthly",
       "entry":"20260101T120000Z","due":"20261201T120000Z","project":"Home Admin"},
      {"uuid":"u-odd","description":"Odd repeat","status":"pending","recur":"every blue moon",
       "entry":"20261005T120000Z"}
    ]"#;

    fn today() -> NaiveDate {
        date("2026-11-02").unwrap()
    }

    #[test]
    fn dry_run_reports_and_writes_nothing() {
        let (_d, v) = vault_with(&[(
            "01-Projects/garden.md",
            "---\ntitle: Garden\ntype: project\n---\n\n## Tasks\n- [ ] Existing\n",
        )]);
        let before = std::fs::read_to_string(v.root.join("01-Projects/garden.md")).unwrap();
        let mut idx = TaskIndex::build(std::slice::from_ref(&v));
        let r = import(&v, &mut idx, EXPORT, true, today()).unwrap();
        assert!(r.dry_run);
        assert_eq!(r.total, 11);
        assert_eq!(
            (r.pending, r.waiting, r.completed, r.deleted, r.templates),
            (5, 2, 1, 1, 2)
        );
        assert_eq!(
            r.written, 9,
            "everything but the deleted task and the instanced template"
        );
        assert_eq!(r.duplicates, 0);
        let notes: Vec<_> = r
            .targets
            .iter()
            .map(|t| (t.note.as_str(), t.tasks, t.create))
            .collect();
        assert!(notes.contains(&("01-Projects/garden.md", 3, false)));
        assert!(notes.contains(&("01-Projects/Home Admin.md", 1, true)));
        assert!(notes.contains(&(LOOSE_TASKS, 5, true)));
        assert_eq!(r.sample.len(), 9);
        assert!(r.warnings.iter().any(|w| w.contains("every blue moon")));
        assert_eq!(
            std::fs::read_to_string(v.root.join("01-Projects/garden.md")).unwrap(),
            before
        );
        assert!(!v.root.join(LOOSE_TASKS).exists());
    }

    #[test]
    fn import_writes_maps_fields_and_is_idempotent() {
        let (_d, v) = vault_with(&[(
            "01-Projects/garden.md",
            "---\ntitle: Garden\ntype: project\n---\n\n## Tasks\n- [ ] Existing\n",
        )]);
        let mut idx = TaskIndex::build(std::slice::from_ref(&v));
        let r = import(&v, &mut idx, EXPORT, false, today()).unwrap();
        assert_eq!(r.written, 9);
        let all = idx.finished(today());
        let get = |d: &str| {
            all.iter()
                .find(|t| t.description == d)
                .unwrap_or_else(|| panic!("{d}"))
        };

        let dig = get("Dig the bed");
        assert_eq!(dig.project.as_deref(), Some("garden"));
        assert_eq!(dig.priority, Some(Priority::High));
        assert_eq!(dig.created.as_deref(), Some("2026-10-01"));
        assert_eq!(dig.due.as_deref(), Some("2026-11-05"));
        assert_eq!(dig.tags, vec!["outdoor", "weekend"]);
        assert_eq!(dig.annotations[0].text, "spade in the shed");
        assert_eq!(dig.annotations[0].date.as_deref(), Some("2026-10-02"));
        assert!(dig.blocking);
        let plant = get("Plant bulbs");
        assert_eq!(plant.depends, vec![dig.id.clone().unwrap()]);
        assert!(plant.blocked);
        assert_eq!(get("Read chapter 7").status, TaskStatus::Started);
        assert_eq!(get("Read chapter 7").start.as_deref(), Some("2026-10-30"));
        let wait = get("Call about parts");
        assert_eq!(wait.status, TaskStatus::Waiting);
        assert_eq!(wait.scheduled.as_deref(), Some("2026-12-01"));
        assert!(wait.hidden);
        let w3 = get("TW3 style wait");
        assert_eq!(w3.status, TaskStatus::Waiting);
        assert_eq!(w3.depends.len(), 1, "unknown uuids are dropped");
        let done = get("Measure beds");
        assert_eq!(done.status, TaskStatus::Done);
        assert_eq!(done.done.as_deref(), Some("2026-09-20"));
        let water: Vec<_> = all
            .iter()
            .filter(|t| t.description == "Water plants")
            .collect();
        assert_eq!(water.len(), 1);
        assert_eq!(water[0].recur.as_deref(), Some("every week"));
        let rent = get("Pay rent");
        assert_eq!(rent.recur.as_deref(), Some("every month"));
        assert_eq!(rent.project.as_deref(), Some("Home Admin"));
        assert_eq!(rent.note_title, "Home Admin");
        assert_eq!(get("Odd repeat").recur, None);

        let garden = std::fs::read_to_string(v.root.join("01-Projects/garden.md")).unwrap();
        assert!(garden.contains(
            "## Tasks\n- [ ] Existing\n- [x] Measure beds ➕ 2026-09-01 ✅ 2026-09-20 🆔 "
        ));
        let admin = std::fs::read_to_string(v.root.join("01-Projects/Home Admin.md")).unwrap();
        assert!(admin.starts_with("---\ntitle: Home Admin\ntype: project\nstatus: active\n"));

        // a second run finds everything already there
        let again = import(&v, &mut idx, EXPORT, false, today()).unwrap();
        assert_eq!(again.written, 0);
        assert_eq!(again.duplicates, 9);
        assert_eq!(idx.finished(today()).len(), all.len());
    }

    #[test]
    fn bad_input_and_recur_mapping() {
        let (_d, v) = vault_with(&[]);
        let mut idx = TaskIndex::default();
        assert!(matches!(
            import(&v, &mut idx, "{not json", true, today()),
            Err(Error::Invalid(_))
        ));
        let r = import(
            &v,
            &mut idx,
            r#"[{"uuid":"x","description":"  ","status":"pending"}]"#,
            true,
            today(),
        )
        .unwrap();
        assert_eq!(r.written, 0);
        assert_eq!(r.warnings.len(), 1);
        for (tw, ours) in [
            ("daily", "every day"),
            ("weekdays", "every weekday"),
            ("P1W", "every week"),
            ("3d", "every 3 days"),
            ("2wk", "every 2 weeks"),
            ("6mo", "every 6 months"),
            ("quarterly", "every 3 months"),
            ("P1Y", "every year"),
        ] {
            assert_eq!(tw_recur(tw).as_deref(), Some(ours), "{tw}");
            assert!(crate::tasks::recur::parse_rule(ours).is_ok());
        }
        assert_eq!(tw_recur("sometimes"), None);
        assert_eq!(
            tw_date("20261020T120000Z"),
            Some(date("2026-10-20").unwrap())
        );
        assert_eq!(
            tw_date("2026-10-20T12:00:00Z"),
            Some(date("2026-10-20").unwrap())
        );
        assert_eq!(tw_date("soon"), None);
    }
}
