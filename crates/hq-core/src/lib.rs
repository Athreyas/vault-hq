//! Vault HQ's core: everything the app and the MCP server do with a vault, with no UI.
//! See docs/v2/SPEC.md. Each module names the workstream that owns it.

pub mod error;
pub mod frontmatter;
pub mod locks;
pub mod settings;
pub mod types;
pub mod util;

// ---- workstream modules: add new modules below their stream's line only ----
// A core
pub mod capture;
pub mod days;
pub mod home;
pub mod hq;
pub mod inbox;
pub mod needs;
pub mod notes;
pub mod vault;
// B tasks
pub mod tasks;
// C search
pub mod ollama;
pub mod search;
// E onboarding
pub mod onboarding;
// F bookmarks
pub mod bookmarks;
// W widgets
pub mod widgets;

pub use error::{Error, Result};
