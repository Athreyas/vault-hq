// Every command the UI may invoke: SPEC section 10 plus the shell's own. `build.rs`
// includes this file to generate one ACL permission per command (`allow-<name>`), and
// `commands/mod.rs` uses it to answer "not in this build yet" for commands a stream has
// not landed. Add a name here only together with the SPEC change that introduces it.

pub const COMMANDS: &[&str] = &[
    // app (shell)
    "app_status",
    "settings_get",
    "settings_update",
    "reveal",
    // vaults (A)
    "vaults_list",
    "vault_add",
    "vault_create",
    "vault_remove",
    "vault_set_primary",
    "vault_candidates",
    // notes (A)
    "notes_list",
    "note_get",
    "note_source",
    "note_save",
    "note_create",
    "note_move",
    "templates_list",
    // capture (A)
    "capture",
    // home (A)
    "home",
    "days",
    "needs",
    "needs_decide",
    "inbox",
    "inbox_archive",
    // tasks (B)
    "tasks_query",
    "task_add",
    "task_update",
    "task_done",
    "task_cancel",
    "task_delete",
    "task_annotate",
    "task_start",
    "tasks_stats",
    "tasks_import_taskwarrior",
    // search (C)
    "search",
    "ask_start",
    "ask_cancel",
    "conversations_list",
    "conversation_get",
    // ai (C)
    "ollama_status",
    "ollama_recommend",
    // bookmarks (F)
    "browsers_detect",
    "browser_allow",
    "bookmarks_import",
    "bookmarks_query",
    "bookmark_add",
    "bookmarks_enrich",
    "bookmarks_enrich_cancel",
    "memories",
    "memory_dismiss",
    // onboarding (E)
    "onboarding_state",
    "onboarding_set_step",
    "agents_catalog",
    "agent_prompt",
    "agent_config_preview",
    "agent_config_apply",
    "import_analyze",
    "import_apply",
    // widgets (W)
    "widgets_layout_get",
    "widgets_layout_set",
    "widget_data",
    // shell: the quick-capture window
    "capture_open",
    "capture_hide",
];

/// What the quick-capture window may call: capture, nothing else that writes.
pub const CAPTURE_WINDOW: &[&str] = &["capture", "capture_hide", "settings_get"];
