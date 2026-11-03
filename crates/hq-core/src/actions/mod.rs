//! Action items from Ask and quick capture: "add a task to build the new feature for the
//! Katha app tomorrow at 5PM", "remind me to call the bank Friday 9am".
//!
//! 1. `detect`: cheap rules say whether the text is an action ("add a task", "remind me",
//!    "todo:" …), might be one (an imperative with a date), or is not.
//! 2. `extract` (local model, structured output) reads description, day, time, project,
//!    tags, priority. `when` (no model) reads the day and time too; it wins where they
//!    disagree, and stands alone when there is no model.
//! 3. `link` files it: a project note named in the text gets it in `## Tasks`, otherwise
//!    the primary vault's `09-Planning/Tasks.md`; related notes and bookmarks are linked.
//! 4. The result is an `ActionDraft` the user edits and confirms. Nothing here writes.

pub mod extract;
pub mod link;
pub mod when;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use crate::error::{Error, Result};
use crate::ollama::JsonChat;
use crate::search::{Index, Semantic};
use crate::tasks::model::clean_tag;
use crate::tasks::mutate::{check_time, LOOSE_TASKS};
use crate::types::{note_id, split_note_id, ActionDraft, Priority, TaskInput};
use crate::vault::Vault;

use extract::Extracted;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Said so: "add a task", "remind me", "todo:".
    Action,
    /// An imperative with a day or time ("call the bank Friday"); the model decides.
    Maybe,
    No,
}

/// Politeness and addressing that come before the request itself.
const LEAD_INS: &[&str] = &[
    "please ",
    "pls ",
    "hey hq, ",
    "hey hq ",
    "hq, ",
    "can you ",
    "could you ",
    "would you ",
    "can u ",
    "ok ",
    "okay ",
];

/// Phrases that make a text an action item, longest first within a family.
const TRIGGERS: &[&str] = &[
    "add a task to ",
    "add a task for ",
    "add a task: ",
    "add a task ",
    "add task: ",
    "add task to ",
    "add task ",
    "add a todo to ",
    "add a todo: ",
    "add a todo ",
    "add todo: ",
    "add todo ",
    "add to my tasks: ",
    "add to my tasks ",
    "add to my todo list: ",
    "add to my to-do list: ",
    "add to my todo list ",
    "create a task to ",
    "create a task: ",
    "create a task ",
    "create task: ",
    "create task ",
    "new task: ",
    "new task ",
    "make a task to ",
    "make a task ",
    "task: ",
    "todo: ",
    "to do: ",
    "to-do: ",
    "todo ",
    "remind me to ",
    "remind me about ",
    "remind me that ",
    "remind me ",
    "reminder: ",
    "reminder to ",
    "set a reminder to ",
    "set a reminder for ",
    "set a reminder ",
    "don't let me forget to ",
    "dont let me forget to ",
    "i need to remember to ",
];

const QUESTION_WORDS: &[&str] = &[
    "what",
    "who",
    "when",
    "where",
    "why",
    "how",
    "which",
    "did",
    "do",
    "does",
    "is",
    "are",
    "was",
    "were",
    "should",
    "could",
    "would",
    "can",
    "have",
    "has",
    "will",
    "tell",
    "show",
    "summarize",
    "summarise",
    "list",
    "find",
    "explain",
];

/// Verbs that open a to-do when a day or time follows ("call", "buy", "send" …).
const IMPERATIVES: &[&str] = &[
    "call", "email", "mail", "text", "message", "ping", "buy", "order", "book", "pay", "send",
    "finish", "submit", "schedule", "renew", "cancel", "fix", "build", "write", "review", "check",
    "follow", "prepare", "clean", "pick", "drop", "return", "visit", "meet", "ship", "deploy",
    "update", "file", "water", "feed", "take", "bring", "get", "go", "start", "plan", "draft",
    "read", "watch", "practice", "practise", "study", "remember",
];

/// Whether `text` asks for a task, and how many bytes of it were the asking.
pub fn detect(text: &str, now: NaiveDateTime) -> (Intent, usize, bool) {
    let lower = text.trim_start().to_lowercase();
    let offset = text.len() - text.trim_start().len();
    // lowercasing kept the length for ASCII lead-ins; others have no trigger to strip
    let mut at = 0;
    while let Some(l) = LEAD_INS.iter().find(|l| lower[at..].starts_with(**l)) {
        at += l.len();
    }
    if let Some(t) = TRIGGERS.iter().find(|t| lower[at..].starts_with(**t)) {
        let reminder = t.contains("remind");
        let cut = offset + at + t.len();
        // only if the bytes line up with the original (ASCII prefix)
        if text.is_char_boundary(cut) && text[..cut].is_ascii() {
            return (Intent::Action, cut, reminder);
        }
    }
    let rest = lower[at..].trim();
    let first = rest.split_whitespace().next().unwrap_or("");
    if rest.ends_with('?') || QUESTION_WORDS.contains(&first) {
        return (Intent::No, 0, false);
    }
    let w = when::parse(text, now);
    if IMPERATIVES.contains(&first) && (w.date.is_some() || w.time.is_some()) {
        return (Intent::Maybe, 0, false);
    }
    (Intent::No, 0, false)
}

/// What the rules alone read from the text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleRead {
    pub description: String,
    pub due: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub tags: Vec<String>,
    pub priority: Option<Priority>,
    pub reminder: bool,
}

const PRIORITY_WORDS: &[(&str, Priority)] = &[
    ("highest priority", Priority::Highest),
    ("high priority", Priority::High),
    ("top priority", Priority::Highest),
    ("medium priority", Priority::Medium),
    ("low priority", Priority::Low),
    ("lowest priority", Priority::Lowest),
    ("urgently", Priority::High),
    ("urgent", Priority::High),
    ("asap", Priority::High),
    ("important", Priority::High),
];

/// Read description, day, time, `#tags` and priority words without a model.
pub fn read_rules(text: &str, skip: usize, reminder: bool, now: NaiveDateTime) -> RuleRead {
    let body = &text[skip.min(text.len())..];
    let w = when::parse(body, now);
    let mut desc = w.strip(body);
    let mut tags = Vec::new();
    let mut kept = Vec::new();
    for word in desc.split_whitespace() {
        match word.strip_prefix('#') {
            Some(t) if !clean_tag(t).is_empty() && !t.chars().all(|c| c.is_ascii_digit()) => {
                tags.push(clean_tag(t).to_lowercase())
            }
            _ => kept.push(word),
        }
    }
    desc = kept.join(" ");
    let mut priority = None;
    let lower = desc.to_lowercase();
    for (w, p) in PRIORITY_WORDS {
        if let Some(i) = lower.find(w) {
            let boundary = |j: usize| {
                lower[..j]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphanumeric())
            };
            let end = i + w.len();
            let after_ok = lower[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            if boundary(i) && after_ok {
                priority = Some(*p);
                desc = format!("{}{}", &desc[..i], &desc[end..]);
                break;
            }
        }
    }
    let mut description = when::tidy(desc.trim_start_matches([',', ':', '-', ' ']));
    if description.starts_with(|c: char| c.is_lowercase()) {
        let mut cs = description.chars();
        if let Some(f) = cs.next() {
            description = f.to_uppercase().chain(cs).collect();
        }
    }
    RuleRead {
        description,
        due: w.date,
        time: w.time,
        tags,
        priority,
        reminder,
    }
}

fn priority_from(s: &str) -> Option<Priority> {
    Some(match s.trim().to_lowercase().as_str() {
        "highest" => Priority::Highest,
        "high" => Priority::High,
        "medium" => Priority::Medium,
        "low" => Priority::Low,
        "lowest" => Priority::Lowest,
        _ => return None,
    })
}

/// The fields of a draft, from the rules and (if it answered) the model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reconciled {
    pub is_action: bool,
    pub description: String,
    pub due: Option<String>,
    pub time: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<Priority>,
    pub reminder: bool,
    pub project_hint: Option<String>,
    pub notes: Vec<String>,
}

/// Check the model against the rules: dates and times the rules found win; the model's
/// may fill gaps, but never a day in the past or a malformed value.
pub fn reconcile(
    intent: Intent,
    rules: &RuleRead,
    model: Option<&Extracted>,
    now: NaiveDateTime,
) -> Reconciled {
    let today = now.date();
    let fmt_d = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let fmt_t = |t: NaiveTime| t.format("%H:%M").to_string();
    let mut r = Reconciled {
        is_action: intent == Intent::Action,
        description: rules.description.clone(),
        due: rules.due.map(fmt_d),
        time: rules.time.map(fmt_t),
        tags: rules.tags.clone(),
        priority: rules.priority,
        reminder: rules.reminder,
        ..Default::default()
    };
    let Some(m) = model else {
        return r;
    };
    if intent == Intent::Maybe {
        r.is_action = m.is_action;
    }
    let d = m.description.trim();
    if !d.is_empty() && d.chars().count() <= 200 && !d.contains('\n') {
        r.description = d.to_string();
    }
    let model_date = m
        .due_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok());
    match (rules.due, model_date) {
        (Some(a), Some(b)) if a != b => r.notes.push(format!(
            "The model read the day as {}; HQ's reading, {}, is used.",
            fmt_d(b),
            fmt_d(a)
        )),
        (None, Some(b)) if b < today => r.notes.push(format!(
            "The model gave a day in the past ({}); it was left out.",
            fmt_d(b)
        )),
        (None, Some(b)) => {
            r.due = Some(fmt_d(b));
            r.notes
                .push("The day is the model's reading; check it.".into());
        }
        _ => {}
    }
    let model_time = m.due_time.as_deref().and_then(|t| check_time(t).ok());
    match (rules.time, &model_time) {
        (Some(a), Some(b)) if fmt_t(a) != *b => r.notes.push(format!(
            "The model read the time as {b}; HQ's reading, {}, is used.",
            fmt_t(a)
        )),
        (None, Some(b)) => r.time = Some(b.clone()),
        _ => {}
    }
    if r.priority.is_none() {
        r.priority = m.priority.as_deref().and_then(priority_from);
    }
    for t in &m.tags {
        let t = clean_tag(t).to_lowercase();
        if !t.is_empty() && !r.tags.contains(&t) && r.tags.len() < 5 {
            r.tags.push(t);
        }
    }
    r.reminder |= m.reminder;
    r.project_hint = m
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from);
    r
}

/// The local date and time: what relative dates are resolved against.
pub fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// Quick answer for text that is clearly not an action, before any vault is read.
pub fn obviously_not(text: &str, force: bool, now: NaiveDateTime) -> Option<ActionDraft> {
    let t = text.trim();
    if force || t.is_empty() {
        return None;
    }
    (t.contains('\n') || detect(t, now).0 == Intent::No).then(|| not_action(t))
}

/// Default hour for a reminder given a day but no time.
pub const REMINDER_HOUR: &str = "09:00";

pub struct DraftCtx<'a> {
    pub index: &'a Index,
    pub semantic: Option<Semantic<'a>>,
    pub model: Option<&'a dyn JsonChat>,
    /// Where an action with no project note goes.
    pub primary: Option<&'a Vault>,
    /// Vault ids to look for projects and related notes in (empty: all).
    pub vaults: Vec<String>,
    pub now: NaiveDateTime,
}

/// Read `text` as an action item. `force` (quick capture's ⌘Enter) treats it as one
/// whatever the rules say. Never writes.
pub fn draft(text: &str, force: bool, ctx: DraftCtx<'_>) -> Result<ActionDraft> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Invalid("Write the task first.".into()));
    }
    if text.contains('\n') && !force {
        return Ok(not_action(text));
    }
    let (mut intent, skip, reminder) = detect(text, ctx.now);
    if force && intent != Intent::Action {
        intent = Intent::Action;
    }
    if intent == Intent::No {
        return Ok(not_action(text));
    }
    let rules = read_rules(text, skip, reminder, ctx.now);
    let mut notes = Vec::new();
    let model = match ctx.model {
        Some(m) => match extract::extract(m, text, ctx.now) {
            Ok(e) => Some(e),
            Err(e) => {
                notes.push(format!(
                    "The local model could not read it ({e}); HQ's own rules did."
                ));
                None
            }
        },
        None => None,
    };
    let source = if model.is_some() { "model" } else { "rules" };
    let mut r = reconcile(intent, &rules, model.as_ref(), ctx.now);
    if !r.is_action {
        return Ok(not_action(text));
    }
    notes.append(&mut r.notes);
    if r.description.is_empty() {
        r.description = when::tidy(&text[skip.min(text.len())..]);
    }
    if r.reminder && r.time.is_none() {
        if r.due.is_some() {
            r.time = Some(REMINDER_HOUR.into());
            notes.push(format!(
                "No time was given, so the reminder is at {REMINDER_HOUR}."
            ));
        } else {
            notes.push("Pick a day and time for the reminder.".into());
        }
    }
    let today = ctx.now.date().format("%Y-%m-%d").to_string();
    let linked = link::link(
        ctx.index,
        text,
        &r.description,
        r.project_hint.as_deref(),
        &ctx.vaults,
        ctx.semantic,
        &today,
    )?;
    let (note, note_title, vault, project) = match &linked.project {
        Some(p) => (Some(p.id()), p.title.clone(), Some(p.vault.clone()), None),
        None => {
            let v = ctx.primary;
            (
                v.map(|v| note_id(&v.id, LOOSE_TASKS)),
                "Tasks".to_string(),
                v.map(|v| v.id.clone()),
                r.project_hint.clone(),
            )
        }
    };
    let suggested_tags = linked
        .suggested_tags
        .into_iter()
        .filter(|t| !r.tags.contains(t))
        .collect();
    Ok(ActionDraft {
        is_action: true,
        text: text.to_string(),
        description: r.description,
        due: r.due,
        due_time: r.time,
        reminder: r.reminder,
        priority: r.priority,
        tags: r.tags,
        suggested_tags,
        project,
        note,
        note_title,
        vault,
        related: linked.related,
        source: source.into(),
        notes,
    })
}

fn not_action(text: &str) -> ActionDraft {
    ActionDraft {
        text: text.to_string(),
        ..Default::default()
    }
}

/// The task to add for a confirmed draft. The loose task list is named by vault, not by
/// note, so it is created on first use.
pub fn to_input(d: &ActionDraft) -> Result<TaskInput> {
    if d.description.trim().is_empty() {
        return Err(Error::Invalid("A task needs a description.".into()));
    }
    let (note, vault) = match d.note.as_deref().and_then(split_note_id) {
        Some((v, rel)) if rel == LOOSE_TASKS => (None, Some(v.to_string())),
        Some(_) => (d.note.clone(), None),
        None => (None, d.vault.clone()),
    };
    Ok(TaskInput {
        description: d.description.trim().to_string(),
        tags: d.tags.clone(),
        project: d.project.clone().filter(|p| !p.trim().is_empty()),
        note,
        vault,
        priority: d.priority,
        due: d.due.clone().filter(|s| !s.trim().is_empty()),
        due_time: d.due_time.clone().filter(|s| !s.trim().is_empty()),
        ..Default::default()
    })
}

/// `related: [[a]], [[b]]` for the ticked links, or None.
pub fn related_annotation(d: &ActionDraft) -> Option<String> {
    let mut stems: Vec<String> = Vec::new();
    for r in d.related.iter().filter(|r| r.include) {
        let link = format!("[[{}]]", r.stem.replace([']', '[', '|'], ""));
        if !stems.contains(&link) {
            stems.push(link);
        }
    }
    (!stems.is_empty()).then(|| format!("related: {}", stems.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::tests::note;
    use crate::types::HitKind;
    use extract::tests::FakeModel;
    use serde_json::json;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 3)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap()
    }

    #[test]
    fn detection() {
        let cases = [
            (
                "Add a task to build new feature for the Katha app tomorrow at 5PM",
                Intent::Action,
            ),
            ("remind me to call the bank Friday 9am", Intent::Action),
            ("please remind me to water plants", Intent::Action),
            ("Can you add a todo: renew passport", Intent::Action),
            ("todo: buy milk", Intent::Action),
            ("call the bank Friday 9am", Intent::Maybe),
            ("call the bank", Intent::No),
            ("what did I decide about the garden?", Intent::No),
            ("when is the dentist tomorrow", Intent::No),
            ("the fence looks great today", Intent::No),
        ];
        for (t, want) in cases {
            assert_eq!(detect(t, now()).0, want, "{t}");
        }
        let (_, skip, rem) = detect("Please remind me to call", now());
        assert_eq!(&"Please remind me to call"[skip..], "call");
        assert!(rem);
    }

    #[test]
    fn rules_alone() {
        let t = "Add a task to build new feature for the Katha app tomorrow at 5PM #dev urgent";
        let (_, skip, rem) = detect(t, now());
        let r = read_rules(t, skip, rem, now());
        assert_eq!(r.description, "Build new feature for the Katha app");
        assert_eq!(r.due, NaiveDate::from_ymd_opt(2026, 11, 4));
        assert_eq!(r.time, NaiveTime::from_hms_opt(17, 0, 0));
        assert_eq!(r.tags, vec!["dev"]);
        assert_eq!(r.priority, Some(Priority::High));
    }

    #[test]
    fn model_checked_against_rules() {
        let t = "remind me to call the bank Friday 9am";
        let (intent, skip, rem) = detect(t, now());
        let rules = read_rules(t, skip, rem, now());
        // the model miscounts the weekday and gets the time right
        let m = Extracted {
            is_action: true,
            description: "Call the bank".into(),
            due_date: Some("2026-11-07".into()),
            due_time: Some("09:00".into()),
            tags: vec!["Finance".into()],
            priority: Some("low".into()),
            reminder: true,
            ..Default::default()
        };
        let r = reconcile(intent, &rules, Some(&m), now());
        assert_eq!(r.due.as_deref(), Some("2026-11-06"));
        assert_eq!(r.time.as_deref(), Some("09:00"));
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.tags, vec!["finance"]);
        assert_eq!(r.priority, Some(Priority::Low));
        // a gap the rules left is filled, a past day is refused, a bad time ignored
        let rules = read_rules("call mum at the end of the month", 0, false, now());
        let m2 = Extracted {
            is_action: true,
            due_date: Some("2026-11-30".into()),
            due_time: Some("5pm".into()),
            ..Default::default()
        };
        let r = reconcile(Intent::Maybe, &rules, Some(&m2), now());
        assert_eq!(r.due.as_deref(), Some("2026-11-30"));
        assert_eq!(r.time, None);
        let m3 = Extracted {
            is_action: true,
            due_date: Some("2025-01-01".into()),
            ..Default::default()
        };
        assert_eq!(
            reconcile(Intent::Action, &rules, Some(&m3), now()).due,
            None
        );
        // the model says a "maybe" is not an action
        let no = Extracted::default();
        assert!(!reconcile(Intent::Maybe, &rules, Some(&no), now()).is_action);
    }

    /// A vault with a project, a note and a bookmark about it, and an unrelated note.
    fn fixture() -> (tempfile::TempDir, Vault, Index) {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let mut idx = Index::new();
        idx.update_vault(
            &v.id,
            &[
                note(
                    "01-Projects/katha-app.md",
                    &[("title", "Katha app"), ("type", "project"), ("tags", "[katha, mobile]")],
                    "## Plan\nThe Katha app tells stories; next is the reading-mode feature.\n\n## Tasks\n",
                ),
                note(
                    "05-References/katha-design.md",
                    &[("title", "Katha design notes"), ("tags", "[design, katha]")],
                    "Design notes for the Katha app: the reading mode, fonts and the new feature list.\n",
                ),
                note(
                    "10-Bookmarks/chrome/swiftui-reader.md",
                    &[("title", "SwiftUI reader layouts"), ("type", "bookmark"), ("url", "https://example.com/reader")],
                    "Building a reading feature in SwiftUI for a story app like Katha.\n",
                ),
                note(
                    "01-Projects/garden.md",
                    &[("title", "Garden"), ("type", "project")],
                    "## Irrigation\nLay the drip line along the fence before the frost.\n",
                ),
            ],
        );
        (dir, v, idx)
    }

    #[test]
    fn files_in_the_named_project_and_links_related() {
        let (_d, v, idx) = fixture();
        let m = FakeModel::new(json!({
            "is_action": true, "description": "Build new feature for the Katha app",
            "due_date": "2026-11-04", "due_time": "17:00", "project": "Katha app",
            "tags": [], "priority": null, "reminder": false
        }));
        let d = draft(
            "Add a task to build new feature for the Katha app tomorrow at 5PM",
            false,
            DraftCtx {
                index: &idx,
                // keywords only: the fake embedder's letter counts make everything look alike
                semantic: None,
                model: Some(&m),
                primary: Some(&v),
                vaults: vec![],
                now: now(),
            },
        )
        .unwrap();
        assert!(d.is_action);
        assert_eq!(d.source, "model");
        assert_eq!(
            d.note.as_deref(),
            Some(note_id(&v.id, "01-Projects/katha-app.md").as_str())
        );
        assert_eq!(d.note_title, "Katha app");
        assert_eq!(
            (d.due.as_deref(), d.due_time.as_deref()),
            (Some("2026-11-04"), Some("17:00"))
        );
        assert!(d.project.is_none(), "the note implies the project");
        let stems: Vec<&str> = d.related.iter().map(|r| r.stem.as_str()).collect();
        assert!(stems.contains(&"katha-design"), "{stems:?}");
        assert!(
            !stems.contains(&"katha-app"),
            "the project note itself is not 'related'"
        );
        assert!(!stems.contains(&"garden"));
        assert!(
            d.related.iter().any(|r| r.kind == HitKind::Bookmark),
            "{stems:?}"
        );
        assert!(d.suggested_tags.contains(&"katha".to_string()));
        assert!(d.suggested_tags.contains(&"mobile".to_string()));
        let input = to_input(&d).unwrap();
        assert_eq!(input.note, d.note);
        assert_eq!(input.due_time.as_deref(), Some("17:00"));
        let ann = related_annotation(&d).unwrap();
        assert!(ann.starts_with("related: [[katha-design]]"), "{ann}");
    }

    #[test]
    fn without_a_model_or_project_it_goes_to_the_loose_list() {
        let (_d, v, idx) = fixture();
        let d = draft(
            "remind me to call the bank Friday",
            false,
            DraftCtx {
                index: &idx,
                semantic: None,
                model: Some(&FakeModel::down()),
                primary: Some(&v),
                vaults: vec![],
                now: now(),
            },
        )
        .unwrap();
        assert_eq!(d.source, "rules");
        assert_eq!(d.description, "Call the bank");
        assert_eq!(d.due.as_deref(), Some("2026-11-06"));
        assert_eq!(
            d.due_time.as_deref(),
            Some("09:00"),
            "a reminder defaults to 9"
        );
        assert!(d.reminder);
        assert_eq!(d.note_title, "Tasks");
        let input = to_input(&d).unwrap();
        assert_eq!((input.note, input.vault), (None, Some(v.id.clone())));
        assert!(d.notes.iter().any(|n| n.contains("could not read")));
    }

    #[test]
    fn questions_are_not_actions_and_force_overrides() {
        let (_d, v, idx) = fixture();
        let mk = |text: &str, force: bool| {
            draft(
                text,
                force,
                DraftCtx {
                    index: &idx,
                    semantic: None,
                    model: None,
                    primary: Some(&v),
                    vaults: vec![],
                    now: now(),
                },
            )
            .unwrap()
        };
        assert!(!mk("what is next for the Katha app?", false).is_action);
        let forced = mk("water the garden beds at 6", true);
        assert!(forced.is_action);
        assert_eq!(forced.due_time.as_deref(), Some("18:00"));
        assert_eq!(forced.note_title, "Garden", "named by its title");
        assert!(draft(
            "  ",
            false,
            DraftCtx {
                index: &idx,
                semantic: None,
                model: None,
                primary: None,
                vaults: vec![],
                now: now()
            }
        )
        .is_err());
    }
}
