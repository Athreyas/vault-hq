//! What the MCP tools call. One method per tool, typed in and out, so the protocol layer
//! never touches the vault and tests can run it against a fake.
//!
//! Inputs are the MCP argument shapes (checked strictly: unknown fields are errors).
//! Outputs are the core's own types where one exists (`NoteView`, `Task`, `TaskGroups`,
//! `SearchResult`, `Bookmark`), so an agent sees the same data the app shows.
//! `CoreBackend` (core_backend.rs) implements this over `hq_core::hq::Hq`.

use hq_core::ollama::CancelToken;
use hq_core::types::{
    Bookmark, NoteSummary, NoteView, Priority, SearchHit, SearchResult, Task, TaskGroups,
    TaskPatch, TaskQuery, TaskStatus,
};
use hq_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Per-call context: who is writing, and whether the client has given up on the call.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The `author:` value writes use, derived from `clientInfo.name` (see `author_for`).
    pub author: String,
    /// `clientInfo.name` exactly as the client sent it, for logs.
    pub client: String,
    cancel: CancelToken,
}

impl Ctx {
    pub fn new(client: &str) -> Self {
        Ctx {
            author: author_for(client),
            client: client.to_string(),
            cancel: CancelToken::new(),
        }
    }

    pub(crate) fn with_cancel(client: &str, cancel: CancelToken) -> Self {
        Ctx {
            cancel,
            ..Ctx::new(client)
        }
    }

    /// True once the client sent `notifications/cancelled` for this call (or disconnected).
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The same flag as the core's token, so a model request stops between tokens.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
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
    pub text: Option<String>,
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
    pub template: Option<String>,
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

/// A new task: stream B's `TaskInput`, field for field (all one word, so the MCP and the
/// core spellings agree).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddTaskInput {
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// A project note's name (its file stem).
    pub project: Option<String>,
    /// Put the task in this note's `## Tasks` section (a note id).
    pub note: Option<String>,
    pub vault: Option<String>,
    pub priority: Option<Priority>,
    pub start: Option<String>,
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub recur: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub waiting: bool,
}

/// `update_task`: B's `TaskPatch` for the fields, plus what B has separate calls for:
/// `status` done/cancelled (B's `done`/`cancel`, which handle recurrence) and `annotate`.
#[derive(Debug, Clone, Default)]
pub struct UpdateTaskInput {
    /// The task's key: its `🆔`, or the content key `list_tasks` returned.
    pub id: String,
    pub patch: TaskPatch,
    pub status: Option<TaskStatus>,
    pub annotate: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListTasksInput {
    pub query: TaskQuery,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddBookmarkInput {
    pub url: String,
    pub note: Option<String>,
    pub vault: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogInput {
    pub text: String,
    pub vault: Option<String>,
}

// ---- outputs ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AskOutput {
    pub answer: String,
    pub model: String,
    /// The notes the answer was drawn from.
    pub sources: Vec<SearchHit>,
    /// False when embeddings were unavailable and sources were ranked by keywords only.
    pub semantic: bool,
}

#[derive(Debug, Clone, Serialize)]
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
pub struct BookmarkOutput {
    pub bookmark: Bookmark,
    /// False when the URL was already saved; `bookmark` is then the existing note.
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VaultSummary {
    pub id: String,
    pub name: String,
    pub path: String,
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
    fn search(&self, ctx: &Ctx, input: SearchInput) -> Result<SearchResult>;
    fn ask(&self, ctx: &Ctx, input: AskInput) -> Result<AskOutput>;
    fn read_note(&self, ctx: &Ctx, input: ReadNoteInput) -> Result<NoteView>;
    fn list_notes(&self, ctx: &Ctx, input: ListNotesInput) -> Result<ListNotesOutput>;
    fn create_note(&self, ctx: &Ctx, input: CreateNoteInput) -> Result<NoteView>;
    fn append_to_note(&self, ctx: &Ctx, input: AppendInput) -> Result<WriteOutput>;
    fn capture(&self, ctx: &Ctx, input: CaptureInput) -> Result<LineOutput>;
    fn add_task(&self, ctx: &Ctx, input: AddTaskInput) -> Result<Task>;
    fn update_task(&self, ctx: &Ctx, input: UpdateTaskInput) -> Result<Task>;
    fn list_tasks(&self, ctx: &Ctx, input: ListTasksInput) -> Result<TaskGroups>;
    fn add_bookmark(&self, ctx: &Ctx, input: AddBookmarkInput) -> Result<BookmarkOutput>;
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
        let token = CancelToken::new();
        let ctx = Ctx::with_cancel("x", token.clone());
        assert!(ctx.check().is_ok());
        token.cancel();
        assert!(ctx.is_cancelled());
        assert!(ctx.cancel_token().is_cancelled());
        assert!(ctx.check().is_err());
    }
}
