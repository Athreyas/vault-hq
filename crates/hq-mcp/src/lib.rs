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

/// `~/Library/<sub>/local.vaulthq.desktop`, the folders the app's Tauri identifier gets.
fn app_dir(env: &str, sub: &str) -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os(env).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    std::env::var_os("HOME")
        .map(|h| {
            Path::new(&h)
                .join("Library")
                .join(sub)
                .join("local.vaulthq.desktop")
        })
        .ok_or_else(|| "HOME is not set".to_string())
}

/// What `vault-hq mcp` serves, on the app's own state (`hq_core::hq::Hq`):
/// - settings from `VAULT_HQ_SUPPORT_DIR`, else `~/Library/Application Support/local.vaulthq.desktop`;
/// - caches (search vectors) in `VAULT_HQ_CACHE_DIR`, else `~/Library/Caches/local.vaulthq.desktop`;
/// - the vaults in those settings, unless `--vault <folder>` (repeatable; the first is
///   primary) or `VAULT_HQ_VAULT=<folder>` names them. Then the settings' blocklist and
///   model settings still apply, and the settings file is left alone.
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
    let support = app_dir("VAULT_HQ_SUPPORT_DIR", "Application Support")?;
    let cache = app_dir("VAULT_HQ_CACHE_DIR", "Caches")?;
    if paths.is_empty() {
        return Ok(CoreBackend::open(&support, &cache));
    }
    // without readable settings there is no blocklist to honour: refuse rather than guess
    let settings = hq_core::settings::Store::new(&support)
        .load()
        .map_err(|e| e.to_string())?;
    CoreBackend::with_vaults(&paths, settings, &cache).map_err(|e| e.to_string())
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
    let vaults = backend.hq().vaults();
    eprintln!(
        "vault-hq mcp {}: serving {} vault(s): {}",
        env!("CARGO_PKG_VERSION"),
        vaults.len(),
        vaults
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for p in backend.hq().registry().problems() {
        eprintln!("vault-hq mcp: skipped a vault: {p}");
    }
    if let Some(why) = backend.hq().broken() {
        eprintln!("vault-hq mcp: {why}");
    }
    let stdin = std::io::stdin();
    serve(Arc::new(backend), stdin.lock(), Box::new(std::io::stdout()));
    Ok(())
}
