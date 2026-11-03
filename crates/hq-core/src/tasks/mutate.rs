//! Every change to a task (SPEC 4.2). Each one is a single `locks::guarded_edit` on the
//! containing note: the task is found again in the text read under the lock, by `🆔`
//! (or, for a line that has none yet, by its content key), never by line number. So an
//! edit made meanwhile by the user, an editor or an agent is never overwritten; if the
//! task itself changed or went away the edit is refused with a message to refresh.
//!
//! Every task HQ touches leaves with a `🆔` and a `➕`.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::NaiveDate;

use super::model::{clean_tag, FieldKind, TaskLine};
use super::parse::{self, Found};
use super::recur::{next_dates, parse_rule};
use super::{check_date, date, fmt, keys_for, split_key, Key, TaskIndex};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{note_id, split_note_id, Task, TaskInput, TaskPatch, TaskStatus};
use crate::util;
use crate::vault::{scan, Vault};

pub const LOOSE_TASKS: &str = "09-Planning/Tasks.md";
pub const PROJECTS_DIR: &str = "01-Projects";

/// What a mutation needs: the vaults, the index (updated in place after each write) and
/// today's date.
pub struct Ctx<'a> {
    pub vaults: &'a [Vault],
    pub index: &'a mut TaskIndex,
    pub today: NaiveDate,
}

impl Ctx<'_> {
    fn vault(&self, id: &str) -> Result<&Vault> {
        self.vaults
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| Error::NotFound(format!("No open vault has the id {id}.")))
    }

    fn today(&self) -> String {
        fmt(self.today)
    }

    /// The finished task with this `🆔` in this vault, after a write.
    fn task(&self, vault: &str, id: &str) -> Result<Task> {
        self.index
            .finished(self.today)
            .into_iter()
            .find(|t| t.vault == vault && t.id.as_deref() == Some(id))
            .ok_or_else(|| Error::Internal(format!("Task {id} was written but not read back.")))
    }
}

/// Where a key points, resolved against the index (rebuilt once if it is stale).
struct Target {
    vault: String,
    rel: String,
    path: PathBuf,
}

fn target(ctx: &mut Ctx, key: &str) -> Result<Target> {
    if let Key::Id { vault: None, id } = split_key(key) {
        let vs = ctx.index.vaults_with_id(id);
        if vs.len() > 1 {
            return Err(Error::Invalid(format!(
                "The task id {id} is used in more than one vault; name the vault as <vault>:{id}."
            )));
        }
    }
    let found = match ctx.index.locate(key) {
        Some(x) => Some(x),
        None => {
            *ctx.index = TaskIndex::build(ctx.vaults);
            ctx.index.locate(key)
        }
    };
    let (vault, rel) = found.ok_or_else(|| {
        Error::NotFound(format!("No task {key}. It may have been deleted or moved."))
    })?;
    let path = ctx.vault(&vault)?.resolve_rel(&rel)?;
    Ok(Target { vault, rel, path })
}

/// Which of the parsed tasks the key names.
fn find_in(found: &[Found], nid: &str, key: &str) -> Option<usize> {
    match split_key(key) {
        Key::Id { id, .. } => found
            .iter()
            .position(|f| f.task.get(FieldKind::Id) == Some(id)),
        Key::Content { .. } => keys_for(nid, found).iter().position(|k| k == key),
    }
}

/// Splits text into lines with their endings, so a replacement keeps CRLF files CRLF.
fn split_keep(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn ending_of(line: &str) -> &'static str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Replace lines `[a, b)` with `new` lines.
fn splice(text: &str, a: usize, b: usize, new: &[String]) -> String {
    let lines = split_keep(text);
    let nl = lines
        .get(a)
        .or_else(|| lines.last())
        .map(|l| ending_of(l))
        .unwrap_or("\n");
    let mut out = String::with_capacity(text.len() + 128);
    for l in &lines[..a.min(lines.len())] {
        out.push_str(l);
    }
    let last_had_newline = lines
        .get(b.saturating_sub(1))
        .is_none_or(|l| l.ends_with('\n'));
    let tail_empty = b >= lines.len();
    for (i, l) in new.iter().enumerate() {
        out.push_str(l);
        if i + 1 < new.len() || !tail_empty || last_had_newline {
            out.push_str(nl);
        }
    }
    for l in lines.iter().skip(b) {
        out.push_str(l);
    }
    out
}

/// A `🆔` not used anywhere in the vault yet.
fn fresh_id(used: &mut HashSet<String>) -> String {
    loop {
        let id = util::task_id();
        if used.insert(id.clone()) {
            return id;
        }
    }
}

fn used_ids(index: &TaskIndex, vault: &str, text: &str) -> HashSet<String> {
    let mut used = index.ids(vault);
    for f in parse::parse_note(text) {
        if let Some(id) = f.task.get(FieldKind::Id) {
            used.insert(id.to_string());
        }
    }
    used
}

/// Give a line its `🆔` and `➕` if it lacks them.
fn ensure_identity(t: &mut TaskLine, used: &mut HashSet<String>, today: &str) -> String {
    if t.get(FieldKind::Created).is_none() {
        t.set(FieldKind::Created, Some(today));
    }
    match t.get(FieldKind::Id) {
        Some(id) => id.to_string(),
        None => {
            let id = fresh_id(used);
            t.set(FieldKind::Id, Some(&id));
            id
        }
    }
}

/// What an edit does to the task's block of lines (the task and its annotations).
enum Outcome {
    /// The new block; `id` is the task to return.
    Lines {
        lines: Vec<String>,
        id: String,
    },
    Removed,
}

/// Find the task under the lock and let `f` rewrite its block. Returns the task's `🆔`
/// (`None` when it was removed).
fn edit(
    ctx: &mut Ctx,
    key: &str,
    f: impl FnOnce(&Found, &[String], &mut HashSet<String>) -> Result<Outcome>,
) -> Result<(String, Option<String>)> {
    let t = target(ctx, key)?;
    let nid = note_id(&t.vault, &t.rel);
    let today = ctx.today();
    let index: &TaskIndex = &*ctx.index;
    let mut result: Option<String> = None;
    locks::guarded_edit(&t.path, None, |cur| {
        let found = parse::parse_note(cur);
        let i = find_in(&found, &nid, key).ok_or_else(|| {
            Error::NotFound(format!(
                "The task {key} is no longer in {}. Refresh and try again.",
                t.rel
            ))
        })?;
        let all = parse::lines(cur);
        let block: Vec<String> = all[found[i].line..found[i].end]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut used = used_ids(index, &t.vault, cur);
        let (new, id) = match f(&found[i], &block, &mut used)? {
            Outcome::Lines { lines, id } => (lines, Some(id)),
            Outcome::Removed => (Vec::new(), None),
        };
        result = id;
        let next = splice(cur, found[i].line, found[i].end, &new);
        Ok(frontmatter::bump_updated(&next, &today))
    })?;
    let vault = ctx.vault(&t.vault)?.clone();
    ctx.index.refresh_note(&vault, &t.rel);
    Ok((t.vault, result))
}

/// Edit just the task line, keeping its annotations.
fn edit_line(
    ctx: &mut Ctx,
    key: &str,
    f: impl FnOnce(&mut TaskLine, &str) -> Result<()>,
) -> Result<Task> {
    let today = ctx.today();
    let (vault, id) = edit(ctx, key, |found, block, used| {
        let mut t = found.task.clone();
        let id = ensure_identity(&mut t, used, &today);
        f(&mut t, &today)?;
        let mut lines = vec![t.print()];
        lines.extend(block[1..].iter().cloned());
        Ok(Outcome::Lines { lines, id })
    })?;
    ctx.task(&vault, &id.expect("edited, not removed"))
}

fn one_line(field: &str, s: &str) -> Result<String> {
    let s = s.trim();
    if s.contains(['\n', '\r']) {
        return Err(Error::Invalid(format!("The {field} must be one line.")));
    }
    Ok(s.to_string())
}

fn check_ids(ids: &[String]) -> Result<Vec<String>> {
    ids.iter()
        .map(|i| i.trim().to_string())
        .filter(|i| !i.is_empty())
        .map(|i| {
            if i.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                Ok(i)
            } else {
                Err(Error::Invalid(format!("\"{i}\" is not a task id.")))
            }
        })
        .collect()
}

fn check_recur(r: &str) -> Result<String> {
    let r = one_line("repeat rule", r)?;
    parse_rule(&r)?;
    Ok(r)
}

// ---- adding ---------------------------------------------------------------------------

/// The line HQ writes for a new task.
pub fn render_new(
    input: &TaskInput,
    id: &str,
    today: &str,
    project_tag: Option<&str>,
) -> Result<String> {
    let desc = one_line("description", &input.description)?;
    if desc.is_empty() {
        return Err(Error::Invalid("A task needs a description.".into()));
    }
    let status = if input.waiting { '>' } else { ' ' };
    let mut t = parse::parse_line(&format!("- [{status}] {desc}"))
        .ok_or_else(|| Error::Internal("could not build a task line".into()))?;
    for tag in &input.tags {
        t.add_tag(tag);
    }
    if let Some(p) = project_tag {
        t.add_tag(&format!("project/{p}"));
    }
    t.set_priority(input.priority);
    t.set(FieldKind::Created, Some(today));
    for (kind, name, v) in [
        (FieldKind::Start, "start", &input.start),
        (FieldKind::Scheduled, "scheduled", &input.scheduled),
        (FieldKind::Due, "due", &input.due),
    ] {
        if let Some(v) = v.as_deref().filter(|v| !v.trim().is_empty()) {
            t.set(kind, Some(&check_date(name, v)?));
        }
    }
    if let Some(r) = input.recur.as_deref().filter(|r| !r.trim().is_empty()) {
        t.set(FieldKind::Recur, Some(&check_recur(r)?));
    }
    t.set(FieldKind::Id, Some(id));
    let deps = check_ids(&input.depends)?;
    if !deps.is_empty() {
        t.set(FieldKind::Depends, Some(&deps.join(",")));
    }
    Ok(t.print())
}

fn is_tasks_heading(line: &str) -> Option<usize> {
    let t = line.trim_end();
    let level = t.chars().take_while(|c| *c == '#').count();
    (level > 0 && t[level..].trim().eq_ignore_ascii_case("tasks")).then_some(level)
}

/// Insert lines into the note's `## Tasks` section (after its last non-blank line), or
/// make that section at the end; `09-Planning/Tasks.md` without one just gets them at the end.
pub fn insert_into_tasks_section(text: &str, new: &[String], loose: bool) -> String {
    let lines = parse::lines(text);
    let fm_lines = text[..frontmatter::parse(text).len].matches('\n').count();
    let heading = lines
        .iter()
        .enumerate()
        .skip(fm_lines)
        .find_map(|(i, l)| is_tasks_heading(l).map(|lv| (i, lv)));
    // a file that ends without a newline: add one so the new lines start on their own line
    let base = if !text.is_empty() && !text.ends_with('\n') {
        format!(
            "{text}{}",
            if text.contains("\r\n") { "\r\n" } else { "\n" }
        )
    } else {
        text.to_string()
    };
    let count = split_keep(&base).len();
    match heading {
        Some((h, level)) => {
            let end = lines
                .iter()
                .enumerate()
                .skip(h + 1)
                .find(|(_, l)| {
                    let n = l.chars().take_while(|c| *c == '#').count();
                    n > 0 && n <= level && l[n..].starts_with(' ')
                })
                .map(|(i, _)| i)
                .unwrap_or(count);
            let mut at = end;
            while at > h + 1 && lines.get(at - 1).is_some_and(|l| l.trim().is_empty()) {
                at -= 1;
            }
            // an empty section keeps the blank line under its heading
            if at == h + 1 && h + 2 <= end && lines.get(h + 1).is_some_and(|l| l.trim().is_empty())
            {
                at = h + 2;
            }
            splice(&base, at, at, new)
        }
        None => {
            let mut block = Vec::new();
            if !loose {
                if !base.trim().is_empty() && !base.ends_with("\n\n") {
                    block.push(String::new());
                }
                block.push("## Tasks".to_string());
            }
            block.extend(new.iter().cloned());
            splice(&base, count, count, &block)
        }
    }
}

/// A project note in this vault named `name` (by file stem or title, any case).
pub fn find_project_note(vault: &Vault, name: &str) -> Option<String> {
    let want = name.trim().to_lowercase();
    let notes = scan(vault).notes;
    let projects = notes.iter().filter(|n| {
        n.props.get("type").is_some_and(|t| t == "project") && super::in_scope(&n.rel, &n.props)
    });
    let mut by_title = None;
    for n in projects {
        if n.stem.to_lowercase() == want {
            return Some(n.rel.clone());
        }
        if by_title.is_none() && n.title().to_lowercase() == want {
            by_title = Some(n.rel.clone());
        }
    }
    by_title
}

fn loose_header(today: &str) -> String {
    frontmatter::render(&[
        ("title", "Tasks".into()),
        ("type", "index".into()),
        ("created", today.into()),
        ("updated", today.into()),
        ("author", "hq".into()),
    ]) + "# Tasks\n\n"
}

/// Append lines to a note's tasks section, creating `09-Planning/Tasks.md` if needed.
pub fn append_lines(vault: &Vault, rel: &str, lines: &[String], today: &str) -> Result<()> {
    let path = vault.resolve_rel(rel)?;
    let loose = rel == LOOSE_TASKS;
    if !path.exists() {
        if !loose {
            return Err(Error::NotFound(format!("No note {rel} in {}.", vault.name)));
        }
        match locks::create_new(&path, &loose_header(today)) {
            Ok(()) | Err(Error::Invalid(_)) => {} // created by someone else just now: fine
            Err(e) => return Err(e),
        }
    }
    locks::guarded_edit(&path, None, |cur| {
        let next = insert_into_tasks_section(cur, lines, loose);
        Ok(frontmatter::bump_updated(&next, today))
    })?;
    Ok(())
}

/// Which vault and note a new task goes to, and the `#project/` tag it needs (when its
/// project has no note).
fn destination<'v>(ctx: &'v Ctx, input: &TaskInput) -> Result<(&'v Vault, String, Option<String>)> {
    if let Some(n) = &input.note {
        let (v, rel) =
            split_note_id(n).ok_or_else(|| Error::Invalid(format!("\"{n}\" is not a note id.")))?;
        let vault = ctx.vault(v)?;
        if !vault.resolve_rel(rel)?.exists() {
            return Err(Error::NotFound(format!("No note {rel} in {}.", vault.name)));
        }
        let project = input
            .project
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        return Ok((vault, rel.to_string(), project.map(clean_tag)));
    }
    let vault = match &input.vault {
        Some(v) => ctx.vault(v)?,
        None => ctx
            .vaults
            .first()
            .ok_or_else(|| Error::Unavailable("No vault is open.".into()))?,
    };
    match input
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(p) => match find_project_note(vault, p) {
            Some(rel) => Ok((vault, rel, None)),
            None => Ok((vault, LOOSE_TASKS.into(), Some(clean_tag(p)))),
        },
        None => Ok((vault, LOOSE_TASKS.into(), None)),
    }
}

/// Add a task (SPEC 4.2): to a project note's `## Tasks` section, the note given, or
/// `09-Planning/Tasks.md`.
pub fn add(ctx: &mut Ctx, input: &TaskInput) -> Result<Task> {
    let today = ctx.today();
    let (vault, rel, project_tag) = destination(ctx, input)?;
    let vault = vault.clone();
    let mut used = ctx.index.ids(&vault.id);
    let id = fresh_id(&mut used);
    let line = render_new(input, &id, &today, project_tag.as_deref())?;
    append_lines(&vault, &rel, &[line], &today)?;
    ctx.index.refresh_note(&vault, &rel);
    ctx.task(&vault.id, &id)
}

// ---- changing ---------------------------------------------------------------------------

/// Apply a patch to a line. `note_project` is the project the containing note implies.
pub fn apply_patch(
    t: &mut TaskLine,
    p: &TaskPatch,
    today: &str,
    note_project: Option<&str>,
) -> Result<()> {
    if let Some(d) = &p.description {
        let d = one_line("description", d)?;
        if d.is_empty() {
            return Err(Error::Invalid("A task needs a description.".into()));
        }
        t.set_description(&d);
    }
    if let Some(s) = p.status {
        match s {
            TaskStatus::Done | TaskStatus::Cancelled => {
                return Err(Error::Invalid("Use done or cancel to close a task.".into()))
            }
            TaskStatus::Started if t.get(FieldKind::Start).is_none() => {
                t.set(FieldKind::Start, Some(today))
            }
            _ => {}
        }
        t.set_status(s);
        t.set(FieldKind::Done, None);
        t.set(FieldKind::Cancelled, None);
    }
    if let Some(pr) = p.priority {
        t.set_priority(pr);
    }
    if let Some(tags) = &p.tags {
        for old in t.tags() {
            if !old.starts_with("project/") {
                t.remove_tag(&old);
            }
        }
        for tag in tags {
            t.add_tag(tag);
        }
    }
    for tag in p.remove_tags.iter().flatten() {
        t.remove_tag(&clean_tag(tag));
    }
    for tag in p.add_tags.iter().flatten() {
        t.add_tag(tag);
    }
    if let Some(project) = &p.project {
        for old in t.tags() {
            if old.starts_with("project/") {
                t.remove_tag(&old);
            }
        }
        if let Some(name) = project.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            let name = clean_tag(name);
            if note_project.is_none_or(|np| !np.eq_ignore_ascii_case(&name)) {
                t.add_tag(&format!("project/{name}"));
            }
        }
    }
    for (kind, name, v) in [
        (FieldKind::Start, "start", &p.start),
        (FieldKind::Scheduled, "scheduled", &p.scheduled),
        (FieldKind::Due, "due", &p.due),
    ] {
        match v {
            Some(Some(d)) if !d.trim().is_empty() => t.set(kind, Some(&check_date(name, d)?)),
            Some(_) => t.set(kind, None),
            None => {}
        }
    }
    match &p.recur {
        Some(Some(r)) if !r.trim().is_empty() => t.set(FieldKind::Recur, Some(&check_recur(r)?)),
        Some(_) => t.set(FieldKind::Recur, None),
        None => {}
    }
    if let Some(deps) = &p.depends {
        let deps = check_ids(deps)?;
        if let Some(own) = t.get(FieldKind::Id) {
            if deps.iter().any(|d| d == own) {
                return Err(Error::Invalid("A task cannot depend on itself.".into()));
            }
        }
        let joined = deps.join(",");
        t.set(
            FieldKind::Depends,
            (!deps.is_empty()).then_some(joined.as_str()),
        );
    }
    Ok(())
}

/// The project the containing note stands for (its stem, if it is `type: project`).
fn note_project(path: &std::path::Path, rel: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let fm = frontmatter::parse(&text);
    if fm.props.get("type").is_some_and(|t| t == "project") {
        std::path::Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    } else {
        None
    }
}

pub fn update(ctx: &mut Ctx, key: &str, patch: &TaskPatch) -> Result<Task> {
    let t = target(ctx, key)?;
    let np = note_project(&t.path, &t.rel);
    let task = edit_line(ctx, key, |line, today| {
        apply_patch(line, patch, today, np.as_deref())
    })?;
    match &patch.note {
        Some(to) if *to != task.note => move_to(ctx, &task.key, to),
        _ => Ok(task),
    }
}

/// Toggle started: `[/]` with `🛫` today, or back to `[ ]` without the start date.
pub fn start(ctx: &mut Ctx, key: &str) -> Result<Task> {
    edit_line(ctx, key, |t, today| {
        match t.status() {
            TaskStatus::Started => {
                t.set_status(TaskStatus::Pending);
                t.set(FieldKind::Start, None);
            }
            TaskStatus::Done | TaskStatus::Cancelled => {
                return Err(Error::Invalid("A closed task cannot be started.".into()))
            }
            _ => {
                t.set_status(TaskStatus::Started);
                t.set(FieldKind::Start, Some(today));
            }
        }
        Ok(())
    })
}

pub fn cancel(ctx: &mut Ctx, key: &str) -> Result<Task> {
    edit_line(ctx, key, |t, today| {
        t.set_status(TaskStatus::Cancelled);
        t.set(FieldKind::Done, None);
        t.set(FieldKind::Cancelled, Some(today));
        Ok(())
    })
}

/// The next occurrence of a recurring line done on `today`, or `None` without a rule.
pub fn next_occurrence(t: &TaskLine, today: NaiveDate, id: &str) -> Result<Option<TaskLine>> {
    let Some(rule) = t.get(FieldKind::Recur) else {
        return Ok(None);
    };
    let rule = parse_rule(rule)?;
    let get = |k| t.get(k).and_then(date);
    let dates = [
        get(FieldKind::Start),
        get(FieldKind::Scheduled),
        get(FieldKind::Due),
    ];
    let next = next_dates(&rule, dates, today)?;
    let mut n = t.clone();
    n.set_status(if t.status() == TaskStatus::Waiting {
        TaskStatus::Waiting
    } else {
        TaskStatus::Pending
    });
    n.set(FieldKind::Done, None);
    n.set(FieldKind::Cancelled, None);
    for (kind, d) in [FieldKind::Start, FieldKind::Scheduled, FieldKind::Due]
        .into_iter()
        .zip(next)
    {
        n.set(kind, d.map(fmt).as_deref());
    }
    n.set(FieldKind::Created, Some(&fmt(today)));
    n.set(FieldKind::Id, Some(id));
    Ok(Some(n))
}

/// Done; a recurring task gets its next occurrence inserted above it.
pub fn done(ctx: &mut Ctx, key: &str) -> Result<Task> {
    let today = ctx.today();
    let today_d = ctx.today;
    let (vault, id) = edit(ctx, key, |found, block, used| {
        let mut t = found.task.clone();
        let id = ensure_identity(&mut t, used, &today);
        if t.status() == TaskStatus::Done {
            return Ok(Outcome::Lines {
                lines: block.to_vec(),
                id,
            });
        }
        let next = if t.status() == TaskStatus::Started {
            // a started task's 🛫 is when work began, not part of the repeat pattern
            let mut base = t.clone();
            base.set(FieldKind::Start, None);
            next_occurrence(&base, today_d, &fresh_id(used))?
        } else {
            next_occurrence(&t, today_d, &fresh_id(used))?
        };
        t.set_status(TaskStatus::Done);
        t.set(FieldKind::Cancelled, None);
        t.set(FieldKind::Done, Some(&today));
        let mut lines = Vec::new();
        if let Some(n) = next {
            lines.push(n.print());
        }
        lines.push(t.print());
        lines.extend(block[1..].iter().cloned());
        Ok(Outcome::Lines { lines, id })
    })?;
    ctx.task(&vault, &id.expect("edited, not removed"))
}

/// Remove the line and its annotations.
pub fn delete(ctx: &mut Ctx, key: &str) -> Result<()> {
    edit(ctx, key, |_, _, _| Ok(Outcome::Removed))?;
    Ok(())
}

/// Add an indented `- text (date)` line under the task.
pub fn annotate(ctx: &mut Ctx, key: &str, text: &str) -> Result<Task> {
    let note = one_line("annotation", text)?;
    if note.is_empty() {
        return Err(Error::Invalid("An annotation needs some text.".into()));
    }
    let today = ctx.today();
    let (vault, id) = edit(ctx, key, |found, block, used| {
        let mut t = found.task.clone();
        let id = ensure_identity(&mut t, used, &today);
        let mut lines = vec![t.print()];
        lines.extend(block[1..].iter().cloned());
        let indent: String = t
            .indent
            .chars()
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();
        lines.push(format!("{indent}    - {note} ({today})"));
        Ok(Outcome::Lines { lines, id })
    })?;
    ctx.task(&vault, &id.expect("edited, not removed"))
}

/// Move a task with its annotations to another note's `## Tasks` section. The new copy is
/// written before the old one is removed, so a failure in between leaves two, never none.
pub fn move_to(ctx: &mut Ctx, key: &str, to: &str) -> Result<Task> {
    let (to_vault, to_rel) =
        split_note_id(to).ok_or_else(|| Error::Invalid(format!("\"{to}\" is not a note id.")))?;
    let dest = ctx.vault(to_vault)?.clone();
    if !dest.resolve_rel(to_rel)?.exists() {
        return Err(Error::NotFound(format!(
            "No note {to_rel} in {}.",
            dest.name
        )));
    }
    let t = target(ctx, key)?;
    let (text, _) = locks::read(&t.path)?;
    let nid = note_id(&t.vault, &t.rel);
    let found = parse::parse_note(&text);
    let i = find_in(&found, &nid, key)
        .ok_or_else(|| Error::NotFound(format!("The task {key} is no longer in {}.", t.rel)))?;
    let today = ctx.today();
    let mut line = found[i].task.clone();
    let mut used = used_ids(ctx.index, &dest.id, &text);
    let id = ensure_identity(&mut line, &mut used, &today);
    line.indent.clear();
    let mut block = vec![line.print()];
    for l in &parse::lines(&text)[found[i].line + 1..found[i].end] {
        block.push(format!("    {}", l.trim_start()));
    }
    append_lines(&dest, to_rel, &block, &today)?;
    ctx.index.refresh_note(&dest, to_rel);
    // the copy has the id now; remove the original by the key it had
    let src = ctx.vault(&t.vault)?.clone();
    let removed = locks::guarded_edit(&t.path, None, |cur| {
        let found = parse::parse_note(cur);
        match find_in(&found, &nid, key) {
            Some(i) => Ok(frontmatter::bump_updated(
                &splice(cur, found[i].line, found[i].end, &[]),
                &today,
            )),
            None => Err(Error::NotFound(format!(
                "The task was copied to {to_rel} but changed in {} meanwhile; remove the old line by hand.",
                t.rel
            ))),
        }
    });
    ctx.index.refresh_note(&src, &t.rel);
    removed?;
    ctx.task(&dest.id, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::tests::vault_with;
    use crate::types::Priority;

    const GARDEN: &str = "---\ntitle: Garden\ntype: project\nupdated: 2026-01-01\n---\n\n## Tasks\n- [ ] Dig the bed 🆔 aaa111\n    - spade is in the shed (2026-10-01)\n- [ ] Plant ⛔ aaa111\n\n## Notes\nSoil is clay.\n";

    fn ctx<'a>(vaults: &'a [Vault], index: &'a mut TaskIndex) -> Ctx<'a> {
        Ctx {
            vaults,
            index,
            today: date("2026-11-02").unwrap(),
        }
    }

    fn read(v: &Vault, rel: &str) -> String {
        std::fs::read_to_string(v.root.join(rel)).unwrap()
    }

    #[test]
    fn done_by_id_keeps_everything_else() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        let t = done(&mut c, "aaa111").unwrap();
        assert_eq!(t.status, TaskStatus::Done);
        assert_eq!(t.done.as_deref(), Some("2026-11-02"));
        let text = read(&v, "01-Projects/garden.md");
        assert!(text.contains("updated: 2026-11-02\n"));
        assert!(text.contains(
            "- [x] Dig the bed ➕ 2026-11-02 ✅ 2026-11-02 🆔 aaa111\n    - spade is in the shed (2026-10-01)\n- [ ] Plant ⛔ aaa111\n\n## Notes\nSoil is clay.\n"
        ));
        // done twice is a no-op
        done(&mut c, "aaa111").unwrap();
        assert_eq!(read(&v, "01-Projects/garden.md"), text);
    }

    #[test]
    fn a_task_without_id_gets_one_on_first_edit() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let key = idx
            .tasks()
            .find(|t| t.description == "Plant")
            .unwrap()
            .key
            .clone();
        let mut c = ctx(&vs, &mut idx);
        let t = start(&mut c, &key).unwrap();
        let id = t.id.clone().unwrap();
        assert_eq!(id.len(), 6);
        assert_ne!(id, "aaa111");
        assert_eq!(t.status, TaskStatus::Started);
        assert!(t.blocked);
        assert!(read(&v, "01-Projects/garden.md").contains(&format!(
            "- [/] Plant ➕ 2026-11-02 🛫 2026-11-02 🆔 {id} ⛔ aaa111\n"
        )));
        // the old content key no longer finds it
        assert!(matches!(start(&mut c, &key), Err(Error::NotFound(_))));
        // toggling stops it
        let t = start(&mut c, &id).unwrap();
        assert_eq!(t.status, TaskStatus::Pending);
        assert_eq!(t.start, None);
    }

    #[test]
    fn edits_survive_concurrent_changes_elsewhere_in_the_note() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        // someone adds lines above the task after the index was built
        let p = v.root.join("01-Projects/garden.md");
        let text = read(&v, "01-Projects/garden.md")
            .replace("## Tasks\n", "## Tasks\n- [ ] new one\nIntro\n");
        std::fs::write(&p, text).unwrap();
        let mut c = ctx(&vs, &mut idx);
        let patch = TaskPatch {
            due: Some(Some("2026-11-20".into())),
            priority: Some(Some(Priority::High)),
            add_tags: Some(vec!["outdoor".into()]),
            ..Default::default()
        };
        let t = update(&mut c, "aaa111", &patch).unwrap();
        assert_eq!(t.due.as_deref(), Some("2026-11-20"));
        let text = read(&v, "01-Projects/garden.md");
        assert!(text.contains("## Tasks\n- [ ] new one\nIntro\n- [ ] Dig the bed #outdoor ⏫ ➕ 2026-11-02 📅 2026-11-20 🆔 aaa111\n    - spade"));
    }

    #[test]
    fn recurring_done_inserts_the_next_above() {
        let (_d, v) = vault_with(&[(
            "09-Planning/Tasks.md",
            "- [ ] Water plants 🔁 every week ⏳ 2026-10-30 📅 2026-11-01 🆔 rec001\r\n- [ ] other\r\n",
        )]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        done(&mut c, "rec001").unwrap();
        let text = read(&v, "09-Planning/Tasks.md");
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert!(lines[0].starts_with(
            "- [ ] Water plants ➕ 2026-11-02 🔁 every week ⏳ 2026-11-06 📅 2026-11-08 🆔 "
        ));
        assert!(!lines[0].contains("rec001"));
        assert_eq!(
            lines[1],
            "- [x] Water plants ➕ 2026-11-02 🔁 every week ⏳ 2026-10-30 📅 2026-11-01 ✅ 2026-11-02 🆔 rec001"
        );
        assert_eq!(lines[2], "- [ ] other");
        assert_eq!(text.matches('\n').count(), 3, "CRLF kept, one line added");
    }

    #[test]
    fn add_goes_to_the_right_note() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        let t = add(
            &mut c,
            &TaskInput {
                description: "Mulch".into(),
                project: Some("Garden".into()),
                due: Some("2026-11-09".into()),
                tags: vec!["outdoor".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(t.project.as_deref(), Some("garden"));
        let id = t.id.unwrap();
        let text = read(&v, "01-Projects/garden.md");
        assert!(text.contains(&format!(
            "- [ ] Plant ⛔ aaa111\n- [ ] Mulch #outdoor ➕ 2026-11-02 📅 2026-11-09 🆔 {id}\n\n## Notes"
        )));

        // no project note: the loose list, created with frontmatter, tagged with the project
        let t = add(
            &mut c,
            &TaskInput {
                description: "Renew passport".into(),
                project: Some("travel".into()),
                priority: Some(Priority::Highest),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(t.project.as_deref(), Some("travel"));
        assert_eq!(t.note, format!("{}:09-Planning/Tasks.md", v.id));
        let loose = read(&v, LOOSE_TASKS);
        assert!(loose.starts_with("---\ntitle: Tasks\ntype: index\n"));
        assert!(
            loose.contains("# Tasks\n\n- [ ] Renew passport #project/travel 🔺 ➕ 2026-11-02 🆔 ")
        );

        // a note without a tasks section gets one
        std::fs::write(v.root.join("idea.md"), "# Idea\nSome text").unwrap();
        idx_note_add(&mut c, &v, "idea.md", "Try it");
        assert!(read(&v, "idea.md")
            .starts_with("# Idea\nSome text\n\n## Tasks\n- [ ] Try it ➕ 2026-11-02 🆔 "));

        for bad in [
            TaskInput::default(),
            TaskInput {
                description: "two\nlines".into(),
                ..Default::default()
            },
            TaskInput {
                description: "x".into(),
                due: Some("friday".into()),
                ..Default::default()
            },
            TaskInput {
                description: "x".into(),
                recur: Some("weekly".into()),
                ..Default::default()
            },
            TaskInput {
                description: "x".into(),
                note: Some(format!("{}:missing.md", v.id)),
                ..Default::default()
            },
        ] {
            assert!(add(&mut c, &bad).is_err(), "{bad:?}");
        }
    }

    fn idx_note_add(c: &mut Ctx, v: &Vault, rel: &str, desc: &str) {
        add(
            c,
            &TaskInput {
                description: desc.into(),
                note: Some(note_id(&v.id, rel)),
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn cancel_delete_annotate() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        let t = annotate(&mut c, "aaa111", "borrowed a fork").unwrap();
        assert_eq!(t.annotations.len(), 2);
        assert_eq!(t.annotations[1].text, "borrowed a fork");
        assert!(read(&v, "01-Projects/garden.md").contains(
            "    - spade is in the shed (2026-10-01)\n    - borrowed a fork (2026-11-02)\n- [ ] Plant"
        ));
        assert!(annotate(&mut c, "aaa111", "  ").is_err());
        let t = cancel(&mut c, "aaa111").unwrap();
        assert_eq!(t.status, TaskStatus::Cancelled);
        assert_eq!(t.cancelled.as_deref(), Some("2026-11-02"));
        delete(&mut c, "aaa111").unwrap();
        let text = read(&v, "01-Projects/garden.md");
        assert!(!text.contains("Dig"));
        assert!(!text.contains("spade"));
        assert!(text.contains("## Tasks\n- [ ] Plant ⛔ aaa111\n\n## Notes"));
        assert!(matches!(delete(&mut c, "aaa111"), Err(Error::NotFound(_))));
    }

    #[test]
    fn patch_fields_and_refusals() {
        let (_d, v) = vault_with(&[("01-Projects/garden.md", GARDEN)]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        let patch: TaskPatch = serde_json::from_str(
            r#"{"description":"Dig the big bed","tags":["a","b"],"scheduled":"2026-11-05","recur":"every month","depends":["zzz999"],"status":"waiting"}"#,
        )
        .unwrap();
        let t = update(&mut c, "aaa111", &patch).unwrap();
        assert_eq!(t.description, "Dig the big bed");
        assert_eq!(t.tags, vec!["a", "b"]);
        assert_eq!(t.status, TaskStatus::Waiting);
        assert!(t.hidden);
        assert_eq!(t.recur.as_deref(), Some("every month"));
        assert_eq!(t.depends, vec!["zzz999"]);
        // null clears, missing leaves alone
        let patch: TaskPatch =
            serde_json::from_str(r#"{"scheduled":null,"recur":null,"project":"house"}"#).unwrap();
        assert_eq!(patch.priority, None);
        let t = update(&mut c, "aaa111", &patch).unwrap();
        assert_eq!(t.scheduled, None);
        assert_eq!(t.recur, None);
        assert_eq!(t.description, "Dig the big bed");
        assert_eq!(t.project.as_deref(), Some("house"));
        // back to the note's own project: the tag goes
        let patch: TaskPatch = serde_json::from_str(r#"{"project":"garden"}"#).unwrap();
        let t = update(&mut c, "aaa111", &patch).unwrap();
        assert_eq!(t.project.as_deref(), Some("garden"));
        assert!(!t.raw.contains("#project/"));
        for bad in [
            r#"{"status":"done"}"#,
            r#"{"due":"tomorrow"}"#,
            r#"{"depends":["aaa111"]}"#,
            r#"{"depends":["no way"]}"#,
            r#"{"description":""}"#,
        ] {
            let p: TaskPatch = serde_json::from_str(bad).unwrap();
            assert!(update(&mut c, "aaa111", &p).is_err(), "{bad}");
        }
        assert!(matches!(
            update(&mut c, "nope00", &TaskPatch::default()),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn move_between_notes() {
        let (_d, v) = vault_with(&[
            ("01-Projects/garden.md", GARDEN),
            (
                "01-Projects/house.md",
                "---\ntitle: House\ntype: project\n---\n\n## Tasks\n- [ ] Paint\n",
            ),
        ]);
        let vs = [v.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        let patch = TaskPatch {
            note: Some(note_id(&v.id, "01-Projects/house.md")),
            ..Default::default()
        };
        let t = update(&mut c, "aaa111", &patch).unwrap();
        assert_eq!(t.project.as_deref(), Some("house"));
        assert_eq!(t.annotations.len(), 1);
        assert!(!read(&v, "01-Projects/garden.md").contains("Dig"));
        assert!(read(&v, "01-Projects/house.md").contains(
            "- [ ] Paint\n- [ ] Dig the bed ➕ 2026-11-02 🆔 aaa111\n    - spade is in the shed (2026-10-01)\n"
        ));
    }

    #[test]
    fn ambiguous_ids_across_vaults() {
        let (_a, v1) = vault_with(&[("a.md", "- [ ] one 🆔 same01\n")]);
        let (_b, v2) = vault_with(&[("b.md", "- [ ] two 🆔 same01\n")]);
        let vs = [v1.clone(), v2.clone()];
        let mut idx = TaskIndex::build(&vs);
        let mut c = ctx(&vs, &mut idx);
        assert!(matches!(done(&mut c, "same01"), Err(Error::Invalid(_))));
        let t = done(&mut c, &format!("{}:same01", v2.id)).unwrap();
        assert_eq!(t.description, "two");
    }

    #[test]
    fn splice_and_sections() {
        assert_eq!(splice("a\nb\nc", 1, 2, &["X".into()]), "a\nX\nc");
        assert_eq!(splice("a\nb", 1, 2, &[]), "a\n");
        assert_eq!(
            splice("a\nb", 2, 2, &["c".into()]),
            "a\nbc",
            "callers add the newline first"
        );
        assert_eq!(splice("", 0, 0, &["x".into()]), "x\n");
        let t = insert_into_tasks_section(
            "# T\n\n### Tasks\n\n- [ ] a\n\n\n### Next\n",
            &["- [ ] b".into()],
            false,
        );
        assert_eq!(t, "# T\n\n### Tasks\n\n- [ ] a\n- [ ] b\n\n\n### Next\n");
        let t = insert_into_tasks_section("## Tasks\n#tag line\n", &["- [ ] b".into()], false);
        assert_eq!(t, "## Tasks\n#tag line\n- [ ] b\n");
        let t = insert_into_tasks_section("", &["- [ ] b".into()], true);
        assert_eq!(t, "- [ ] b\n");
    }
}
