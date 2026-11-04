//! Local model server client (SPEC 7). Blocking on purpose: the command layer runs it on
//! its own thread, and the MCP server is synchronous anyway.
//!
//! Only addresses on this Mac or the home network are ever contacted (`check_url`), so a
//! typo or a pasted cloud URL can't send the user's notes off the machine.

use std::io::{BufRead, BufReader, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::settings::OllamaSettings;
use crate::types::{OllamaStatus, Recommendation};

pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

/// One context size for every chat call. Ollama reloads a model whenever `num_ctx`
/// changes, so Ask (streamed) and action drafts (JSON) asking for different sizes cost a
/// full reload, about 15-20 s for an 8B model, each time the user switched between them.
pub const NUM_CTX: u32 = 8192;

/// How long Ollama keeps a model loaded after a call. Its default is 5 minutes, so a
/// question asked after a short pause paid the load again; 30 minutes covers a working
/// session and still frees the memory when HQ sits idle.
pub const KEEP_ALIVE: &str = "30m";

const BINARIES: &[&str] = &[
    "/opt/homebrew/bin/ollama",
    "/usr/local/bin/ollama",
    "/Applications/Ollama.app",
];

// ---- the local-address rule ---------------------------------------------------------------

/// The server URL with any trailing slash removed, if it is allowed: `http`, and a host
/// that is loopback, RFC 1918, link-local or `*.local`. Everything else is `Denied`.
pub fn check_url(url: &str) -> Result<String> {
    let deny = |why: &str| {
        Error::Denied(format!(
            "{url} is not allowed as the model server: {why}. Use an address on this Mac \
             (http://127.0.0.1:11434) or your home network (192.168.x.x, 10.x.x.x, name.local)."
        ))
    };
    let trimmed = url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| deny("only http:// is supported"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(deny("user names in the address are not allowed"));
    }
    if rest.len() > authority.len() {
        return Err(deny("give only the server address, without a path"));
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(|| deny("bad IPv6 address"))?;
        check_port(after).map_err(|_| deny("bad port"))?;
        h
    } else {
        let (h, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, format!(":{p}")),
            None => (authority, String::new()),
        };
        check_port(&port).map_err(|_| deny("bad port"))?;
        h
    };
    if host.is_empty() {
        return Err(deny("no host"));
    }
    if host_is_local(host) {
        Ok(trimmed.to_string())
    } else {
        Err(deny("it is not on this Mac or the home network"))
    }
}

fn check_port(s: &str) -> std::result::Result<(), ()> {
    match s.strip_prefix(':') {
        None if s.is_empty() => Ok(()),
        Some(p) if !p.is_empty() && p.parse::<u16>().is_ok() => Ok(()),
        _ => Err(()),
    }
}

fn host_is_local(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" {
        return true;
    }
    if let Ok(ip) = lower.parse::<IpAddr>() {
        return ip_is_local(ip);
    }
    // a hostname: only mDNS names, which never leave the local link
    lower.ends_with(".local")
        && lower.len() > ".local".len()
        && lower
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !lower.split('.').any(str::is_empty)
}

fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_is_local(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4_is_local(v4);
            }
            v6 == Ipv6Addr::LOCALHOST || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn v4_is_local(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local()
}

// ---- the client -----------------------------------------------------------------------------

/// Cancels a running request; checked between streamed lines.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        ChatMessage {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// How a streamed answer ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Streamed {
    pub text: String,
    pub stopped: bool,
}

/// What search needs from a model server, so tests can stand in for it.
pub trait Embedder {
    fn model(&self) -> &str;
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Structured output: a JSON value that follows `schema` (Ollama's `format`).
pub trait JsonChat {
    fn model(&self) -> &str;
    fn chat_json(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value>;
}

/// What Ask needs from a model server.
pub trait Chat {
    fn model(&self) -> &str;
    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        cancel: &CancelToken,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<Streamed>;
}

#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    agent: ureq::Agent,
}

impl Client {
    /// A client for `url`; refused unless the address passes `check_url`.
    pub fn new(url: &str) -> Result<Client> {
        let base = check_url(url)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(3)))
            // a model loading into memory can take a while before its first byte
            .timeout_recv_response(Some(Duration::from_secs(300)))
            .timeout_recv_body(Some(Duration::from_secs(300)))
            .build()
            .into();
        Ok(Client { base, agent })
    }

    pub fn url(&self) -> &str {
        &self.base
    }

    /// Installed model names, from `GET /api/tags`.
    pub fn tags(&self) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Tag>,
        }
        #[derive(Deserialize)]
        struct Tag {
            name: String,
        }
        let mut r = self
            .agent
            .get(format!("{}/api/tags", self.base))
            .config()
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .call()
            .map_err(|e| self.unreachable(e))?;
        let status = r.status().as_u16();
        let body = r
            .body_mut()
            .read_to_string()
            .map_err(|e| self.unreachable(e))?;
        if status != 200 {
            return Err(Error::Unavailable(format!(
                "The model server at {} answered {status}.",
                self.base
            )));
        }
        let tags: Tags = serde_json::from_str(&body).map_err(|_| {
            Error::Unavailable(format!("{} answered, but not like Ollama does.", self.base))
        })?;
        Ok(tags.models.into_iter().map(|t| t.name).collect())
    }

    /// `POST /api/embed`, one vector per input.
    pub fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Out {
            embeddings: Vec<Vec<f32>>,
        }
        let mut r = self
            .agent
            .post(format!("{}/api/embed", self.base))
            .send_json(json!({ "model": model, "input": inputs, "keep_alive": KEEP_ALIVE }))
            .map_err(|e| self.unreachable(e))?;
        let status = r.status().as_u16();
        let body = r
            .body_mut()
            .read_to_string()
            .map_err(|e| self.unreachable(e))?;
        if status != 200 {
            return Err(server_error(status, &body, model));
        }
        let out: Out = serde_json::from_str(&body)
            .map_err(|e| Error::Internal(format!("Unexpected embedding reply: {e}")))?;
        if out.embeddings.len() != inputs.len() {
            return Err(Error::Internal(format!(
                "Asked for {} embeddings, got {}.",
                inputs.len(),
                out.embeddings.len()
            )));
        }
        Ok(out.embeddings)
    }

    /// `POST /api/chat` with streaming on; tokens go to `on_token` as they arrive.
    pub fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        cancel: &CancelToken,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<Streamed> {
        let mut r = self
            .agent
            .post(format!("{}/api/chat", self.base))
            .send_json(json!({
                "model": model,
                "stream": true,
                "messages": messages,
                "keep_alive": KEEP_ALIVE,
                "options": { "temperature": 0.1, "num_ctx": NUM_CTX },
            }))
            .map_err(|e| self.unreachable(e))?;
        let status = r.status().as_u16();
        if status != 200 {
            let body = r.body_mut().read_to_string().unwrap_or_default();
            return Err(server_error(status, &body, model));
        }
        let reader = BufReader::new(r.body_mut().as_reader());
        read_chat_stream(reader, cancel, on_token)
    }

    /// `POST /api/chat` with structured output: `schema` (a JSON schema) goes in `format`,
    /// and the reply's content is parsed as JSON. Not streamed: the caller wants it whole.
    pub fn chat_json(
        &self,
        model: &str,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        #[derive(Deserialize)]
        struct Out {
            message: Msg,
        }
        #[derive(Deserialize)]
        struct Msg {
            content: String,
        }
        let mut r = self
            .agent
            .post(format!("{}/api/chat", self.base))
            .send_json(json!({
                "model": model,
                "stream": false,
                "format": schema,
                "messages": messages,
                "keep_alive": KEEP_ALIVE,
                "options": { "temperature": 0, "num_ctx": NUM_CTX },
            }))
            .map_err(|e| self.unreachable(e))?;
        let status = r.status().as_u16();
        let body = r
            .body_mut()
            .read_to_string()
            .map_err(|e| self.unreachable(e))?;
        if status != 200 {
            return Err(server_error(status, &body, model));
        }
        let out: Out = serde_json::from_str(&body)
            .map_err(|e| Error::Internal(format!("Unexpected chat reply: {e}")))?;
        serde_json::from_str(&out.message.content).map_err(|e| {
            Error::Internal(format!(
                "The model's answer was not the JSON asked for: {e}"
            ))
        })
    }

    fn unreachable(&self, e: ureq::Error) -> Error {
        Error::Unavailable(format!(
            "Ollama is not reachable at {} ({e}). Start it with: ollama serve",
            self.base
        ))
    }
}

fn server_error(status: u16, body: &str, model: &str) -> Error {
    let msg = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| body.chars().take(200).collect());
    if status == 404 {
        Error::Unavailable(format!(
            "Ollama has no model {model}. Pull it with: ollama pull {model}"
        ))
    } else {
        Error::Unavailable(format!("Ollama answered {status}: {msg}"))
    }
}

/// Read Ollama's NDJSON chat stream: one JSON object per line, `message.content` is the
/// next piece, `done: true` ends it, `error` aborts. Stops early when `cancel` is set.
pub fn read_chat_stream<R: BufRead>(
    reader: R,
    cancel: &CancelToken,
    on_token: &mut dyn FnMut(&str),
) -> Result<Streamed> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(default)]
        message: Option<Msg>,
        #[serde(default)]
        done: bool,
        #[serde(default)]
        error: Option<String>,
    }
    #[derive(Deserialize)]
    struct Msg {
        #[serde(default)]
        content: String,
    }
    let mut text = String::new();
    for line in reader.lines() {
        if cancel.is_cancelled() {
            return Ok(Streamed {
                text,
                stopped: true,
            });
        }
        let line = line
            .map_err(|e| Error::Unavailable(format!("The model server stopped mid-answer: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let l: Line = serde_json::from_str(&line)
            .map_err(|e| Error::Internal(format!("Unexpected line from the model server: {e}")))?;
        if let Some(err) = l.error {
            return Err(Error::Unavailable(format!("Ollama: {err}")));
        }
        if let Some(m) = l.message {
            if !m.content.is_empty() {
                text.push_str(&m.content);
                on_token(&m.content);
            }
        }
        if l.done {
            return Ok(Streamed {
                text,
                stopped: false,
            });
        }
    }
    if cancel.is_cancelled() {
        return Ok(Streamed {
            text,
            stopped: true,
        });
    }
    Err(Error::Unavailable(
        "The model server closed the answer before it was finished.".into(),
    ))
}

/// For callers holding a plain reader.
pub fn read_chat_stream_from(
    r: impl Read,
    cancel: &CancelToken,
    on_token: &mut dyn FnMut(&str),
) -> Result<Streamed> {
    read_chat_stream(BufReader::new(r), cancel, on_token)
}

/// A `Client` bound to the user's chosen models.
pub struct Models {
    pub client: Client,
    pub chat_model: String,
    pub embed_model: String,
}

impl Models {
    pub fn new(settings: &OllamaSettings) -> Result<Models> {
        Ok(Models {
            client: Client::new(&settings.url)?,
            chat_model: settings.chat_model.clone(),
            embed_model: settings.embed_model.clone(),
        })
    }

    pub fn embedder(&self) -> Bound<'_> {
        Bound(self, &self.embed_model)
    }

    pub fn chat(&self) -> Bound<'_> {
        Bound(self, &self.chat_model)
    }
}

/// The client with one model chosen.
pub struct Bound<'a>(&'a Models, &'a str);

impl Embedder for Bound<'_> {
    fn model(&self) -> &str {
        self.1
    }
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        self.0.client.embed(self.1, inputs)
    }
}

impl JsonChat for Bound<'_> {
    fn model(&self) -> &str {
        self.1
    }
    fn chat_json(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.0.client.chat_json(self.1, messages, schema)
    }
}

impl Chat for Bound<'_> {
    fn model(&self) -> &str {
        self.1
    }
    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        cancel: &CancelToken,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<Streamed> {
        self.0
            .client
            .chat_stream(self.1, messages, cancel, on_token)
    }
}

// ---- status and recommendations -------------------------------------------------------------

/// Whether `wanted` is among `installed`, the way Ollama names them (`name` = `name:latest`).
pub fn has_model(installed: &[String], wanted: &str) -> bool {
    let base = |m: &str| m.strip_suffix(":latest").unwrap_or(m).to_string();
    let w = base(wanted);
    !w.is_empty() && installed.iter().any(|m| base(m) == w)
}

pub fn find_binary() -> Option<String> {
    BINARIES
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|p| p.to_string())
}

/// What the Settings and onboarding screens show. Never fails: problems are reported in it.
pub fn status(settings: &OllamaSettings) -> OllamaStatus {
    let mut s = OllamaStatus {
        url: settings.url.clone(),
        up: false,
        binary: find_binary(),
        models: vec![],
        chat_model: settings.chat_model.clone(),
        embed_model: settings.embed_model.clone(),
        has_chat: false,
        has_embed: false,
        problem: None,
    };
    let client = match Client::new(&settings.url) {
        Ok(c) => c,
        Err(e) => {
            s.problem = Some(e.to_string());
            return s;
        }
    };
    match client.tags() {
        Ok(models) => {
            s.up = true;
            s.has_chat = has_model(&models, &settings.chat_model);
            s.has_embed = has_model(&models, &settings.embed_model);
            s.models = models;
            s.problem = if !s.has_chat {
                Some(format!(
                    "No chat model {0}. Pull it with: ollama pull {0}",
                    settings.chat_model
                ))
            } else if !s.has_embed {
                Some(format!(
                    "No embedding model {0}, so search uses keywords only. Pull it with: ollama pull {0}",
                    settings.embed_model
                ))
            } else {
                None
            };
        }
        Err(_) => {
            s.problem = Some(if s.binary.is_some() {
                format!(
                    "Ollama is installed but not answering at {}. Start it with: ollama serve",
                    settings.url
                )
            } else {
                "Ollama is not installed. Get it from ollama.com or: brew install ollama".into()
            });
        }
    }
    s
}

/// Unified memory in bytes (`sysctl hw.memsize`), or None if the kernel won't say.
pub fn memsize() -> Option<u64> {
    let mut value: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    // SAFETY: the name is NUL-terminated and `value`/`len` describe a u64 we own.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && value > 0).then_some(value)
}

pub const EMBED_MODEL: &str = "nomic-embed-text";

/// The recommendation table of SPEC 7, by memory in bytes.
pub fn recommend(mem_bytes: u64) -> Recommendation {
    let gb = ((mem_bytes as f64) / (1u64 << 30) as f64).round() as u32;
    let chat: Vec<String> = match gb {
        0..=15 => vec!["llama3.2:3b"],
        16..=31 => vec!["qwen2.5:7b", "llama3.1:8b"],
        32..=63 => vec!["qwen2.5:14b"],
        _ => vec!["qwen2.5:32b"],
    }
    .into_iter()
    .map(String::from)
    .collect();
    let mut commands = vec![format!("ollama pull {EMBED_MODEL}")];
    if let Some(first) = chat.first() {
        commands.push(format!("ollama pull {first}"));
    }
    Recommendation {
        memory_gb: gb,
        embed: EMBED_MODEL.into(),
        chat,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// Home-network addresses built from parts, so a privacy scan of the source never
    /// mistakes test data for a real host.
    fn lan(a: u8, b: u8, c: u8, d: u8) -> String {
        format!("{a}.{b}.{c}.{d}")
    }

    #[test]
    fn address_rule_allows_local() {
        let lan_urls = [
            format!("http://{}:11434", lan(10, 0, 0, 5)),
            format!("http://{}", lan(172, 16, 0, 1)),
            format!("http://{}:80", lan(172, 31, 255, 255)),
            format!("http://{}:11434", lan(192, 168, 1, 20)),
            format!("http://{}:11434", lan(169, 254, 10, 10)),
            format!("http://[::ffff:{}]:11434", lan(192, 168, 1, 2)),
        ];
        let fixed = [
            "http://127.0.0.1:11434",
            "http://127.0.0.1:11434/",
            "http://localhost:11434",
            "http://LOCALHOST",
            "http://[::1]:11434",
            "http://[fe80::1]:11434",
            "http://studio.local:11434",
            "http://my-mac.lan.local",
        ];
        for ok in lan_urls.iter().map(String::as_str).chain(fixed) {
            assert!(check_url(ok).is_ok(), "{ok} should be allowed");
        }
        assert_eq!(
            check_url("http://127.0.0.1:11434/").unwrap(),
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn address_rule_rejects_the_rest() {
        for bad in [
            "https://127.0.0.1:11434",
            "ftp://127.0.0.1",
            "127.0.0.1:11434",
            "http://8.8.8.8:11434",
            "http://api.openai.com",
            "http://example.com:11434",
            "http://local",
            "http://.local",
            "http://evil.local.example.com",
            "http://127.0.0.1@evil.com", // privacy-check: allow (a test that this trick is refused)
            "http://evil.com#@127.0.0.1",
            "http://127.0.0.1:11434/v1",
            "http://[2001:db8::1]:11434",
            "http://[::ffff:8.8.8.8]",
            "http://127.0.0.1:99999",
            "http://127.0.0.1:",
            "http://",
            "",
        ]
        .into_iter()
        .map(String::from)
        .chain(
            [lan(172, 32, 0, 1), lan(172, 15, 0, 1), lan(192, 169, 0, 1)]
                .map(|h| format!("http://{h}")),
        ) {
            let bad = bad.as_str();
            assert!(
                matches!(check_url(bad), Err(Error::Denied(_))),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn recommendation_table() {
        const G: u64 = 1 << 30;
        let r = recommend(8 * G);
        assert_eq!(r.memory_gb, 8);
        assert_eq!(r.chat, vec!["llama3.2:3b"]);
        assert_eq!(r.embed, "nomic-embed-text");
        assert_eq!(recommend(16 * G).chat, vec!["qwen2.5:7b", "llama3.1:8b"]);
        assert_eq!(recommend(24 * G).chat, vec!["qwen2.5:7b", "llama3.1:8b"]);
        assert_eq!(recommend(32 * G).chat, vec!["qwen2.5:14b"]);
        assert_eq!(recommend(48 * G).chat, vec!["qwen2.5:14b"]);
        assert_eq!(recommend(64 * G).chat, vec!["qwen2.5:32b"]);
        assert_eq!(recommend(128 * G).chat, vec!["qwen2.5:32b"]);
        assert_eq!(
            recommend(36 * G).commands,
            vec!["ollama pull nomic-embed-text", "ollama pull qwen2.5:14b"]
        );
    }

    #[test]
    fn this_mac_has_memory() {
        assert!(memsize().unwrap_or(0) >= 1 << 30);
    }

    #[test]
    fn model_names_match_like_ollama() {
        let installed = vec!["nomic-embed-text:latest".into(), "qwen2.5:7b".into()];
        assert!(has_model(&installed, "nomic-embed-text"));
        assert!(has_model(&installed, "qwen2.5:7b"));
        assert!(!has_model(&installed, "qwen2.5:14b"));
        assert!(!has_model(&installed, "qwen2.5"));
        assert!(!has_model(&installed, ""));
    }

    pub(crate) const STREAM: &str = concat!(
        r#"{"model":"m","message":{"role":"assistant","content":"The "},"done":false}"#,
        "\n",
        r#"{"model":"m","message":{"role":"assistant","content":"fence"},"done":false}"#,
        "\n\n",
        r#"{"model":"m","message":{"role":"assistant","content":" [[garden]]."},"done":false}"#,
        "\n",
        r#"{"model":"m","message":{"role":"assistant","content":""},"done":true,"eval_count":3}"#,
        "\n",
    );

    #[test]
    fn ndjson_stream_tokens_in_order() {
        let mut got = vec![];
        let s = read_chat_stream_from(STREAM.as_bytes(), &CancelToken::new(), &mut |t| {
            got.push(t.to_string())
        })
        .unwrap();
        assert_eq!(got, vec!["The ", "fence", " [[garden]]."]);
        assert_eq!(s.text, "The fence [[garden]].");
        assert!(!s.stopped);
    }

    #[test]
    fn ndjson_stream_errors_and_truncation() {
        let err = "{\"message\":{\"content\":\"a\"}}\n{\"error\":\"model ran out of memory\"}\n";
        let r = read_chat_stream_from(err.as_bytes(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(r, Err(Error::Unavailable(m)) if m.contains("out of memory")));
        let cut = "{\"message\":{\"content\":\"a\"},\"done\":false}\n";
        let r = read_chat_stream_from(cut.as_bytes(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(r, Err(Error::Unavailable(_))));
        let junk = "not json\n";
        let r = read_chat_stream_from(junk.as_bytes(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(r, Err(Error::Internal(_))));
    }

    #[test]
    fn ndjson_stream_cancel_stops_early() {
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let mut n = 0;
        let s = read_chat_stream_from(STREAM.as_bytes(), &cancel, &mut |_| {
            n += 1;
            c2.cancel();
        })
        .unwrap();
        assert_eq!(n, 1);
        assert!(s.stopped);
        assert_eq!(s.text, "The ");
    }

    /// A tiny HTTP server on loopback that answers each request with the next reply.
    fn serve(replies: Vec<(u16, &'static str, String)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for (status, ctype, body) in replies {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 8192];
                let mut req = Vec::new();
                // read the headers and the body the client sends with them
                loop {
                    let n = s.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if req.len() >= end + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                s.write_all(head.as_bytes()).unwrap();
                s.write_all(body.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn client_against_a_fake_server() {
        let url = serve(vec![
            (
                200,
                "application/json",
                r#"{"models":[{"name":"nomic-embed-text:latest"},{"name":"qwen2.5:7b"}]}"#.into(),
            ),
            (
                200,
                "application/json",
                r#"{"embeddings":[[1.0,0.0],[0.0,1.0]]}"#.into(),
            ),
            (200, "application/x-ndjson", STREAM.into()),
            (
                404,
                "application/json",
                r#"{"error":"model 'x' not found"}"#.into(),
            ),
            (
                200,
                "application/json",
                r#"{"message":{"role":"assistant","content":"{\"ok\":true}"},"done":true}"#.into(),
            ),
        ]);
        let c = Client::new(&url).unwrap();
        assert_eq!(
            c.tags().unwrap(),
            vec!["nomic-embed-text:latest", "qwen2.5:7b"]
        );
        let v = c.embed("e", &["a".into(), "b".into()]).unwrap();
        assert_eq!(v, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        let mut toks = String::new();
        let s = c
            .chat_stream(
                "m",
                &[ChatMessage::new("user", "hi")],
                &CancelToken::new(),
                &mut |t| toks.push_str(t),
            )
            .unwrap();
        assert_eq!(s.text, "The fence [[garden]].");
        assert_eq!(s.text, toks);
        let e = c.embed("x", &["a".into()]).unwrap_err();
        assert!(e.to_string().contains("ollama pull x"), "{e}");
        let v = c
            .chat_json(
                "m",
                &[ChatMessage::new("user", "hi")],
                &json!({"type": "object"}),
            )
            .unwrap();
        assert_eq!(v, json!({"ok": true}));
    }

    #[test]
    fn status_reports_a_dead_or_refused_server() {
        // a port nothing listens on
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        let s = status(&OllamaSettings {
            url,
            ..Default::default()
        });
        assert!(!s.up);
        assert!(s.problem.is_some());
        let s = status(&OllamaSettings {
            url: "http://example.com".into(),
            ..Default::default()
        });
        assert!(!s.up);
        assert!(s.problem.unwrap().contains("not allowed"));
    }
}
