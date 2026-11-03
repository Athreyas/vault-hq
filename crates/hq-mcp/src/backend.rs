//! What the MCP tools call. One method per tool, typed in and out, so the protocol layer
//! never touches the vault and tests can run it against a fake.
//!
//! `CoreBackend` (core_backend.rs) implements this over hq-core. Methods the other streams
//! own are stubs there that return `Error::Unavailable`; see the TODO list in that file.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use hq_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Per-call context: who is writing, and whether the client has given up on the call.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The `author:` value writes use, derived from `clientInfo.name` (see `author_for`).
    pub author: String,
    /// `clientInfo.name` exactly as the client sent it, for logs.
    pub client: String,
    cancel: Arc<AtomicBool>,
}

impl Ctx {
    pub fn new(client: &str) -> Self {
        Ctx {
            author: author_for(client),
            client: client.to_string(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn with_cancel(client: &str, cancel: Arc<AtomicBool>) -> Self {
        Ctx {
            cancel,
            ..Ctx::new(client)
        }
    }

    /// True once the client sent `notifications/cancelled` for this call (or disconnected).
    /// Long work (ask, big scans) should check it and stop early.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// `Err` if cancelled, for `?` between steps. Nothing is sent back for a cancelled call.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Unavailable(
                "The client cancelled this request.".into(),
            ))
        } else {
            Ok(())
        }
    }
}

/// The `author:` for a client: the agent names SPEC 3 lists when we recognise the client
/// (Claude Code says "claude-code", Claude Desktop "claude-ai", Codex "codex-mcp-client"),
/// otherwise a slug of its name. A client can never write as `human` or `hq`.
pub fn author_for(client: &str) -> String {
    let lower = client.to_lowercase();
    for known in ["claude", "cursor", "codex", "chatgpt", "gemini", "copilot"] {
        if lower.contains(known) {
            return known.into();
        }
    }
    let s = hq_core::util::slug(client, 40);
    match s.as_str() {
        "untitled" | "human" | "hq" => "agent".into(),
        _ => s,
    }
}

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Vault ids or names; empty = every vault.
    #[serde(default)]
    pub vaults: Vec<String>,
    /// Only notes dated within the last N days.
    pub days: Option<u32>,
    #[serde(default)]
    pub kind: ScopeKind,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    #[default]
    All,
    Notes,
    Bookmarks,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    pub query: String,
    #[serde(default)]
    pub scope: Scope,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskInput {
    pub question: String,
    #[serde(default)]
    pub scope: Scope,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadNoteInput {
    pub id: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Which vault a `ref` is resolved in; default = every vault, primary first.
    pub vault: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListNotesInput {
    pub folder: Option<String>,
    #[serde(rename = "type")]
    pub note_type: Option<String>,
    pub updated_since: Option<String>,
    pub vault: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateNoteInput {
    pub title: String,
    pub folder: Option<String>,
    #[serde(rename = "type")]
    pub note_type: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub vault: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendInput {
    pub id: String,
    pub heading: Option<String>,
    pub text: String,
    pub base_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureInput {
    pub text: String,
    pub vault: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Started,
    Waiting,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddTaskInput {
    pub description: String,
    /// A project note id, or a project name (`#project/<name>`).
    pub project: Option<String>,
    pub priority: Option<Priority>,
    pub due: Option<String>,
    /// `HH:MM` (24-hour, local) on the due date: written as `⏰`, and HQ reminds then.
    pub due_time: Option<String>,
    pub scheduled: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub vault: Option<String>,
}

/// Fields to change. A date given as "" clears it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskPatch {
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<Priority>,
    pub due: Option<String>,
    pub scheduled: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Adds an annotation line under the task.
    pub annotate: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskInput {
    pub id: String,
    pub patch: TaskPatch,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskSort {
    #[default]
    Urgency,
    Due,
    Priority,
    Created,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTasksInput {
    /// Free text matched against descriptions.
    pub query: Option<String>,
    /// Default: open tasks (pending, started, waiting).
    #[serde(default)]
    pub status: Vec<TaskStatus>,
    pub project: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub due_before: Option<String>,
    pub due_after: Option<String>,
    pub vault: Option<String>,
    #[serde(default)]
    pub sort: TaskSort,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddBookmarkInput {
    pub url: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogInput {
    pub text: String,
    pub vault: Option<String>,
}

// ---- outputs ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchOutput {
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AskOutput {
    pub answer: String,
    pub sources: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoteOut {
    pub id: String,
    pub title: String,
    /// Frontmatter, as written.
    pub props: BTreeMap<String, String>,
    /// Everything after the frontmatter.
    pub body: String,
    /// SHA-256 of the whole file: pass it back as `base_hash` when writing.
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub note_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ListNotesOutput {
    pub notes: Vec<NoteSummary>,
    /// Matching notes in all, before `limit`/`offset`.
    pub total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
}

/// After a write: the note and its new hash (use it as the next `base_hash`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WriteOutput {
    pub id: String,
    pub hash: String,
}

/// After capture or log: where the line went and the line itself.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LineOutput {
    pub id: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskOut {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The note the checkbox line lives in.
    pub note_id: String,
    pub urgency: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ListTasksOutput {
    pub tasks: Vec<TaskOut>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BookmarkOut {
    pub id: String,
    pub url: String,
    pub title: String,
    /// False when the URL was already bookmarked; `id` is then the existing note.
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VaultSummary {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub notes: u32,
    pub unreadable: u32,
    /// Top-level folders, so an agent knows where things go.
    pub folders: Vec<String>,
    pub has_agents_md: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VaultInfoOutput {
    pub vaults: Vec<VaultSummary>,
    /// The `author:` this session's writes carry.
    pub author: String,
    pub today: String,
    pub server_version: String,
}

/// A resource body (`resources/read`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceText {
    pub mime_type: String,
    pub text: String,
}

/// Everything the MCP server can do to a vault. Implementations must be safe to call from
/// several threads at once (each `tools/call` runs on its own thread).
pub trait Backend: Send + Sync {
    fn search(&self, ctx: &Ctx, input: SearchInput) -> Result<SearchOutput>;
    fn ask(&self, ctx: &Ctx, input: AskInput) -> Result<AskOutput>;
    fn read_note(&self, ctx: &Ctx, input: ReadNoteInput) -> Result<NoteOut>;
    fn list_notes(&self, ctx: &Ctx, input: ListNotesInput) -> Result<ListNotesOutput>;
    fn create_note(&self, ctx: &Ctx, input: CreateNoteInput) -> Result<WriteOutput>;
    fn append_to_note(&self, ctx: &Ctx, input: AppendInput) -> Result<WriteOutput>;
    fn capture(&self, ctx: &Ctx, input: CaptureInput) -> Result<LineOutput>;
    fn add_task(&self, ctx: &Ctx, input: AddTaskInput) -> Result<TaskOut>;
    fn update_task(&self, ctx: &Ctx, input: UpdateTaskInput) -> Result<TaskOut>;
    fn list_tasks(&self, ctx: &Ctx, input: ListTasksInput) -> Result<ListTasksOutput>;
    fn add_bookmark(&self, ctx: &Ctx, input: AddBookmarkInput) -> Result<BookmarkOut>;
    fn log(&self, ctx: &Ctx, input: LogInput) -> Result<LineOutput>;
    fn vault_info(&self, ctx: &Ctx) -> Result<VaultInfoOutput>;

    /// `vault://AGENTS.md`: the primary vault's agent contract.
    fn agents_md(&self, ctx: &Ctx) -> Result<ResourceText>;

    /// `vault://note/{id}`: the note's whole file, frontmatter included.
    fn note_resource(&self, ctx: &Ctx, id: &str) -> Result<ResourceText>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors() {
        assert_eq!(author_for("claude-code"), "claude");
        assert_eq!(author_for("claude-ai"), "claude");
        assert_eq!(author_for("Cursor"), "cursor");
        assert_eq!(author_for("codex-mcp-client"), "codex");
        assert_eq!(author_for("My Agent 2"), "my-agent-2");
        assert_eq!(author_for("human"), "agent");
        assert_eq!(author_for("HQ"), "agent");
        assert_eq!(author_for(""), "agent");
    }

    #[test]
    fn cancel_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let ctx = Ctx::with_cancel("x", flag.clone());
        assert!(ctx.check().is_ok());
        flag.store(true, Ordering::SeqCst);
        assert!(ctx.is_cancelled());
        assert!(ctx.check().is_err());
    }
}
