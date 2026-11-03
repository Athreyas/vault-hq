//! MCP stdio server over hq-core (SPEC 9). Owned by stream G.
//!
//! `vault-hq mcp` → [`serve_stdio`]. Agents (Claude Code, Claude Desktop, Cursor, Codex)
//! start that process and speak MCP (JSON-RPC 2.0, one message per line, revision
//! 2025-06-18) on its stdin/stdout. stdout carries protocol messages only; every log line
//! goes to stderr, which clients show in their MCP logs.
//!
//! Layers: `server` (protocol) → `tools` (catalogue, argument checks) → [`Backend`]
//! (one typed method per tool) → [`CoreBackend`] (hq-core).

pub mod backend;
pub mod core_backend;
pub mod server;
pub mod tools;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use backend::{Backend, Ctx};
pub use core_backend::CoreBackend;
pub use server::{Server, PROTOCOL_VERSION};

/// Serve one client on `input`/`output` until `input` ends. The transport-free entry
/// point: tests drive it in memory, `serve_stdio` hands it the process's stdio.
pub fn serve(backend: Arc<dyn Backend>, input: impl BufRead, output: Box<dyn Write + Send>) {
    Server::new(backend, output).run(input);
}

/// Which vaults `vault-hq mcp` serves. In order of precedence:
/// - `--vault <folder>` arguments after `mcp` (repeatable; the first is primary);
/// - `VAULT_HQ_VAULT=<folder>` (one folder; for trying the server without the app);
/// - the app's settings: `VAULT_HQ_SUPPORT_DIR`, else
///   `~/Library/Application Support/local.vaulthq.desktop/settings.json` (SPEC 13).
///
/// The settings' blocklist applies to folders named either way.
pub fn backend_from_env(args: &[String]) -> Result<CoreBackend, String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--vault" => match it.next() {
                Some(p) => paths.push(PathBuf::from(p)),
                None => return Err("--vault needs a folder".into()),
            },
            other => {
                return Err(format!(
                    "unknown argument {other} (usage: vault-hq mcp [--vault <folder>]…)"
                ))
            }
        }
    }
    if paths.is_empty() {
        if let Some(p) = std::env::var_os("VAULT_HQ_VAULT").filter(|p| !p.is_empty()) {
            paths.push(PathBuf::from(p));
        }
    }
    let support = match std::env::var_os("VAULT_HQ_SUPPORT_DIR").filter(|p| !p.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => std::env::var_os("HOME")
            .map(|h| Path::new(&h).join("Library/Application Support/local.vaulthq.desktop"))
            .ok_or("HOME is not set")?,
    };
    let settings = hq_core::settings::Store::new(&support)
        .load()
        .map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Ok(CoreBackend::from_settings(&settings));
    }
    let mut vaults = Vec::new();
    for p in paths {
        if let Some(why) = hq_core::settings::blocked(&settings, &p) {
            return Err(why);
        }
        vaults.push(hq_core::vault::Vault::open(&p).map_err(|e| e.to_string())?);
    }
    Ok(CoreBackend::new(vaults, None))
}

/// `vault-hq mcp [--vault <folder>]…`: serve this process's stdin/stdout until the client
/// disconnects. Reads its own arguments (everything after `mcp`), so `main` only has to
/// spot the subcommand.
pub fn serve_stdio() -> Result<(), String> {
    let args: Vec<String> = std::env::args()
        .skip_while(|a| a != "mcp")
        .skip(1)
        .collect();
    let backend = backend_from_env(&args)?;
    eprintln!(
        "vault-hq mcp {}: serving {} vault(s): {}",
        env!("CARGO_PKG_VERSION"),
        backend.vaults().len(),
        backend
            .vaults()
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let stdin = std::io::stdin();
    serve(Arc::new(backend), stdin.lock(), Box::new(std::io::stdout()));
    Ok(())
}
