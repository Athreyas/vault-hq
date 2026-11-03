//! The tool catalogue (SPEC 9): names, JSON Schemas, descriptions, argument checks, and
//! dispatch to the `Backend`. The descriptions are what an agent reads to decide what to
//! call, so they say when to use a tool and what the rules are, not how it is built.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::backend::*;
use hq_core::Error;

/// Longest text one write may carry. Agents sometimes paste whole files; a vault note
/// that size is almost always a mistake, and a huge line would stall every reader.
pub const MAX_TEXT: usize = 100_000;

pub enum CallError {
    /// No such tool: a protocol error (JSON-RPC -32602), per the MCP spec.
    UnknownTool(String),
    /// Bad arguments or a failed call: reported as a tool result with `isError: true`, so
    /// the model sees the message and can correct itself.
    Tool(Error),
}

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub read_only: bool,
    pub idempotent: bool,
    pub open_world: bool,
    pub schema: fn() -> Value,
}

impl Tool {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": (self.schema)(),
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                // nothing over MCP deletes or overwrites: writes add, or edit under a hash
                "destructiveHint": false,
                "idempotentHint": self.idempotent,
                "openWorldHint": self.open_world,
            }
        })
    }
}

fn object(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

const DATE: &str = "^\\d{4}-\\d{2}-\\d{2}$";

fn vault_prop() -> Value {
    json!({
        "type": "string",
        "description": "Vault id or name (see vault_info). Default: the primary vault."
    })
}

fn scope_prop() -> Value {
    json!({
        "type": "object",
        "description": "Narrow what is searched. Omit to search everything.",
        "properties": {
            "vaults": {
                "type": "array", "items": { "type": "string" },
                "description": "Vault ids or names. Empty or omitted = every vault."
            },
            "days": {
                "type": "integer", "minimum": 1, "maximum": 3650,
                "description": "Only notes dated within the last N days."
            },
            "kind": {
                "type": "string", "enum": ["all", "notes", "bookmarks"], "default": "all",
                "description": "Notes, saved bookmarks, or both."
            }
        },
        "additionalProperties": false
    })
}

fn priority_prop() -> Value {
    json!({
        "type": "string",
        "enum": ["highest", "high", "medium", "low", "lowest", "none"],
        "description": "Task priority."
    })
}

fn tags_prop(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string", "pattern": "^[A-Za-z0-9_/-]+$" },
        "description": format!("{what} Without the leading #; nested tags use a slash (area/garden).")
    })
}

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "vault_info",
        title: "Vault info",
        description: "What vaults this brain has, their folders and note counts, today's date, \
            and the author name your writes are recorded under. Call it first in a session. \
            Then read the resource vault://AGENTS.md: it is this vault's contract for agents \
            (what goes where, what you may do).",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || object(json!({}), &[]),
    },
    Tool {
        name: "search",
        title: "Search notes",
        description: "Find notes and bookmarks by meaning and keywords. Returns note ids with a \
            short snippet; call read_note on an id for the whole note. Prefer this over \
            list_notes when you are looking for a topic.",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || {
            object(
                json!({
                    "query": { "type": "string", "minLength": 1,
                        "description": "What to look for, in plain words." },
                    "scope": scope_prop(),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10,
                        "description": "Most hits to return." }
                }),
                &["query"],
            )
        },
    },
    Tool {
        name: "ask",
        title: "Ask the vault",
        description: "Answer a question from the user's own notes, with the local model the \
            user chose, and return the answer with the notes it used. Slower than search; use \
            it when you need a synthesis, and search when you need sources. Fails with an \
            Unavailable error when no local model is set up: fall back to search.",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || {
            object(
                json!({
                    "question": { "type": "string", "minLength": 1 },
                    "scope": scope_prop()
                }),
                &["question"],
            )
        },
    },
    Tool {
        name: "read_note",
        title: "Read a note",
        description: "Read one note: its frontmatter (props), body and hash. Give either `id` \
            (\"<vaultId>:<path>\", as search and list_notes return) or `ref` (a [[wiki link]] \
            target such as a file name without .md, or a vault-relative path). Keep the \
            `hash`: append_to_note needs it to prove you saw the latest text.",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || {
            object(
                json!({
                    "id": { "type": "string",
                        "description": "Note id, \"<vaultId>:<vault-relative path>\"." },
                    "ref": { "type": "string",
                        "description": "A link target: \"garden\", \"01-Projects/garden\", \"[[garden]]\"." },
                    "vault": vault_prop()
                }),
                &[],
            )
        },
    },
    Tool {
        name: "list_notes",
        title: "List notes",
        description: "List notes, newest first, optionally only in one folder, of one type, \
            or updated since a date. Returns ids, titles, types and dates, not bodies. Use \
            `offset` with `next_offset` to page.",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || {
            object(
                json!({
                    "folder": { "type": "string",
                        "description": "Vault-relative folder, e.g. \"01-Projects\" (subfolders included)." },
                    "type": { "type": "string",
                        "description": "Frontmatter type: note, project, area, reference, findings, daily, capture, session, proposal, bookmark, memory, agent, index." },
                    "updated_since": { "type": "string", "pattern": DATE,
                        "description": "YYYY-MM-DD; notes updated on or after it." },
                    "vault": vault_prop(),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 100 },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 }
                }),
                &[],
            )
        },
    },
    Tool {
        name: "create_note",
        title: "Create a note",
        description: "Create a new note with proper frontmatter (title, type, dates, your \
            author name, tags). It goes in 00-Inbox unless you name a folder, so the user \
            files it; put it elsewhere only when AGENTS.md says you may. Never overwrites: \
            if a note with that file name exists you get an error. Returns its id and hash.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "title": { "type": "string", "minLength": 1, "maxLength": 200,
                        "description": "The title; the file name is made from it." },
                    "folder": { "type": "string", "default": "00-Inbox",
                        "description": "Vault-relative folder, e.g. \"05-References\"." },
                    "type": { "type": "string", "default": "note",
                        "description": "Frontmatter type (see list_notes). Use session for a work-session summary, proposal for a suggested change." },
                    "body": { "type": "string", "maxLength": MAX_TEXT,
                        "description": "Markdown after the frontmatter. Link other notes with [[name]]." },
                    "tags": tags_prop("Tags for the frontmatter."),
                    "vault": vault_prop()
                }),
                &["title"],
            )
        },
    },
    Tool {
        name: "append_to_note",
        title: "Append to a note",
        description: "Add text to the end of a note, or to the end of one section when you \
            name its heading. You must pass the `hash` from your latest read_note as \
            `base_hash`: if the note changed since (the user or another agent edited it), \
            nothing is written and you get a Conflict error with the current hash. Then read \
            the note again, check your text still fits, and retry with the new hash.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "id": { "type": "string", "description": "Note id from read_note." },
                    "heading": { "type": "string",
                        "description": "Heading text without the #s, e.g. \"Notes\". Added at the end of the note if missing." },
                    "text": { "type": "string", "minLength": 1, "maxLength": MAX_TEXT,
                        "description": "Markdown to add." },
                    "base_hash": { "type": "string", "pattern": "^[0-9a-f]{64}$",
                        "description": "The hash read_note returned." }
                }),
                &["id", "text", "base_hash"],
            )
        },
    },
    Tool {
        name: "capture",
        title: "Quick capture",
        description: "Drop a thought into today's inbox note (00-Inbox/YYYY-MM-DD-capture.md) \
            as a timestamped line, for the user to sort later. Use it for anything worth \
            keeping that has no obvious home. One idea per call, one line of text.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "text": { "type": "string", "minLength": 1, "maxLength": 2000 },
                    "vault": vault_prop()
                }),
                &["text"],
            )
        },
    },
    Tool {
        name: "log",
        title: "Agent log",
        description: "Add one line to the Agent log section of today's daily note, recording \
            what you did (\"filed 3 captures into 01-Projects/garden\"). Log meaningful \
            actions on the vault, not every read.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "text": { "type": "string", "minLength": 1, "maxLength": 500 },
                    "vault": vault_prop()
                }),
                &["text"],
            )
        },
    },
    Tool {
        name: "add_task",
        title: "Add a task",
        description: "Add a task (a checkbox line) to a project note's Tasks section, or to \
            the loose task list when no project is given. Returns the task with its id, \
            which update_task takes.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "description": { "type": "string", "minLength": 1, "maxLength": 500 },
                    "project": { "type": "string",
                        "description": "A project note id, or a project name." },
                    "priority": priority_prop(),
                    "due": { "type": "string", "pattern": DATE, "description": "YYYY-MM-DD." },
                    "scheduled": { "type": "string", "pattern": DATE,
                        "description": "YYYY-MM-DD: the day to start on it." },
                    "tags": tags_prop("Task tags."),
                    "vault": vault_prop()
                }),
                &["description"],
            )
        },
    },
    Tool {
        name: "update_task",
        title: "Update a task",
        description: "Change a task by its id: mark it started, done, waiting or cancelled, \
            change its text, priority, dates or tags, or add an annotation. Only the fields \
            in `patch` change. A date set to \"\" is cleared. Tasks cannot be deleted here; \
            cancel instead.",
        read_only: false,
        idempotent: false,
        open_world: false,
        schema: || {
            object(
                json!({
                    "id": { "type": "string", "pattern": "^[a-z0-9]{6}$",
                        "description": "The task id (6 characters) from list_tasks or add_task." },
                    "patch": {
                        "type": "object",
                        "minProperties": 1,
                        "properties": {
                            "description": { "type": "string", "minLength": 1, "maxLength": 500 },
                            "status": { "type": "string",
                                "enum": ["pending", "started", "waiting", "done", "cancelled"] },
                            "priority": priority_prop(),
                            "due": { "type": "string", "pattern": "^(\\d{4}-\\d{2}-\\d{2})?$",
                                "description": "YYYY-MM-DD, or \"\" to clear." },
                            "scheduled": { "type": "string", "pattern": "^(\\d{4}-\\d{2}-\\d{2})?$",
                                "description": "YYYY-MM-DD, or \"\" to clear." },
                            "tags": tags_prop("Replaces the task's tags."),
                            "annotate": { "type": "string", "minLength": 1, "maxLength": 500,
                                "description": "A note to add under the task." }
                        },
                        "additionalProperties": false
                    }
                }),
                &["id", "patch"],
            )
        },
    },
    Tool {
        name: "list_tasks",
        title: "List tasks",
        description: "List tasks across the vault, most urgent first by default. With no \
            filters it returns open tasks (pending, started, waiting).",
        read_only: true,
        idempotent: true,
        open_world: false,
        schema: || {
            object(
                json!({
                    "query": { "type": "string", "description": "Words in the task text." },
                    "status": { "type": "array", "items": { "type": "string",
                        "enum": ["pending", "started", "waiting", "done", "cancelled"] },
                        "description": "Default: pending, started, waiting." },
                    "project": { "type": "string", "description": "Project note id or name." },
                    "tags": tags_prop("Tasks having all of these tags."),
                    "due_before": { "type": "string", "pattern": DATE },
                    "due_after": { "type": "string", "pattern": DATE },
                    "vault": vault_prop(),
                    "sort": { "type": "string", "enum": ["urgency", "due", "priority", "created"],
                        "default": "urgency" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 50 }
                }),
                &[],
            )
        },
    },
    Tool {
        name: "add_bookmark",
        title: "Save a bookmark",
        description: "Save a web page as a bookmark note in 10-Bookmarks, with an optional \
            note on why it matters. If the URL is already saved you get the existing note. \
            The page's title is fetched later by the app.",
        read_only: false,
        idempotent: true,
        open_world: true,
        schema: || {
            object(
                json!({
                    "url": { "type": "string", "pattern": "^https?://",
                        "description": "An http or https URL." },
                    "note": { "type": "string", "maxLength": 2000,
                        "description": "Why it is worth keeping." }
                }),
                &["url"],
            )
        },
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn list_json() -> Value {
    Value::Array(TOOLS.iter().map(Tool::to_json).collect())
}

// ---- argument checks --------------------------------------------------------------------

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, Error> {
    // a missing `arguments` is the same as `{}`
    let args = if args.is_null() { json!({}) } else { args };
    if !args.is_object() {
        return Err(Error::Invalid("arguments must be a JSON object".into()));
    }
    serde_json::from_value(args).map_err(|e| Error::Invalid(format!("Invalid arguments: {e}")))
}

fn required(field: &str, value: &str, max: usize) -> Result<(), Error> {
    if value.trim().is_empty() {
        return Err(Error::Invalid(format!("`{field}` must not be empty")));
    }
    if value.chars().count() > max {
        return Err(Error::Invalid(format!(
            "`{field}` is longer than {max} characters"
        )));
    }
    Ok(())
}

fn one_line(field: &str, value: &str) -> Result<(), Error> {
    if value.contains('\n') || value.contains('\r') {
        return Err(Error::Invalid(format!("`{field}` must be a single line")));
    }
    Ok(())
}

pub(crate) fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&s[5..7].parse::<u32>().unwrap_or(0))
        && (1..=31).contains(&s[8..10].parse::<u32>().unwrap_or(0))
}

fn date(field: &str, value: &Option<String>, allow_empty: bool) -> Result<(), Error> {
    match value {
        Some(v) if allow_empty && v.is_empty() => Ok(()),
        Some(v) if !is_date(v) => Err(Error::Invalid(format!(
            "`{field}` must be a date as YYYY-MM-DD, got \"{v}\""
        ))),
        _ => Ok(()),
    }
}

fn limit(field: &str, value: Option<u32>, max: u32) -> Result<(), Error> {
    match value {
        Some(n) if n == 0 || n > max => Err(Error::Invalid(format!(
            "`{field}` must be between 1 and {max}"
        ))),
        _ => Ok(()),
    }
}

fn tags(field: &str, list: &[String]) -> Result<(), Error> {
    for t in list {
        let t = t.strip_prefix('#').unwrap_or(t);
        if t.is_empty()
            || !t
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
        {
            return Err(Error::Invalid(format!(
                "`{field}`: \"{t}\" is not a tag (letters, digits, _ - /)"
            )));
        }
    }
    Ok(())
}

fn scope(s: &Scope) -> Result<(), Error> {
    if let Some(d) = s.days {
        if d == 0 || d > 3650 {
            return Err(Error::Invalid(
                "`scope.days` must be between 1 and 3650".into(),
            ));
        }
    }
    Ok(())
}

fn note_id(field: &str, id: &str) -> Result<(), Error> {
    if hq_core::types::split_note_id(id).is_none() {
        return Err(Error::Invalid(format!(
            "`{field}` must be a note id like \"<vaultId>:01-Projects/garden.md\" (from search, list_notes or read_note)"
        )));
    }
    Ok(())
}

fn task_id(id: &str) -> Result<(), Error> {
    if id.len() != 6
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err(Error::Invalid(format!(
            "`id` must be a 6-character task id like \"a1b2c3\", got \"{id}\""
        )));
    }
    Ok(())
}

// ---- dispatch ---------------------------------------------------------------------------

fn out<T: Serialize>(r: hq_core::Result<T>) -> Result<Value, Error> {
    let v = r?;
    serde_json::to_value(v).map_err(|e| Error::Internal(e.to_string()))
}

/// Check the arguments for `name` and run it. The result is the tool's structured output.
pub fn call(b: &dyn Backend, ctx: &Ctx, name: &str, args: Value) -> Result<Value, CallError> {
    let run = || -> Result<Value, Error> {
        match name {
            "vault_info" => {
                let _: Empty = parse(args)?;
                out(b.vault_info(ctx))
            }
            "search" => {
                let i: SearchInput = parse(args)?;
                required("query", &i.query, 2000)?;
                scope(&i.scope)?;
                limit("limit", i.limit, 50)?;
                out(b.search(ctx, i))
            }
            "ask" => {
                let i: AskInput = parse(args)?;
                required("question", &i.question, 4000)?;
                scope(&i.scope)?;
                out(b.ask(ctx, i))
            }
            "read_note" => {
                let i: ReadNoteInput = parse(args)?;
                match (&i.id, &i.reference) {
                    (Some(id), None) => note_id("id", id)?,
                    (None, Some(r)) => required("ref", r, 1000)?,
                    _ => return Err(Error::Invalid("give exactly one of `id` or `ref`".into())),
                }
                out(b.read_note(ctx, i))
            }
            "list_notes" => {
                let i: ListNotesInput = parse(args)?;
                date("updated_since", &i.updated_since, false)?;
                limit("limit", i.limit, 500)?;
                out(b.list_notes(ctx, i))
            }
            "create_note" => {
                let i: CreateNoteInput = parse(args)?;
                required("title", &i.title, 200)?;
                one_line("title", &i.title)?;
                if let Some(t) = &i.note_type {
                    required("type", t, 40)?;
                    one_line("type", t)?;
                }
                if let Some(body) = &i.body {
                    if body.chars().count() > MAX_TEXT {
                        return Err(Error::Invalid(format!(
                            "`body` is longer than {MAX_TEXT} characters"
                        )));
                    }
                }
                tags("tags", &i.tags)?;
                out(b.create_note(ctx, i))
            }
            "append_to_note" => {
                let i: AppendInput = parse(args)?;
                note_id("id", &i.id)?;
                required("text", &i.text, MAX_TEXT)?;
                if let Some(h) = &i.heading {
                    required("heading", h, 200)?;
                    one_line("heading", h)?;
                }
                if i.base_hash.len() != 64 || !i.base_hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(Error::Invalid(
                        "`base_hash` must be the 64-character hash read_note returned".into(),
                    ));
                }
                out(b.append_to_note(ctx, i))
            }
            "capture" => {
                let i: CaptureInput = parse(args)?;
                required("text", &i.text, 2000)?;
                out(b.capture(ctx, i))
            }
            "log" => {
                let i: LogInput = parse(args)?;
                required("text", &i.text, 500)?;
                one_line("text", &i.text)?;
                out(b.log(ctx, i))
            }
            "add_task" => {
                let i: AddTaskInput = parse(args)?;
                required("description", &i.description, 500)?;
                one_line("description", &i.description)?;
                date("due", &i.due, false)?;
                date("scheduled", &i.scheduled, false)?;
                tags("tags", &i.tags)?;
                out(b.add_task(ctx, i))
            }
            "update_task" => {
                let i: UpdateTaskInput = parse(args)?;
                task_id(&i.id)?;
                let p = &i.patch;
                if *p == TaskPatch::default() {
                    return Err(Error::Invalid(
                        "`patch` must change at least one field".into(),
                    ));
                }
                if let Some(d) = &p.description {
                    required("patch.description", d, 500)?;
                    one_line("patch.description", d)?;
                }
                if let Some(a) = &p.annotate {
                    required("patch.annotate", a, 500)?;
                    one_line("patch.annotate", a)?;
                }
                date("patch.due", &p.due, true)?;
                date("patch.scheduled", &p.scheduled, true)?;
                if let Some(t) = &p.tags {
                    tags("patch.tags", t)?;
                }
                out(b.update_task(ctx, i))
            }
            "list_tasks" => {
                let i: ListTasksInput = parse(args)?;
                date("due_before", &i.due_before, false)?;
                date("due_after", &i.due_after, false)?;
                tags("tags", &i.tags)?;
                limit("limit", i.limit, 500)?;
                out(b.list_tasks(ctx, i))
            }
            "add_bookmark" => {
                let i: AddBookmarkInput = parse(args)?;
                let u = i.url.trim();
                if !(u.starts_with("http://") || u.starts_with("https://"))
                    || u.len() < 10
                    || u.contains(char::is_whitespace)
                {
                    return Err(Error::Invalid(format!(
                        "`url` must be an http(s) URL, got \"{}\"",
                        i.url
                    )));
                }
                if let Some(n) = &i.note {
                    if n.chars().count() > 2000 {
                        return Err(Error::Invalid(
                            "`note` is longer than 2000 characters".into(),
                        ));
                    }
                }
                out(b.add_bookmark(ctx, i))
            }
            _ => unreachable!("checked by find() below"),
        }
    };
    if find(name).is_none() {
        return Err(CallError::UnknownTool(name.to_string()));
    }
    run().map_err(CallError::Tool)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert!(is_date("2026-11-02"));
        for bad in [
            "2026-13-01",
            "2026-1-01",
            "26-11-02",
            "2026-11-00",
            "tomorrow",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
    }

    #[test]
    fn every_tool_has_an_object_schema() {
        let mut names: Vec<_> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len(), "tool names are unique");
        for t in TOOLS {
            let s = (t.schema)();
            assert_eq!(s["type"], "object", "{}", t.name);
            assert_eq!(s["additionalProperties"], false, "{}", t.name);
            for r in s["required"].as_array().unwrap() {
                assert!(
                    s["properties"].get(r.as_str().unwrap()).is_some(),
                    "{}: required {r} is a property",
                    t.name
                );
            }
            assert!(t.description.len() > 40, "{} explains itself", t.name);
        }
    }
}
