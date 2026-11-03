//! MCP over newline-delimited JSON-RPC 2.0 (revision 2025-06-18, stdio transport).
//!
//! One reader loop takes a line at a time. Everything is answered inline except
//! `tools/call`, which runs on its own thread so a slow `ask` never blocks a `ping` or the
//! `notifications/cancelled` that stops it. Each message goes out as one line under a
//! mutex, so concurrent replies never interleave. Only protocol messages go to the writer;
//! logs go to stderr.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::backend::{Backend, Ctx};
use crate::tools::{self, CallError};
use hq_core::ollama::CancelToken;
use hq_core::Error;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Older revisions whose messages we already speak; a client asking for one gets it.
const SUPPORTED: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// MCP's code for an unknown resource URI.
pub const RESOURCE_NOT_FOUND: i64 = -32002;

const AGENTS_URI: &str = "vault://AGENTS.md";
const NOTE_PREFIX: &str = "vault://note/";

const INSTRUCTIONS: &str = "This server is the user's local second brain (Vault HQ): \
markdown notes, tasks and bookmarks. Start with vault_info, then read the resource \
vault://AGENTS.md, which says what agents may do in this vault. Search before you create, \
so you add to an existing note instead of making a duplicate. New notes go to 00-Inbox \
unless AGENTS.md says otherwise. Edits need the hash from your latest read_note; on a \
Conflict, read again and retry. Nothing can be deleted through this server. Record \
meaningful changes with log.";

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

/// One request's id as the client sent it (a number or a string), usable as a map key.
fn id_key(id: &Value) -> String {
    id.to_string()
}

struct Session {
    /// Set by `initialize`; `None` until then.
    client: Option<String>,
    in_flight: HashMap<String, CancelToken>,
}

pub struct Server {
    backend: Arc<dyn Backend>,
    out: Out,
    session: Arc<Mutex<Session>>,
    workers: Vec<JoinHandle<()>>,
    grace: Duration,
}

/// How long calls still running at EOF get to finish and reply before they are cancelled.
/// A client that writes its requests and closes stdin still wants the answers; one that
/// went away should not keep a slow `ask` running.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // a worker that panicked mid-reply must not take the whole server down with it
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn send(out: &Out, msg: &Value) {
    let mut line = msg.to_string();
    line.push('\n');
    let mut w = lock(out);
    if let Err(e) = w.write_all(line.as_bytes()).and_then(|_| w.flush()) {
        eprintln!("vault-hq mcp: could not write to the client: {e}");
    }
}

fn result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// A tool result for an `hq_core::Error`: the model reads `content`, a program reads
/// `structuredContent.error` (kind, message, and the current hash on a conflict).
fn tool_error(e: &Error) -> Value {
    let mut err = json!({ "kind": e.kind(), "message": e.to_string() });
    let text = match e {
        Error::Conflict { current } => {
            err["current_hash"] = json!(current.hash);
            format!(
                "Conflict: the note changed since you read it, so nothing was written. \
                 Its current hash is {}. Call read_note again, check your change still \
                 makes sense, and retry with that hash as base_hash.",
                current.hash
            )
        }
        Error::Unavailable(m) => format!("Unavailable: {m}"),
        other => format!("{}: {other}", other.kind()),
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": { "error": err },
        "isError": true,
    })
}

fn tool_ok(value: Value) -> Value {
    // structuredContent must be an object; the text copy is for clients that predate it
    let structured = if value.is_object() {
        value
    } else {
        json!({ "result": value })
    };
    json!({
        "content": [{ "type": "text", "text": structured.to_string() }],
        "structuredContent": structured,
        "isError": false,
    })
}

/// `vault://note/<id>` with the id percent-encoded or not (clients differ).
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

impl Server {
    pub fn new(backend: Arc<dyn Backend>, out: Box<dyn Write + Send>) -> Self {
        Server {
            backend,
            out: Arc::new(Mutex::new(out)),
            session: Arc::new(Mutex::new(Session {
                client: None,
                in_flight: HashMap::new(),
            })),
            workers: Vec::new(),
            grace: SHUTDOWN_GRACE,
        }
    }

    /// Change `SHUTDOWN_GRACE` (tests use a short one).
    pub fn shutdown_grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// Read until EOF. Then calls still running get `grace` to finish; after that they
    /// are cancelled (their replies dropped) and waited for.
    pub fn run(mut self, input: impl BufRead) {
        for line in input.lines() {
            match line {
                Ok(l) => self.handle_line(&l),
                Err(e) => {
                    eprintln!("vault-hq mcp: stdin closed: {e}");
                    break;
                }
            }
            self.workers.retain(|w| !w.is_finished());
        }
        let deadline = Instant::now() + self.grace;
        while self.workers.iter().any(|w| !w.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        for flag in lock(&self.session).in_flight.values() {
            flag.cancel();
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }

    pub fn handle_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                send(
                    &self.out,
                    &error(&Value::Null, PARSE_ERROR, format!("Parse error: {e}")),
                );
                return;
            }
        };
        let Some(obj) = msg.as_object() else {
            // 2025-06-18 removed JSON-RPC batching
            send(
                &self.out,
                &error(
                    &Value::Null,
                    INVALID_REQUEST,
                    "Expected one JSON-RPC message per line (batches are not supported)",
                ),
            );
            return;
        };
        let id = obj.get("id").cloned();
        let method = obj.get("method").and_then(Value::as_str);
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            if let Some(id) = &id {
                send(
                    &self.out,
                    &error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\""),
                );
            }
            return;
        }
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        match (id, method) {
            (Some(id), Some(m)) => {
                if !(id.is_string() || id.is_i64() || id.is_u64()) {
                    send(
                        &self.out,
                        &error(
                            &Value::Null,
                            INVALID_REQUEST,
                            "id must be a string or an integer",
                        ),
                    );
                    return;
                }
                self.request(id, m, params)
            }
            (None, Some(m)) => self.notification(m, &params),
            // a response to us: we never send requests, so there is nothing waiting for it
            (Some(_), None) if obj.contains_key("result") || obj.contains_key("error") => {}
            (Some(id), None) => send(&self.out, &error(&id, INVALID_REQUEST, "missing method")),
            (None, None) => {}
        }
    }

    fn notification(&mut self, method: &str, params: &Value) {
        match method {
            "notifications/initialized" => {}
            "notifications/cancelled" => {
                if let Some(rid) = params.get("requestId") {
                    if let Some(flag) = lock(&self.session).in_flight.get(&id_key(rid)) {
                        flag.cancel();
                        eprintln!(
                            "vault-hq mcp: request {rid} cancelled: {}",
                            params
                                .get("reason")
                                .and_then(Value::as_str)
                                .unwrap_or("no reason")
                        );
                    }
                }
            }
            // unknown notifications are ignored, as JSON-RPC requires
            _ => {}
        }
    }

    fn request(&mut self, id: Value, method: &str, params: Value) {
        if method == "ping" {
            return send(&self.out, &result(&id, json!({})));
        }
        if method == "initialize" {
            return self.initialize(&id, &params);
        }
        let client = lock(&self.session).client.clone();
        let Some(client) = client else {
            return send(
                &self.out,
                &error(
                    &id,
                    INVALID_REQUEST,
                    "The server is not initialized: send initialize first",
                ),
            );
        };
        match method {
            "tools/list" => send(
                &self.out,
                &result(&id, json!({ "tools": tools::list_json() })),
            ),
            "tools/call" => self.call_tool(id, &client, params),
            "resources/list" => send(
                &self.out,
                &result(
                    &id,
                    json!({ "resources": [{
                        "uri": AGENTS_URI,
                        "name": "AGENTS.md",
                        "title": "Agent contract",
                        "description": "This vault's rules for agents: what goes where, what you may change. Read it before writing.",
                        "mimeType": "text/markdown",
                    }] }),
                ),
            ),
            "resources/templates/list" => send(
                &self.out,
                &result(
                    &id,
                    json!({ "resourceTemplates": [{
                        "uriTemplate": "vault://note/{id}",
                        "name": "note",
                        "title": "A vault note",
                        "description": "One note by id (\"<vaultId>:<vault-relative path>\", as search and list_notes return), as raw markdown with frontmatter.",
                        "mimeType": "text/markdown",
                    }] }),
                ),
            ),
            "resources/read" => self.read_resource(&id, &client, &params),
            _ => send(
                &self.out,
                &error(&id, METHOD_NOT_FOUND, format!("Method not found: {method}")),
            ),
        }
    }

    fn initialize(&mut self, id: &Value, params: &Value) {
        let Some(requested) = params.get("protocolVersion").and_then(Value::as_str) else {
            return send(
                &self.out,
                &error(id, INVALID_PARAMS, "initialize needs protocolVersion"),
            );
        };
        let version = if SUPPORTED.contains(&requested) {
            requested
        } else {
            PROTOCOL_VERSION
        };
        let client = params
            .pointer("/clientInfo/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let ctx = Ctx::new(&client);
        eprintln!(
            "vault-hq mcp: {} connected (protocol {version}); writes as author \"{}\"",
            if client.is_empty() {
                "a client"
            } else {
                &client
            },
            ctx.author
        );
        lock(&self.session).client = Some(client);
        send(
            &self.out,
            &result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false },
                    },
                    "serverInfo": {
                        "name": "vault-hq",
                        "title": "Vault HQ",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": INSTRUCTIONS,
                }),
            ),
        );
    }

    fn call_tool(&mut self, id: Value, client: &str, params: Value) {
        let Some(name) = params
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return send(
                &self.out,
                &error(&id, INVALID_PARAMS, "tools/call needs a tool name"),
            );
        };
        if tools::find(&name).is_none() {
            return send(
                &self.out,
                &error(&id, INVALID_PARAMS, format!("Unknown tool: {name}")),
            );
        }
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        let key = id_key(&id);
        let flag = CancelToken::new();
        {
            let mut s = lock(&self.session);
            if s.in_flight.contains_key(&key) {
                drop(s);
                return send(
                    &self.out,
                    &error(
                        &id,
                        INVALID_REQUEST,
                        "a request with this id is still running",
                    ),
                );
            }
            s.in_flight.insert(key.clone(), flag.clone());
        }
        let ctx = Ctx::with_cancel(client, flag.clone());
        let backend = self.backend.clone();
        let out = self.out.clone();
        let session = self.session.clone();
        let worker = std::thread::spawn(move || {
            let reply = match tools::call(backend.as_ref(), &ctx, &name, args) {
                Ok(v) => result(&id, tool_ok(v)),
                Err(CallError::Tool(e)) => result(&id, tool_error(&e)),
                Err(CallError::UnknownTool(n)) => {
                    error(&id, INVALID_PARAMS, format!("Unknown tool: {n}"))
                }
            };
            // remove first, so a reused id after this reply is not refused
            lock(&session).in_flight.remove(&key);
            // the spec asks receivers not to answer a cancelled request
            if !flag.is_cancelled() {
                send(&out, &reply);
            }
        });
        self.workers.push(worker);
    }

    fn read_resource(&mut self, id: &Value, client: &str, params: &Value) {
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return send(
                &self.out,
                &error(id, INVALID_PARAMS, "resources/read needs a uri"),
            );
        };
        let ctx = Ctx::new(client);
        let read = if uri == AGENTS_URI {
            self.backend.agents_md(&ctx)
        } else if let Some(raw) = uri.strip_prefix(NOTE_PREFIX) {
            match percent_decode(raw) {
                Some(note_id) => self.backend.note_resource(&ctx, &note_id),
                None => Err(Error::Invalid(format!("Bad percent-encoding in {uri}"))),
            }
        } else {
            return send(
                &self.out,
                &error(id, RESOURCE_NOT_FOUND, format!("Resource not found: {uri}")),
            );
        };
        let reply = match read {
            Ok(r) => result(
                id,
                json!({ "contents": [{ "uri": uri, "mimeType": r.mime_type, "text": r.text }] }),
            ),
            Err(e @ Error::NotFound(_)) => {
                let mut v = error(id, RESOURCE_NOT_FOUND, e.to_string());
                v["error"]["data"] = json!({ "uri": uri });
                v
            }
            Err(e @ Error::Invalid(_)) => error(id, INVALID_PARAMS, e.to_string()),
            Err(e) => error(id, INTERNAL_ERROR, e.to_string()),
        };
        send(&self.out, &reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes() {
        assert_eq!(
            percent_decode("ab12%3A01-Projects%2Fgarden.md").as_deref(),
            Some("ab12:01-Projects/garden.md")
        );
        assert_eq!(percent_decode("a:b/c.md").as_deref(), Some("a:b/c.md"));
        assert_eq!(percent_decode("bad%zz"), None);
        assert_eq!(percent_decode("cut%4"), None);
    }
}
