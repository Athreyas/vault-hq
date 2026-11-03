//! The MCP server on a throwaway vault, for trying it by hand:
//!
//! ```sh
//! npx @modelcontextprotocol/inspector \
//!   cargo run -q -p hq-mcp --example demo_server
//! ```
//!
//! Pass a folder to serve that instead (`… --example demo_server -- /tmp/hq-demo`), e.g.
//! one made with `node demo/make-demo.mjs /tmp/hq-demo`. The temp vault is deleted when
//! the inspector disconnects. Logs, including the vault's path, go to stderr.

use std::sync::Arc;

use hq_core::settings::Settings;
use hq_mcp::{serve, CoreBackend};

fn seed(root: &std::path::Path) -> std::io::Result<()> {
    let files = [
        (
            "AGENTS.md",
            "# Agents in this vault\n\n- New notes go to 00-Inbox; the human files them.\n\
             - Never delete anything.\n- Log what you change with the log tool.\n",
        ),
        (
            "01-Projects/garden.md",
            "---\ntitle: Garden\ntype: project\ncreated: 2026-10-01\nupdated: 2026-10-20\n\
             author: human\nstatus: active\ntags: [garden]\n---\n\n# Garden\n\n\
             Drip irrigation along the fence.\n\n## Notes\n\n- buy a pressure reducer\n\n\
             ## Tasks\n\n- [ ] Lay the main line #outdoor ⏫ 📅 2026-11-08 🆔 a1b2c3\n",
        ),
        (
            "05-References/tomatoes.md",
            "---\ntitle: Tomatoes\ntype: reference\ncreated: 2026-09-01\nupdated: 2026-09-01\n\
             author: human\n---\n\nWater early in the morning, at the roots.\n",
        ),
        (
            "06-Memory/preferences.md",
            "---\ntitle: Preferences\ntype: memory\ncreated: 2026-08-01\nupdated: 2026-08-01\n\
             author: human\n---\n\n- Prefers metric units.\n",
        ),
    ];
    for (rel, text) in files {
        let p = root.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, text)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp;
    let root = match std::env::args().nth(1) {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            temp = tempfile::tempdir()?;
            seed(temp.path())?;
            temp.path().to_path_buf()
        }
    };
    let cache = tempfile::tempdir()?;
    // default settings: Ask and semantic search use a local Ollama if one is running
    let backend = CoreBackend::with_vaults(
        std::slice::from_ref(&root),
        Settings::default(),
        cache.path(),
    )?;
    for v in backend.hq().vaults() {
        eprintln!(
            "demo_server: serving vault {} ({}) at {}",
            v.name, v.id, v.path
        );
    }
    let stdin = std::io::stdin();
    serve(Arc::new(backend), stdin.lock(), Box::new(std::io::stdout()));
    Ok(())
}
