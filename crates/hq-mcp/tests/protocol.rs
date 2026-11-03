//! The whole server over an in-memory transport: handshake, catalogue, argument checks,
//! errors, resources, cancellation and a conflicting write, against a temp vault.

use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use hq_core::vault::Vault;
use hq_core::Result;
use hq_mcp::backend::*;
use hq_mcp::{CoreBackend, Server};
use serde_json::{json, Value};

// ---- transport --------------------------------------------------------------------------

/// The server's stdin: bytes the test sends; EOF when the test drops its sender.
struct Inbound {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for Inbound {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos == self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// The server's stdout, cut into lines.
struct Outbound {
    tx: Sender<String>,
    buf: Vec<u8>,
}

impl Write for Outbound {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        while let Some(i) = self.buf.iter().position(|c| *c == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let _ = self.tx.send(String::from_utf8(line).expect("utf-8 out"));
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Client {
    tx: Option<Sender<Vec<u8>>>,
    rx: Receiver<String>,
    server: Option<JoinHandle<()>>,
}

impl Client {
    fn start(backend: Arc<dyn Backend>) -> Client {
        let (in_tx, in_rx) = channel();
        let (out_tx, out_rx) = channel();
        let server = std::thread::spawn(move || {
            let input = BufReader::new(Inbound {
                rx: in_rx,
                buf: Vec::new(),
                pos: 0,
            });
            let out = Box::new(Outbound {
                tx: out_tx,
                buf: Vec::new(),
            });
            Server::new(backend, out)
                .shutdown_grace(Duration::from_millis(300))
                .run(input);
        });
        Client {
            tx: Some(in_tx),
            rx: out_rx,
            server: Some(server),
        }
    }

    fn raw(&self, line: &str) {
        let mut b = line.as_bytes().to_vec();
        b.push(b'\n');
        self.tx.as_ref().unwrap().send(b).unwrap();
    }

    fn send(&self, msg: Value) {
        self.raw(&msg.to_string());
    }

    fn recv(&self) -> Value {
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(10))
            .expect("a reply from the server");
        assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
        serde_json::from_str(&line).expect("one JSON message per line")
    }

    fn silent_for(&self, ms: u64) {
        if let Ok(line) = self.rx.recv_timeout(Duration::from_millis(ms)) {
            panic!("expected no message, got {line}");
        }
    }

    fn request(&self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let r = self.recv();
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], id, "{r}");
        r
    }

    fn init(backend: Arc<dyn Backend>) -> Client {
        let c = Client::start(backend);
        let r = c.request(
            0,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "claude-code", "version": "2.0.0" }
            }),
        );
        assert!(r.get("result").is_some(), "{r}");
        c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        c
    }

    /// `tools/call` → the tool result (`content`, `structuredContent`, `isError`).
    fn call(&self, id: i64, name: &str, args: Value) -> Value {
        let r = self.request(id, "tools/call", json!({ "name": name, "arguments": args }));
        assert!(r.get("error").is_none(), "{name}: protocol error {r}");
        let res = r["result"].clone();
        assert!(res["content"][0]["text"].is_string(), "{res}");
        res
    }

    fn ok(&self, id: i64, name: &str, args: Value) -> Value {
        let r = self.call(id, name, args);
        assert_eq!(r["isError"], false, "{name}: {r}");
        r["structuredContent"].clone()
    }

    fn close(mut self) {
        drop(self.tx.take());
        self.server
            .take()
            .unwrap()
            .join()
            .expect("server exits on EOF");
    }
}

// ---- fixtures ---------------------------------------------------------------------------

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let write = |rel: &str, text: &str| {
        let p = r.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    write(
        "01-Projects/garden.md",
        "---\ntitle: Garden\ntype: project\ncreated: 2026-01-01\nupdated: 2026-10-01\nauthor: human\ntags: [garden, outdoor]\n---\n\n# Garden\n\nDrip irrigation along the fence.\n\n## Notes\n\n- buy hose\n\n## Tasks\n\n- [ ] lay the line\n",
    );
    write(
        "05-References/watering.md",
        "---\ntitle: Watering\ntype: reference\ncreated: 2026-02-01\nupdated: 2026-09-01\nauthor: human\n---\n\nEarly morning watering for tomatoes.\n",
    );
    write("loose.md", "no frontmatter, mentions irrigation once\n");
    write(
        "AGENTS.md",
        "# Agents\n\nNew notes go to 00-Inbox. Never delete.\n",
    );
    write(".obsidian/hidden.md", "not a note\n");
    let v = Vault::open(r).unwrap();
    (dir, v)
}

fn core() -> (tempfile::TempDir, Vault, Arc<dyn Backend>) {
    let (d, v) = vault();
    let b: Arc<dyn Backend> = Arc::new(CoreBackend::new(vec![v.clone()], None));
    (d, v, b)
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

// ---- handshake and protocol -------------------------------------------------------------

#[test]
fn handshake() {
    let (_d, _v, b) = core();
    let c = Client::start(b);
    let r = c.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "roots": { "listChanged": true } },
            "clientInfo": { "name": "claude-ai", "version": "1" }
        }),
    );
    let res = &r["result"];
    assert_eq!(res["protocolVersion"], "2025-06-18");
    assert_eq!(res["serverInfo"]["name"], "vault-hq");
    assert!(res["serverInfo"]["version"].is_string());
    assert_eq!(res["capabilities"]["tools"]["listChanged"], false);
    assert_eq!(res["capabilities"]["resources"]["subscribe"], false);
    assert!(res["instructions"]
        .as_str()
        .unwrap()
        .contains("vault://AGENTS.md"));
    // notifications get no reply
    c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    c.send(json!({ "jsonrpc": "2.0", "method": "notifications/something_new", "params": {} }));
    c.silent_for(150);
    let p = c.request(2, "ping", json!(null));
    assert_eq!(p["result"], json!({}));
    // a string id comes back as a string
    c.send(json!({ "jsonrpc": "2.0", "id": "abc", "method": "ping" }));
    assert_eq!(c.recv()["id"], "abc");
    c.close();
}

#[test]
fn version_negotiation() {
    let (_d, _v, b) = core();
    let c = Client::start(b.clone());
    let r = c.request(
        1,
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "x" } }),
    );
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    c.close();
    let c = Client::start(b.clone());
    let r = c.request(
        1,
        "initialize",
        json!({ "protocolVersion": "1999-01-01", "capabilities": {}, "clientInfo": { "name": "x" } }),
    );
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    c.close();
    let c = Client::start(b);
    let r = c.request(1, "initialize", json!({ "capabilities": {} }));
    assert_eq!(r["error"]["code"], -32602);
    c.close();
}

#[test]
fn requests_before_initialize_are_refused_but_ping_works() {
    let (_d, _v, b) = core();
    let c = Client::start(b);
    let r = c.request(1, "tools/list", json!({}));
    assert_eq!(r["error"]["code"], -32600);
    assert!(r["error"]["message"]
        .as_str()
        .unwrap()
        .contains("initialize"));
    assert_eq!(c.request(2, "ping", json!({}))["result"], json!({}));
    c.close();
}

#[test]
fn json_rpc_errors() {
    let (_d, _v, b) = core();
    let c = Client::init(b);
    c.raw("{not json");
    let r = c.recv();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);
    c.raw(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#);
    assert_eq!(c.recv()["error"]["code"], -32600);
    c.raw(r#"{"jsonrpc":"1.0","id":5,"method":"ping"}"#);
    assert_eq!(c.recv()["error"]["code"], -32600);
    c.raw(r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#);
    assert_eq!(c.recv()["error"]["code"], -32600);
    c.raw("");
    c.raw("   ");
    c.silent_for(100);
    let r = c.request(6, "prompts/list", json!({}));
    assert_eq!(r["error"]["code"], -32601);
    let r = c.request(
        7,
        "tools/call",
        json!({ "name": "delete_note", "arguments": {} }),
    );
    assert_eq!(
        r["error"]["code"], -32602,
        "unknown tools are protocol errors"
    );
    let r = c.request(8, "tools/call", json!({ "arguments": {} }));
    assert_eq!(r["error"]["code"], -32602);
    // still alive
    assert_eq!(c.request(9, "ping", json!({}))["result"], json!({}));
    c.close();
}

#[test]
fn tools_list() {
    let (_d, _v, b) = core();
    let c = Client::init(b);
    let r = c.request(1, "tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "add_bookmark",
            "add_task",
            "append_to_note",
            "ask",
            "capture",
            "create_note",
            "list_notes",
            "list_tasks",
            "log",
            "read_note",
            "search",
            "update_task",
            "vault_info"
        ]
    );
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(t["description"].as_str().unwrap().len() > 40);
        assert_eq!(t["annotations"]["destructiveHint"], false);
        assert!(!t["name"].as_str().unwrap().contains("delete"));
    }
    let read_only: Vec<&str> = tools
        .iter()
        .filter(|t| t["annotations"]["readOnlyHint"] == true)
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(read_only.contains(&"search") && !read_only.contains(&"capture"));
    c.close();
}

// ---- argument validation ----------------------------------------------------------------

#[test]
fn argument_validation() {
    let (_d, v, b) = core();
    let id = format!("{}:01-Projects/garden.md", v.id);
    let hash = "0".repeat(64);
    let cases: Vec<(&str, Value, &str)> = vec![
        ("vault_info", json!({ "x": 1 }), "unknown field"),
        ("vault_info", json!([1]), "JSON object"),
        ("search", json!({}), "missing field `query`"),
        ("search", json!({ "query": "  " }), "must not be empty"),
        ("search", json!({ "query": 5 }), "invalid type"),
        (
            "search",
            json!({ "query": "a", "limit": 0 }),
            "between 1 and 50",
        ),
        (
            "search",
            json!({ "query": "a", "limit": 51 }),
            "between 1 and 50",
        ),
        (
            "search",
            json!({ "query": "a", "scope": { "days": 0 } }),
            "scope.days",
        ),
        (
            "search",
            json!({ "query": "a", "scope": { "kind": "pics" } }),
            "unknown variant",
        ),
        (
            "search",
            json!({ "query": "a", "scope": { "when": 1 } }),
            "unknown field",
        ),
        ("ask", json!({ "question": "" }), "must not be empty"),
        ("read_note", json!({}), "exactly one of `id` or `ref`"),
        (
            "read_note",
            json!({ "id": id, "ref": "garden" }),
            "exactly one",
        ),
        ("read_note", json!({ "id": "garden.md" }), "note id like"),
        (
            "list_notes",
            json!({ "updated_since": "yesterday" }),
            "YYYY-MM-DD",
        ),
        ("list_notes", json!({ "limit": 501 }), "between 1 and 500"),
        ("list_notes", json!({ "offset": -1 }), "invalid value"),
        ("create_note", json!({}), "missing field `title`"),
        ("create_note", json!({ "title": "a\nb" }), "single line"),
        (
            "create_note",
            json!({ "title": "x", "tags": ["has space"] }),
            "is not a tag",
        ),
        (
            "create_note",
            json!({ "title": "x", "body": "y".repeat(100_001) }),
            "longer than",
        ),
        (
            "append_to_note",
            json!({ "id": id, "text": "x" }),
            "missing field `base_hash`",
        ),
        (
            "append_to_note",
            json!({ "id": id, "text": "x", "base_hash": "abc" }),
            "64-character",
        ),
        (
            "append_to_note",
            json!({ "id": id, "text": "", "base_hash": hash }),
            "must not be empty",
        ),
        (
            "append_to_note",
            json!({ "id": "nope", "text": "x", "base_hash": hash }),
            "note id",
        ),
        ("capture", json!({ "text": "" }), "must not be empty"),
        (
            "capture",
            json!({ "text": "x".repeat(2001) }),
            "longer than 2000",
        ),
        ("log", json!({ "text": "a\nb" }), "single line"),
        (
            "add_task",
            json!({ "description": "x", "due": "2026-02-30x" }),
            "YYYY-MM-DD",
        ),
        (
            "add_task",
            json!({ "description": "x", "priority": "urgent" }),
            "unknown variant",
        ),
        (
            "update_task",
            json!({ "id": "a1b2c3", "patch": {} }),
            "at least one field",
        ),
        (
            "update_task",
            json!({ "id": "TOOLONG1", "patch": { "status": "done" } }),
            "6-character",
        ),
        (
            "update_task",
            json!({ "id": "a1b2c3", "patch": { "status": "finished" } }),
            "unknown variant",
        ),
        (
            "update_task",
            json!({ "id": "a1b2c3", "patch": { "due": "soon" } }),
            "patch.due",
        ),
        (
            "list_tasks",
            json!({ "status": ["open"] }),
            "unknown variant",
        ),
        (
            "list_tasks",
            json!({ "due_before": "2026/01/01" }),
            "YYYY-MM-DD",
        ),
        (
            "add_bookmark",
            json!({ "url": "ftp://x.example/a" }),
            "http(s) URL",
        ),
        (
            "add_bookmark",
            json!({ "url": "https://a b.example" }),
            "http(s) URL",
        ),
        ("add_bookmark", json!({}), "missing field `url`"),
    ];
    let c = Client::init(b);
    for (i, (tool, args, want)) in cases.iter().enumerate() {
        let r = c.call(i as i64 + 10, tool, args.clone());
        assert_eq!(r["isError"], true, "{tool} {args}: {r}");
        assert_eq!(
            r["structuredContent"]["error"]["kind"], "Invalid",
            "{tool} {args}: {r}"
        );
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains(want),
            "{tool} {args}: {text:?} lacks {want:?}"
        );
    }
    // `arguments` left out means {}
    let r = c.request(200, "tools/call", json!({ "name": "vault_info" }));
    assert_eq!(r["result"]["isError"], false, "{r}");
    c.close();
}

// ---- tools on a real vault --------------------------------------------------------------

#[test]
fn vault_info_and_listing() {
    let (_d, v, b) = core();
    let c = Client::init(b);
    let info = c.ok(1, "vault_info", json!({}));
    assert_eq!(info["author"], "claude");
    let vi = &info["vaults"][0];
    assert_eq!(vi["id"], v.id);
    assert_eq!(vi["primary"], true);
    assert_eq!(vi["notes"], 4);
    assert_eq!(vi["has_agents_md"], true);
    assert_eq!(vi["folders"], json!(["01-Projects", "05-References"]));

    let all = c.ok(2, "list_notes", json!({}));
    assert_eq!(all["total"], 4);
    let first = &all["notes"][0];
    // newest first: loose.md and AGENTS.md carry today's mtime
    assert!(first["updated"].as_str().unwrap() > "2026-10-01");

    let projects = c.ok(
        3,
        "list_notes",
        json!({ "folder": "01-Projects/", "type": "project" }),
    );
    assert_eq!(projects["total"], 1);
    assert_eq!(projects["notes"][0]["title"], "Garden");
    assert_eq!(projects["notes"][0]["tags"], json!(["garden", "outdoor"]));

    let since = c.ok(
        4,
        "list_notes",
        json!({ "updated_since": "2026-09-15", "folder": "0" }),
    );
    assert_eq!(since["total"], 0, "folder matches whole names only");
    let since = c.ok(
        5,
        "list_notes",
        json!({ "updated_since": "2026-09-15", "type": "reference" }),
    );
    assert_eq!(since["total"], 0);

    let page = c.ok(6, "list_notes", json!({ "limit": 3 }));
    assert_eq!(page["notes"].as_array().unwrap().len(), 3);
    assert_eq!(page["next_offset"], 3);
    let rest = c.ok(7, "list_notes", json!({ "limit": 3, "offset": 3 }));
    assert_eq!(rest["notes"].as_array().unwrap().len(), 1);
    assert!(rest.get("next_offset").is_none());

    let r = c.call(8, "list_notes", json!({ "vault": "nope" }));
    assert_eq!(r["structuredContent"]["error"]["kind"], "NotFound");
    c.close();
}

#[test]
fn read_by_id_and_ref() {
    let (_d, v, b) = core();
    let c = Client::init(b);
    let id = format!("{}:01-Projects/garden.md", v.id);
    let n = c.ok(1, "read_note", json!({ "id": id }));
    assert_eq!(n["title"], "Garden");
    assert_eq!(n["props"]["type"], "project");
    assert!(n["body"].as_str().unwrap().starts_with("\n# Garden"));
    assert_eq!(n["hash"].as_str().unwrap().len(), 64);
    for r in [
        "garden",
        "[[Garden#Notes|the garden]]",
        "01-Projects/garden",
        "01-PROJECTS/Garden.md",
    ] {
        let byref = c.ok(2, "read_note", json!({ "ref": r }));
        assert_eq!(byref["id"], id, "{r}");
    }
    let r = c.call(3, "read_note", json!({ "ref": "nothing-like-it" }));
    assert_eq!(r["structuredContent"]["error"]["kind"], "NotFound");
    let r = c.call(
        4,
        "read_note",
        json!({ "id": format!("{}:../etc/passwd", v.id) }),
    );
    assert_eq!(r["structuredContent"]["error"]["kind"], "Denied");
    let r = c.call(
        5,
        "read_note",
        json!({ "id": format!("{}:.obsidian/hidden.md", v.id) }),
    );
    assert_eq!(r["structuredContent"]["error"]["kind"], "Denied");
    let r = c.call(
        6,
        "read_note",
        json!({ "id": format!("{}:.vault-hq/vault.json", v.id) }),
    );
    assert_eq!(r["isError"], true);
    c.close();
}

#[test]
fn search_keywords() {
    let (_d, v, b) = core();
    let c = Client::init(b);
    let s = c.ok(1, "search", json!({ "query": "irrigation" }));
    let hits = s["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0]["id"], format!("{}:01-Projects/garden.md", v.id));
    assert!(hits[0]["snippet"].as_str().unwrap().contains("irrigation"));
    let s = c.ok(2, "search", json!({ "query": "irrigation", "limit": 1 }));
    assert_eq!(s["hits"].as_array().unwrap().len(), 1);
    let s = c.ok(
        3,
        "search",
        json!({ "query": "irrigation", "scope": { "kind": "bookmarks" } }),
    );
    assert_eq!(s["hits"].as_array().unwrap().len(), 0);
    let s = c.ok(4, "search", json!({ "query": "watering tomatoes" }));
    assert_eq!(s["hits"][0]["title"], "Watering");
    c.close();
}

#[test]
fn create_append_and_author() {
    let (d, v, b) = core();
    let c = Client::init(b);
    let w = c.ok(
        1,
        "create_note",
        json!({ "title": "Session: garden plan", "type": "session", "body": "Talked about hoses.", "tags": ["#garden"] }),
    );
    let rel = "00-Inbox/session-garden-plan.md";
    assert_eq!(w["id"], format!("{}:{rel}", v.id));
    let text = read(d.path(), rel);
    assert!(text.starts_with("---\ntitle: Session: garden plan\ntype: session\n"));
    assert!(text.contains("author: claude\n"));
    assert!(text.contains("tags: [garden]\n"));
    assert!(text.ends_with("---\n\nTalked about hoses.\n"));
    assert_eq!(w["hash"], hq_core::locks::sha256(&text));

    // never overwrites
    let r = c.call(2, "create_note", json!({ "title": "Session: garden plan" }));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("already exists"));
    // not into hidden folders or templates
    let r = c.call(
        3,
        "create_note",
        json!({ "title": "x", "folder": ".vault-hq" }),
    );
    assert_eq!(r["structuredContent"]["error"]["kind"], "Denied");
    let r = c.call(
        4,
        "create_note",
        json!({ "title": "x", "folder": "_templates" }),
    );
    assert_eq!(r["structuredContent"]["error"]["kind"], "Denied");
    let r = c.call(
        5,
        "create_note",
        json!({ "title": "x", "folder": "../outside" }),
    );
    assert_eq!(r["structuredContent"]["error"]["kind"], "Denied");

    // append under a heading with the hash we were given
    let id = format!("{}:01-Projects/garden.md", v.id);
    let n = c.ok(6, "read_note", json!({ "id": id }));
    let a = c.ok(
        7,
        "append_to_note",
        json!({ "id": id, "heading": "Notes", "text": "- check pressure", "base_hash": n["hash"] }),
    );
    let text = read(d.path(), "01-Projects/garden.md");
    assert!(text.contains("## Notes\n\n- buy hose\n- check pressure\n\n## Tasks"));
    assert!(text.contains(&format!("updated: {}\n", hq_core::util::today())));
    assert_eq!(a["hash"], hq_core::locks::sha256(&text));
    // the new hash chains into the next write
    c.ok(
        8,
        "append_to_note",
        json!({ "id": id, "text": "Closing thought.", "base_hash": a["hash"] }),
    );
    assert!(
        read(d.path(), "01-Projects/garden.md").ends_with("- [ ] lay the line\nClosing thought.\n")
    );
    c.close();
}

#[test]
fn conflicting_write_is_a_tool_error() {
    let (d, v, b) = core();
    let c = Client::init(b);
    let id = format!("{}:05-References/watering.md", v.id);
    let n = c.ok(1, "read_note", json!({ "id": id }));
    // someone edits the note after the agent read it
    let p = d.path().join("05-References/watering.md");
    let edited = read(d.path(), "05-References/watering.md") + "Human edit.\n";
    std::fs::write(&p, &edited).unwrap();

    let r = c.call(
        2,
        "append_to_note",
        json!({ "id": id, "text": "Agent edit.", "base_hash": n["hash"] }),
    );
    assert_eq!(r["isError"], true);
    let err = &r["structuredContent"]["error"];
    assert_eq!(err["kind"], "Conflict");
    assert_eq!(err["current_hash"], hq_core::locks::sha256(&edited));
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("read_note again"));
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        edited,
        "nothing written"
    );

    // the server is fine, and the retry with the current hash works
    assert_eq!(c.request(3, "ping", json!({}))["result"], json!({}));
    c.ok(
        4,
        "append_to_note",
        json!({ "id": id, "text": "Agent edit.", "base_hash": err["current_hash"] }),
    );
    assert!(read(d.path(), "05-References/watering.md").ends_with("Human edit.\nAgent edit.\n"));
    c.close();
}

#[test]
fn capture_and_log() {
    let (d, v, b) = core();
    let c = Client::init(b);
    let today = hq_core::util::today();
    let cap = c.ok(1, "capture", json!({ "text": "Try mulch\nunder the beds" }));
    let rel = format!("00-Inbox/{today}-capture.md");
    assert_eq!(cap["id"], format!("{}:{rel}", v.id));
    c.ok(2, "capture", json!({ "text": "second" }));
    let text = read(d.path(), &rel);
    assert!(text.starts_with("---\ntitle: Capture "));
    assert!(text.contains("type: capture\n") && text.contains("author: claude\n"));
    assert!(text.contains(" claude · Try mulch\n  under the beds\n- "));
    assert!(text.ends_with(" claude · second\n"));

    let l = c.ok(3, "log", json!({ "text": "filed one capture" }));
    c.ok(4, "log", json!({ "text": "and another" }));
    let rel = format!("09-Planning/Daily/{today}.md");
    assert_eq!(l["id"], format!("{}:{rel}", v.id));
    let text = read(d.path(), &rel);
    assert!(text.contains("type: daily\n"));
    let log = text.split("## Agent log\n").nth(1).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].ends_with(" claude · filed one capture"));
    assert!(lines[1].ends_with(" claude · and another"));
    assert!(text.contains("## Tomorrow\n\n## Agent log"));
    c.close();
}

#[test]
fn log_into_an_existing_daily_note() {
    let (d, _v, b) = core();
    let today = hq_core::util::today();
    let rel = format!("09-Planning/Daily/{today}.md");
    let p = d.path().join(&rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "---\ntitle: today\nupdated: 2020-01-01\n---\n\n## Work\n\n- shipped\n\n## Agent log\n\n- 08:00 codex · earlier\n\n## Tomorrow\n\n- rest\n",
    )
    .unwrap();
    let c = Client::init(b);
    c.ok(1, "log", json!({ "text": "later" }));
    let text = read(d.path(), &rel);
    assert!(text.contains("- 08:00 codex · earlier\n- "), "{text}");
    assert!(
        text.contains(" claude · later\n\n## Tomorrow\n\n- rest\n"),
        "{text}"
    );
    assert!(text.contains(&format!("updated: {today}")));
    c.close();
}

#[test]
fn unwired_tools_say_unavailable() {
    let (_d, _v, b) = core();
    let c = Client::init(b);
    for (tool, args) in [
        ("ask", json!({ "question": "what about the garden?" })),
        ("add_task", json!({ "description": "water" })),
        (
            "update_task",
            json!({ "id": "a1b2c3", "patch": { "status": "done" } }),
        ),
        ("list_tasks", json!({})),
        ("add_bookmark", json!({ "url": "https://example.com/a" })),
    ] {
        let r = c.call(1, tool, args);
        assert_eq!(r["isError"], true, "{tool}");
        assert_eq!(
            r["structuredContent"]["error"]["kind"], "Unavailable",
            "{tool}"
        );
    }
    c.close();
}

#[test]
fn no_vault_configured() {
    let b: Arc<dyn Backend> = Arc::new(CoreBackend::new(vec![], None));
    let c = Client::init(b);
    let r = c.call(1, "capture", json!({ "text": "x" }));
    assert_eq!(r["structuredContent"]["error"]["kind"], "Unavailable");
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Open Vault HQ"));
    let info = c.ok(2, "vault_info", json!({}));
    assert_eq!(info["vaults"], json!([]));
    c.close();
}

// ---- resources --------------------------------------------------------------------------

#[test]
fn resources() {
    let (d, v, b) = core();
    let c = Client::init(b);
    let r = c.request(1, "resources/list", json!({}));
    assert_eq!(r["result"]["resources"][0]["uri"], "vault://AGENTS.md");
    let r = c.request(2, "resources/templates/list", json!({}));
    assert_eq!(
        r["result"]["resourceTemplates"][0]["uriTemplate"],
        "vault://note/{id}"
    );
    let r = c.request(3, "resources/read", json!({ "uri": "vault://AGENTS.md" }));
    let content = &r["result"]["contents"][0];
    assert_eq!(content["uri"], "vault://AGENTS.md");
    assert_eq!(content["mimeType"], "text/markdown");
    assert!(content["text"].as_str().unwrap().contains("Never delete"));

    let plain = format!("vault://note/{}:01-Projects/garden.md", v.id);
    let encoded = format!("vault://note/{}%3A01-Projects%2Fgarden.md", v.id);
    for uri in [plain, encoded] {
        let r = c.request(4, "resources/read", json!({ "uri": uri }));
        let t = r["result"]["contents"][0]["text"].as_str().unwrap();
        assert_eq!(t, read(d.path(), "01-Projects/garden.md"), "{uri}");
    }
    let r = c.request(
        5,
        "resources/read",
        json!({ "uri": format!("vault://note/{}:missing.md", v.id) }),
    );
    assert_eq!(r["error"]["code"], -32002);
    let r = c.request(6, "resources/read", json!({ "uri": "file:///etc/hosts" }));
    assert_eq!(r["error"]["code"], -32002);
    let r = c.request(7, "resources/read", json!({}));
    assert_eq!(r["error"]["code"], -32602);

    std::fs::remove_file(d.path().join("AGENTS.md")).unwrap();
    let r = c.request(8, "resources/read", json!({ "uri": "vault://AGENTS.md" }));
    assert_eq!(r["error"]["code"], -32002);
    assert!(r["error"]["message"].as_str().unwrap().contains("00-Inbox"));
    c.close();
}

// ---- cancellation -----------------------------------------------------------------------

/// CoreBackend, except `ask` waits until it is cancelled.
struct SlowAsk {
    inner: CoreBackend,
    started: std::sync::Mutex<Sender<()>>,
    saw_cancel: Arc<AtomicBool>,
}

impl Backend for SlowAsk {
    fn ask(&self, ctx: &Ctx, _: AskInput) -> Result<AskOutput> {
        self.started.lock().unwrap().send(()).unwrap();
        for _ in 0..4000 {
            if ctx.is_cancelled() {
                self.saw_cancel.store(true, Ordering::SeqCst);
                ctx.check()?;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(AskOutput {
            answer: "too late".into(),
            sources: vec![],
        })
    }
    fn search(&self, c: &Ctx, i: SearchInput) -> Result<SearchOutput> {
        self.inner.search(c, i)
    }
    fn read_note(&self, c: &Ctx, i: ReadNoteInput) -> Result<NoteOut> {
        self.inner.read_note(c, i)
    }
    fn list_notes(&self, c: &Ctx, i: ListNotesInput) -> Result<ListNotesOutput> {
        self.inner.list_notes(c, i)
    }
    fn create_note(&self, c: &Ctx, i: CreateNoteInput) -> Result<WriteOutput> {
        self.inner.create_note(c, i)
    }
    fn append_to_note(&self, c: &Ctx, i: AppendInput) -> Result<WriteOutput> {
        self.inner.append_to_note(c, i)
    }
    fn capture(&self, c: &Ctx, i: CaptureInput) -> Result<LineOutput> {
        self.inner.capture(c, i)
    }
    fn add_task(&self, c: &Ctx, i: AddTaskInput) -> Result<TaskOut> {
        self.inner.add_task(c, i)
    }
    fn update_task(&self, c: &Ctx, i: UpdateTaskInput) -> Result<TaskOut> {
        self.inner.update_task(c, i)
    }
    fn list_tasks(&self, c: &Ctx, i: ListTasksInput) -> Result<ListTasksOutput> {
        self.inner.list_tasks(c, i)
    }
    fn add_bookmark(&self, c: &Ctx, i: AddBookmarkInput) -> Result<BookmarkOut> {
        self.inner.add_bookmark(c, i)
    }
    fn log(&self, c: &Ctx, i: LogInput) -> Result<LineOutput> {
        self.inner.log(c, i)
    }
    fn vault_info(&self, c: &Ctx) -> Result<VaultInfoOutput> {
        self.inner.vault_info(c)
    }
    fn agents_md(&self, c: &Ctx) -> Result<ResourceText> {
        self.inner.agents_md(c)
    }
    fn note_resource(&self, c: &Ctx, id: &str) -> Result<ResourceText> {
        self.inner.note_resource(c, id)
    }
}

fn slow() -> (
    tempfile::TempDir,
    Arc<dyn Backend>,
    Receiver<()>,
    Arc<AtomicBool>,
) {
    let (d, v) = vault();
    let (tx, rx) = channel();
    let saw = Arc::new(AtomicBool::new(false));
    let b: Arc<dyn Backend> = Arc::new(SlowAsk {
        inner: CoreBackend::new(vec![v], None),
        started: std::sync::Mutex::new(tx),
        saw_cancel: saw.clone(),
    });
    (d, b, rx, saw)
}

#[test]
fn cancellation_stops_the_call_and_sends_nothing() {
    let (_d, b, started, saw) = slow();
    let c = Client::init(b);
    c.send(json!({ "jsonrpc": "2.0", "id": 42, "method": "tools/call",
        "params": { "name": "ask", "arguments": { "question": "slow one" } } }));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    // other requests are answered while the call runs
    assert_eq!(c.request(1, "ping", json!({}))["result"], json!({}));
    let info = c.ok(2, "vault_info", json!({}));
    assert_eq!(info["author"], "claude");
    // cancelling an unknown id is harmless
    c.send(
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": 999 } }),
    );
    c.send(
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": 42, "reason": "user pressed stop" } }),
    );
    for _ in 0..200 {
        if saw.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw.load(Ordering::SeqCst), "the backend saw the cancel");
    c.silent_for(300);
    // the id can be reused once the cancelled call is gone
    assert_eq!(c.request(42, "ping", json!({}))["result"], json!({}));
    c.close();
}

#[test]
fn disconnect_cancels_running_calls() {
    let (_d, b, started, saw) = slow();
    let c = Client::init(b);
    c.send(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": { "name": "ask", "arguments": { "question": "slow" } } }));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    c.close(); // joins the server, which must not wait out the slow call
    assert!(saw.load(Ordering::SeqCst));
}

#[test]
fn replies_still_come_after_stdin_closes() {
    let (d, _v, b) = core();
    let c = Client::init(b);
    c.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "log", "arguments": { "text": "piped" } } }));
    let Client { tx, rx, server } = c;
    drop(tx);
    let r: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["isError"], false, "{r}");
    server.unwrap().join().unwrap();
    let today = hq_core::util::today();
    assert!(read(d.path(), &format!("09-Planning/Daily/{today}.md")).contains(" · piped\n"));
}

#[test]
fn duplicate_in_flight_id_is_refused() {
    let (_d, b, started, _saw) = slow();
    let c = Client::init(b);
    let call = json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": { "name": "ask", "arguments": { "question": "slow" } } });
    c.send(call.clone());
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    c.send(call);
    let r = c.recv();
    assert_eq!(r["id"], 9);
    assert_eq!(r["error"]["code"], -32600);
    c.send(
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": 9 } }),
    );
    c.silent_for(200);
    c.close();
}
