//! The agents onboarding knows (SPEC 9, 11): what each one is, the prompt/skill text that
//! teaches it where the brain is and how to write into it, and the writers that connect it
//! to `vault-hq mcp`.
//!
//! Config writers never touch a file without a preview first: `preview` returns a unified
//! diff and the hash of the file it was computed from; `apply` refuses if the file changed
//! since, copies the old file to a timestamped backup, and keeps every other key.
//! Every path comes from `Env`, so tests run against a fake home folder.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::tilde;
use crate::error::{Error, Result};
use crate::locks;
use crate::types::{Agent, ConfigApplied, ConfigDiff, ConnectKind};

/// The MCP server's name in every agent's config.
pub const SERVER_NAME: &str = "vault-hq";

/// Where things are on this Mac. The app passes the real ones; tests pass a temp dir.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
    /// Where `.app` bundles are looked for (`/Applications`).
    pub apps: PathBuf,
    /// The Vault HQ binary agents start as `<exe> mcp`.
    pub exe: PathBuf,
}

/// The vault a prompt is written for.
#[derive(Debug, Clone)]
pub struct PromptVault {
    pub name: String,
    pub path: String,
}

struct Spec {
    id: &'static str,
    name: &'static str,
    blurb: &'static str,
    mcp: bool,
    connect: ConnectKind,
    prompt_kind: &'static str,
    prompt_target: &'static str,
    author: &'static str,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "claude-code",
        name: "Claude Code",
        blurb: "Anthropic's coding agent in the terminal.",
        mcp: true,
        connect: ConnectKind::Command,
        prompt_kind: "skill",
        prompt_target: "Save as ~/.claude/skills/vault-hq-brain/SKILL.md (a personal skill), or paste the body into ~/.claude/CLAUDE.md.",
        author: "claude",
    },
    Spec {
        id: "claude-desktop",
        name: "Claude Desktop",
        blurb: "The Claude app for Mac.",
        mcp: true,
        connect: ConnectKind::File,
        prompt_kind: "instructions",
        prompt_target: "Paste into a Claude Project's instructions, or Settings → Profile → personal preferences.",
        author: "claude",
    },
    Spec {
        id: "cursor",
        name: "Cursor",
        blurb: "The AI code editor.",
        mcp: true,
        connect: ConnectKind::File,
        prompt_kind: "rule",
        prompt_target: "Save as .cursor/rules/vault-hq.mdc in a project, or paste the body into Cursor Settings → Rules → User Rules.",
        author: "cursor",
    },
    Spec {
        id: "codex",
        name: "Codex",
        blurb: "OpenAI's coding agent (CLI and IDE).",
        mcp: true,
        connect: ConnectKind::File,
        prompt_kind: "agents-md",
        prompt_target: "Append to ~/.codex/AGENTS.md (applies to every Codex session).",
        author: "codex",
    },
    Spec {
        id: "chatgpt",
        name: "ChatGPT",
        blurb: "The ChatGPT app. No local MCP, so it hands notes back for you to capture.",
        mcp: false,
        connect: ConnectKind::None,
        prompt_kind: "instructions",
        prompt_target: "Paste into a ChatGPT Project's instructions, or Settings → Personalization → Custom instructions.",
        author: "chatgpt",
    },
    Spec {
        id: "other",
        name: "Another agent",
        blurb: "Any agent or editor that reads instructions and can write files.",
        mcp: false,
        connect: ConnectKind::None,
        prompt_kind: "agents-md",
        prompt_target: "Paste into that tool's instructions or rules file (many read AGENTS.md).",
        author: "agent",
    },
];

fn spec(id: &str) -> Result<&'static Spec> {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| Error::NotFound(format!("No agent called \"{id}\"")))
}

/// The config file HQ edits for `id`, if it edits one.
pub fn config_path(env: &Env, id: &str) -> Option<PathBuf> {
    match id {
        "claude-desktop" => Some(
            env.home
                .join("Library/Application Support/Claude/claude_desktop_config.json"),
        ),
        "cursor" => Some(env.home.join(".cursor/mcp.json")),
        "codex" => Some(env.home.join(".codex/config.toml")),
        _ => None,
    }
}

/// Only existence is checked: nothing inside these folders is read.
fn detected(env: &Env, id: &str) -> bool {
    let h = &env.home;
    let a = &env.apps;
    match id {
        "claude-code" => h.join(".claude").is_dir() || h.join(".claude.json").is_file(),
        "claude-desktop" => {
            a.join("Claude.app").is_dir() || h.join("Library/Application Support/Claude").is_dir()
        }
        "cursor" => a.join("Cursor.app").is_dir() || h.join(".cursor").is_dir(),
        "codex" => h.join(".codex").is_dir(),
        "chatgpt" => a.join("ChatGPT.app").is_dir(),
        _ => false,
    }
}

pub fn catalog(env: &Env) -> Vec<Agent> {
    SPECS
        .iter()
        .map(|s| Agent {
            id: s.id.into(),
            name: s.name.into(),
            blurb: s.blurb.into(),
            mcp: s.mcp,
            connect: s.connect,
            config_path: config_path(env, s.id).map(|p| tilde(&env.home, &p)),
            detected: detected(env, s.id),
            prompt_kind: s.prompt_kind.into(),
            prompt_target: s.prompt_target.into(),
            author: s.author.into(),
        })
        .collect()
}

// ---- the contract text ------------------------------------------------------------------

/// What each standard folder holds. Shared by AGENTS.md, the per-folder notes and prompts.
pub const FOLDERS: &[(&str, &str)] = &[
    (
        "00-Inbox",
        "captures, agent session notes and proposals; nothing is filed without a human look",
    ),
    (
        "01-Projects",
        "one note per project (`type: project`, `status:`), with a `## Tasks` section",
    ),
    ("02-Areas", "ongoing responsibilities with no end date"),
    ("03-Learning", "courses, books, things being learned"),
    ("04-Personal", "personal notes"),
    (
        "05-References",
        "reference notes; `*-findings.md` are findings logs",
    ),
    ("06-Memory", "facts agents should remember about the user"),
    ("07-Agents", "one note per agent: what it may do here"),
    ("08-Archive", "finished or retired notes"),
    (
        "09-Planning",
        "`Daily/YYYY-MM-DD.md`, `Weekly/`, `decision-log.md`, `Tasks.md` (loose tasks)",
    ),
    (
        "10-Bookmarks",
        "`<source>/<slug>.md`, one note per bookmark",
    ),
];

/// The rules every agent follows. `author` is the value this agent writes.
pub fn rules(author: &str) -> String {
    let mut s = String::new();
    s.push_str("## Folders\n\n");
    for (f, what) in FOLDERS {
        s.push_str(&format!("- `{f}/`: {what}\n"));
    }
    s.push_str("- `_templates/`: a template per note type (`{{title}}`, `{{date}}`)\n");
    s.push_str(&format!(
        r#"
## Every note

Starts with frontmatter between `---` lines, one `key: value` per line, lists as `[a, b]`:

```
---
title: A short title
type: note
created: YYYY-MM-DD
updated: YYYY-MM-DD
author: {author}
tags: [topic, other-topic]
---
```

- `type` is one of: note, project, area, reference, findings, daily, capture, session,
  proposal, bookmark, memory, agent, index. Projects also have `status:` (active, paused,
  done, archived).
- `author` is `{author}` on notes you create. Never change the author of a note someone
  else wrote. Set `updated:` to today whenever you change a note.
- File names: lowercase words with dashes (`garden-irrigation.md`); dates as `YYYY-MM-DD`.
- Link notes with `[[file-name]]` (no `.md`).

## How to write here

1. **Inbox first.** New notes, session summaries and proposals go in `00-Inbox/`. A human
   files them. Write straight into another folder only when the user asks you to.
2. **Never delete, move or rename** a file. To retire or reorganise something, write a
   proposal note in `00-Inbox/` (`type: proposal`) saying what and why.
3. **Change existing notes in small steps**: append, or edit the one section you mean to.
   Never rewrite a whole note someone else wrote.
4. **Log what you did** as one line under `## Agent log` in today's
   `09-Planning/Daily/YYYY-MM-DD.md`: `- HH:MM {author} · what you did`.
5. **Tasks** are checkbox lines: `- [ ] Do the thing #tag 📅 YYYY-MM-DD`, in a project's
   `## Tasks` section or in `09-Planning/Tasks.md`.
6. **Remember** durable facts about the user in `06-Memory/` (one topic per note), and read
   it before asking the user something it may already answer.
7. Never put passwords, keys or tokens in the vault.
"#
    ));
    s
}

/// The paragraph about the MCP tools, for agents that can connect.
pub fn mcp_section() -> &'static str {
    r#"
## With the vault-hq MCP server connected

Prefer its tools to editing files: `search`, `ask`, `read_note`, `list_notes`,
`create_note`, `append_to_note` (pass the `base_hash` you read), `capture`, `add_task`,
`update_task`, `list_tasks`, `add_bookmark`, `log`, `vault_info`. They take the same file
locks as the Vault HQ app, so nothing is lost while the user is editing too, and they set
`author` for you. If the tools are not available, edit the files directly by the rules above.
"#
}

fn intro(vault: &PromptVault) -> String {
    format!(
        "The user keeps a second brain: a folder of markdown notes called \"{name}\" at\n\n    {path}\n\n\
         Treat it as long-term memory. Look there before asking the user something it may\n\
         already answer, and write what is worth keeping back into it. The vault's own\n\
         `AGENTS.md` is the contract; if it disagrees with this text, it wins.\n",
        name = vault.name,
        path = vault.path,
    )
}

/// The copy-paste prompt/skill text for one agent and one vault.
pub fn prompt(agent_id: &str, vault: &PromptVault) -> Result<String> {
    let s = spec(agent_id)?;
    let rules = rules(s.author);
    let mcp = if s.mcp { mcp_section() } else { "" };
    let intro = intro(vault);
    let text = match s.id {
        "claude-code" => format!(
            "---\nname: vault-hq-brain\ndescription: The user's second brain at {path}. Use it to remember, look things up, take notes, log work, add tasks or save bookmarks, and whenever the user mentions their notes, vault, brain or memory.\n---\n\n# The user's brain ({name})\n\n{intro}\n{rules}{mcp}",
            path = vault.path,
            name = vault.name,
        ),
        "cursor" => format!(
            "---\ndescription: Where the user's second brain is and how to write into it\nalwaysApply: true\n---\n\n# The user's brain ({name})\n\n{intro}\n{rules}{mcp}",
            name = vault.name,
        ),
        "codex" => format!(
            "## The user's brain ({name})\n\n{intro}\n{rules}{mcp}",
            name = vault.name,
            rules = demote(&rules),
            mcp = demote(mcp),
        ),
        "claude-desktop" => format!(
            "# The user's brain ({name})\n\n{intro}\nWhen the vault-hq tools are connected, use them for everything below. Without them you cannot write files: give me the note as one markdown block, with its frontmatter and the path it belongs at, and I will capture it.\n\n{rules}{mcp}",
            name = vault.name,
        ),
        "chatgpt" => format!(
            "# My brain ({name})\n\n{intro}\nYou cannot write to it directly. When I ask you to save, remember or note something, reply with the complete note in one markdown code block (frontmatter first, `author: chatgpt`) and the path it belongs at, normally `00-Inbox/<file-name>.md`. I paste it in with Vault HQ's capture.\n\n{rules}",
            name = vault.name,
        ),
        _ => format!(
            "# The user's brain ({name})\n\n{intro}\nUse your own name as `author` (lowercase, one word) instead of `agent`.\n\n{rules}",
            name = vault.name,
        ),
    };
    Ok(text)
}

/// Push markdown headings one level down, so a snippet nests under an AGENTS.md section.
fn demote(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.starts_with('#') {
                format!("#{l}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" }
}

// ---- MCP config writers -----------------------------------------------------------------

/// `claude mcp add` for Claude Code: it owns its config, so HQ prints the command.
pub fn claude_code_command(exe: &Path) -> String {
    format!(
        "claude mcp add --scope user {SERVER_NAME} -- {} mcp",
        shell_quote(&exe.to_string_lossy())
    )
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// JSON that keeps its key order, so rewriting a config only shows the lines HQ changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(IndexMap<String, J>),
}

fn server_entry_json(exe: &Path) -> J {
    let mut m = IndexMap::new();
    m.insert("command".into(), J::Str(exe.to_string_lossy().into_owned()));
    m.insert("args".into(), J::Arr(vec![J::Str("mcp".into())]));
    J::Obj(m)
}

/// Claude Desktop and Cursor share the `{"mcpServers": {name: {command, args}}}` shape.
fn edit_json(display: &str, current: &str, exe: &Path) -> Result<(String, Option<String>)> {
    let mut root: IndexMap<String, J> = if current.trim().is_empty() {
        IndexMap::new()
    } else {
        match serde_json::from_str::<J>(current) {
            Ok(J::Obj(m)) => m,
            Ok(_) => {
                return Err(Error::Invalid(format!(
                    "{display} is not a JSON object, so HQ won't change it. Fix it or move it aside, then try again."
                )))
            }
            Err(e) => {
                return Err(Error::Invalid(format!(
                    "{display} isn't valid JSON ({e}), so HQ won't change it. Fix it or move it aside, then try again."
                )))
            }
        }
    };
    let servers = root
        .entry("mcpServers".into())
        .or_insert_with(|| J::Obj(IndexMap::new()));
    let J::Obj(servers) = servers else {
        return Err(Error::Invalid(format!(
            "\"mcpServers\" in {display} is not an object, so HQ won't change it."
        )));
    };
    let want = server_entry_json(exe);
    let note = match servers.get(SERVER_NAME) {
        Some(old) if *old != want => Some(format!(
            "An existing \"{SERVER_NAME}\" entry is replaced (it pointed elsewhere)."
        )),
        _ => None,
    };
    servers.insert(SERVER_NAME.into(), want);
    let mut text = serde_json::to_string_pretty(&J::Obj(root))?;
    text.push('\n');
    if note.is_none() && same_json(current, &text) {
        // already connected: keep the user's formatting untouched
        return Ok((current.to_string(), None));
    }
    Ok((text, note))
}

fn same_json(a: &str, b: &str) -> bool {
    match (
        serde_json::from_str::<serde_json::Value>(a),
        serde_json::from_str::<serde_json::Value>(b),
    ) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Codex: `[mcp_servers.vault-hq]` in `config.toml`, edited with toml_edit so comments,
/// order and formatting elsewhere survive.
fn edit_toml(display: &str, current: &str, exe: &Path) -> Result<(String, Option<String>)> {
    use toml_edit::{value, Array, DocumentMut, InlineTable, Item, Table, Value};

    let mut doc: DocumentMut = current.parse().map_err(|e| {
        Error::Invalid(format!(
            "{display} isn't valid TOML ({e}), so HQ won't change it. Fix it or move it aside, then try again."
        ))
    })?;
    let exe_s = exe.to_string_lossy().into_owned();
    let mut args = Array::new();
    args.push("mcp");

    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    let already = |t: &dyn toml_edit::TableLike| {
        t.get("command").and_then(|c| c.as_str()) == Some(exe_s.as_str())
            && t.get("args")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().map(|v| v.as_str()).collect::<Vec<_>>())
                == Some(vec![Some("mcp")])
    };
    let mut note = None;
    match servers {
        Item::Table(t) => {
            if let Some(existing) = t.get(SERVER_NAME).and_then(|i| i.as_table_like()) {
                if already(existing) {
                    return Ok((current.to_string(), None));
                }
                note = Some(format!(
                    "An existing [mcp_servers.{SERVER_NAME}] entry is replaced (it pointed elsewhere)."
                ));
            }
            let mut entry = Table::new();
            entry["command"] = value(exe_s.clone());
            entry["args"] = value(args);
            t.insert(SERVER_NAME, Item::Table(entry));
        }
        Item::Value(Value::InlineTable(t)) => {
            if let Some(existing) = t.get(SERVER_NAME).and_then(|v| v.as_inline_table()) {
                if already(existing) {
                    return Ok((current.to_string(), None));
                }
                note = Some(format!(
                    "An existing mcp_servers.{SERVER_NAME} entry is replaced (it pointed elsewhere)."
                ));
            }
            let mut entry = InlineTable::new();
            entry.insert("command", exe_s.clone().into());
            entry.insert("args", Value::Array(args));
            t.insert(SERVER_NAME, Value::InlineTable(entry));
        }
        _ => {
            return Err(Error::Invalid(format!(
                "mcp_servers in {display} is not a table, so HQ won't change it."
            )))
        }
    }
    let mut text = doc.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok((text, note))
}

/// The file as it is now: text and hash, or None when it doesn't exist yet.
fn read_current(path: &Path) -> Result<Option<(String, String)>> {
    if !path.exists() {
        return Ok(None);
    }
    locks::read(path).map(Some).map_err(|e| match e {
        Error::Denied(m) => Error::Denied(format!(
            "{m}. Vault HQ needs permission to read {}.",
            path.display()
        )),
        other => other,
    })
}

fn proposed(env: &Env, id: &str, path: &Path, current: &str) -> Result<(String, Option<String>)> {
    let display = tilde(&env.home, path);
    match id {
        "codex" => edit_toml(&display, current, &env.exe),
        _ => edit_json(&display, current, &env.exe),
    }
}

fn unified(path: &str, old: &str, new: &str, exists: bool) -> String {
    if old == new {
        return String::new();
    }
    let a = if exists {
        format!("a/{path}")
    } else {
        "/dev/null".into()
    };
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&a, &format!("b/{path}"))
        .to_string()
}

/// What connecting `agent_id` would change. Writes nothing.
pub fn preview(env: &Env, agent_id: &str) -> Result<ConfigDiff> {
    let s = spec(agent_id)?;
    match s.connect {
        ConnectKind::Command => Ok(ConfigDiff {
            agent_id: s.id.into(),
            kind: ConnectKind::Command,
            path: None,
            exists: false,
            changed: true,
            diff: String::new(),
            command: Some(claude_code_command(&env.exe)),
            base_hash: None,
            note: Some(
                "Claude Code keeps its own MCP list: run this once in a terminal. `claude mcp list` shows it afterwards."
                    .into(),
            ),
        }),
        ConnectKind::None => Err(Error::Invalid(format!(
            "{} can't run a local MCP server; use its prompt text instead.",
            s.name
        ))),
        ConnectKind::File => {
            let path = config_path(env, s.id).ok_or_else(|| {
                Error::Internal(format!("no config path for {}", s.id))
            })?;
            let current = read_current(&path)?;
            let (old, hash) = current.clone().unwrap_or_default();
            let (new, note) = proposed(env, s.id, &path, &old)?;
            let display = tilde(&env.home, &path);
            Ok(ConfigDiff {
                agent_id: s.id.into(),
                kind: ConnectKind::File,
                path: Some(path.to_string_lossy().into_owned()),
                exists: current.is_some(),
                changed: new != old,
                diff: unified(display.trim_start_matches("~/"), &old, &new, current.is_some()),
                command: None,
                base_hash: current.map(|_| hash),
                note,
            })
        }
    }
}

/// Connect `agent_id`: write its config file after the user saw `preview`. `base_hash` is
/// the preview's; a file that changed since is refused rather than overwritten.
pub fn apply(env: &Env, agent_id: &str, base_hash: Option<&str>) -> Result<ConfigApplied> {
    let s = spec(agent_id)?;
    if s.connect != ConnectKind::File {
        return Err(Error::Invalid(format!(
            "HQ doesn't edit {}'s config. {}",
            s.name,
            match s.connect {
                ConnectKind::Command => format!("Run: {}", claude_code_command(&env.exe)),
                _ => "Use its prompt text instead.".into(),
            }
        )));
    }
    let path = config_path(env, s.id)
        .ok_or_else(|| Error::Internal(format!("no config path for {}", s.id)))?;
    let path_s = path.to_string_lossy().into_owned();
    match read_current(&path)? {
        None => {
            if base_hash.is_some() {
                return Err(Error::Invalid(format!(
                    "{} was removed after the preview. Preview again.",
                    tilde(&env.home, &path)
                )));
            }
            let (new, _) = proposed(env, s.id, &path, "")?;
            locks::create_new(&path, &new)?;
            Ok(ConfigApplied {
                agent_id: s.id.into(),
                path: path_s,
                backup: None,
                changed: true,
            })
        }
        Some((old, hash)) => {
            if let Some(base) = base_hash {
                if base != hash {
                    return Err(Error::Conflict {
                        current: crate::error::Current { text: old, hash },
                    });
                }
            }
            let (new, _) = proposed(env, s.id, &path, &old)?;
            if new == old {
                return Ok(ConfigApplied {
                    agent_id: s.id.into(),
                    path: path_s,
                    backup: None,
                    changed: false,
                });
            }
            let backup = backup(&path, &old)?;
            locks::guarded_write(&path, &hash, &new)?;
            Ok(ConfigApplied {
                agent_id: s.id.into(),
                path: path_s,
                backup: Some(backup.to_string_lossy().into_owned()),
                changed: true,
            })
        }
    }
}

/// `<file>.vault-hq-backup-YYYYMMDD-HHMMSS`, never replacing an earlier backup.
fn backup(path: &Path, text: &str) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    for n in 0..100 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let p = path.with_file_name(format!("{name}.vault-hq-backup-{stamp}{suffix}"));
        match locks::create_new(&p, text) {
            Ok(()) => return Ok(p),
            Err(Error::Invalid(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Error::Internal(format!(
        "could not pick a backup name next to {}",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        _dir: tempfile::TempDir,
        env: Env,
    }

    fn fake() -> Fake {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let apps = dir.path().join("Applications");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&apps).unwrap();
        let env = Env {
            home,
            apps,
            exe: PathBuf::from("/Applications/Vault HQ.app/Contents/MacOS/vault-hq"),
        };
        Fake { _dir: dir, env }
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".vault-hq-backup-"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn catalog_detects_by_existence_only() {
        let f = fake();
        let before = catalog(&f.env);
        assert_eq!(before.len(), 6);
        assert!(before.iter().all(|a| !a.detected));
        std::fs::create_dir_all(f.env.home.join(".cursor")).unwrap();
        std::fs::create_dir_all(f.env.apps.join("ChatGPT.app")).unwrap();
        let after = catalog(&f.env);
        let get = |id: &str| after.iter().find(|a| a.id == id).unwrap().clone();
        assert!(get("cursor").detected && get("chatgpt").detected);
        assert_eq!(
            get("cursor").config_path.as_deref(),
            Some("~/.cursor/mcp.json")
        );
        assert_eq!(get("claude-code").connect, ConnectKind::Command);
        assert!(!get("chatgpt").mcp);
    }

    #[test]
    fn prompts_are_tailored() {
        let v = PromptVault {
            name: "home".into(),
            path: "/Vaults/home".into(),
        };
        let cc = prompt("claude-code", &v).unwrap();
        assert!(cc.starts_with("---\nname: vault-hq-brain\n"));
        assert!(cc.contains("/Vaults/home") && cc.contains("author: claude"));
        assert!(cc.contains("append_to_note") && cc.contains("Never delete"));
        let cur = prompt("cursor", &v).unwrap();
        assert!(cur.contains("alwaysApply: true") && cur.contains("author: cursor"));
        let codex = prompt("codex", &v).unwrap();
        assert!(codex.starts_with("## The user's brain"));
        assert!(codex.contains("\n### How to write here"));
        let gpt = prompt("chatgpt", &v).unwrap();
        assert!(gpt.contains("author: chatgpt") && !gpt.contains("append_to_note"));
        assert!(gpt.contains("00-Inbox/"));
        assert!(prompt("nope", &v).is_err());
    }

    #[test]
    fn claude_code_is_a_command() {
        let f = fake();
        let d = preview(&f.env, "claude-code").unwrap();
        assert_eq!(
            d.command.as_deref(),
            Some("claude mcp add --scope user vault-hq -- '/Applications/Vault HQ.app/Contents/MacOS/vault-hq' mcp")
        );
        assert!(apply(&f.env, "claude-code", None).is_err());
        assert!(preview(&f.env, "chatgpt").is_err());
        assert_eq!(shell_quote("/usr/bin/x"), "/usr/bin/x");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn claude_desktop_keeps_other_keys_and_order() {
        let f = fake();
        let p = config_path(&f.env, "claude-desktop").unwrap();
        let original = r#"{
  "preferences": {"zeta": 1, "alpha": [1, 2.5, null, true]},
  "mcpServers": {
    "other": {"command": "/bin/other", "args": [], "env": {"Z": "1", "A": "2"}}
  },
  "globalShortcut": ""
}
"#;
        write(&p, original);
        let d = preview(&f.env, "claude-desktop").unwrap();
        assert!(d.exists && d.changed);
        assert!(d.diff.contains("+    \"vault-hq\": {"), "{}", d.diff);
        assert!(d
            .diff
            .starts_with("--- a/Library/Application Support/Claude/"));
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            original,
            "preview writes nothing"
        );

        let r = apply(&f.env, "claude-desktop", d.base_hash.as_deref()).unwrap();
        assert!(r.changed);
        let backup = r.backup.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);

        let text = std::fs::read_to_string(&p).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["preferences"]["alpha"][1], serde_json::json!(2.5));
        assert_eq!(v["mcpServers"]["other"]["env"]["A"], "2");
        assert_eq!(v["globalShortcut"], "");
        assert_eq!(v["mcpServers"]["vault-hq"]["args"][0], "mcp");
        // order survives: preferences before mcpServers, zeta before alpha, Z before A
        assert!(text.find("preferences").unwrap() < text.find("mcpServers").unwrap());
        assert!(text.find("zeta").unwrap() < text.find("alpha").unwrap());
        assert!(text.find("\"Z\"").unwrap() < text.find("\"A\"").unwrap());

        // idempotent: a second run changes nothing and makes no backup
        let again = preview(&f.env, "claude-desktop").unwrap();
        assert!(!again.changed && again.diff.is_empty());
        let r2 = apply(&f.env, "claude-desktop", again.base_hash.as_deref()).unwrap();
        assert!(!r2.changed && r2.backup.is_none());
        assert_eq!(backups(p.parent().unwrap()).len(), 1);
    }

    #[test]
    fn cursor_missing_file_is_created() {
        let f = fake();
        let d = preview(&f.env, "cursor").unwrap();
        assert!(!d.exists && d.changed && d.base_hash.is_none());
        assert!(d.diff.starts_with("--- /dev/null\n+++ b/.cursor/mcp.json"));
        let r = apply(&f.env, "cursor", None).unwrap();
        assert!(r.backup.is_none());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&r.path).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["vault-hq"]["command"],
            "/Applications/Vault HQ.app/Contents/MacOS/vault-hq"
        );
    }

    #[test]
    fn stale_preview_is_refused() {
        let f = fake();
        let p = config_path(&f.env, "cursor").unwrap();
        write(&p, "{\"mcpServers\": {}}\n");
        let d = preview(&f.env, "cursor").unwrap();
        write(&p, "{\"mcpServers\": {}, \"changed\": true}\n");
        assert!(matches!(
            apply(&f.env, "cursor", d.base_hash.as_deref()),
            Err(Error::Conflict { .. })
        ));
        assert!(backups(p.parent().unwrap()).is_empty());
    }

    #[test]
    fn bad_json_is_never_overwritten() {
        let f = fake();
        let p = config_path(&f.env, "cursor").unwrap();
        write(&p, "{ not json");
        assert!(matches!(preview(&f.env, "cursor"), Err(Error::Invalid(_))));
        assert!(matches!(
            apply(&f.env, "cursor", None),
            Err(Error::Invalid(_))
        ));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
        write(&p, "[1]");
        assert!(preview(&f.env, "cursor").is_err());
        write(&p, "{\"mcpServers\": 3}");
        assert!(preview(&f.env, "cursor").is_err());
    }

    #[test]
    fn old_entry_is_replaced_with_a_note() {
        let f = fake();
        let p = config_path(&f.env, "cursor").unwrap();
        write(
            &p,
            "{\"mcpServers\": {\"vault-hq\": {\"command\": \"/old/vault-hq\", \"args\": [\"mcp\"]}}}",
        );
        let d = preview(&f.env, "cursor").unwrap();
        assert!(d.changed && d.note.is_some());
        assert!(d.diff.contains("-") && d.diff.contains("/old/vault-hq"));
    }

    #[test]
    fn codex_toml_keeps_comments_and_tables() {
        let f = fake();
        let p = config_path(&f.env, "codex").unwrap();
        let original = "# my codex settings\nmodel = \"o4\"  # fast\n\n[mcp_servers.other]\ncommand = \"/bin/other\"\nargs = [\"--x\"]\n\n[profiles.work]\nmodel = \"o3\"\n";
        write(&p, original);
        let d = preview(&f.env, "codex").unwrap();
        assert!(d.changed);
        assert!(d.diff.contains("+[mcp_servers.vault-hq]"), "{}", d.diff);
        apply(&f.env, "codex", d.base_hash.as_deref()).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# my codex settings\nmodel = \"o4\"  # fast\n"));
        assert!(text.contains("[profiles.work]\nmodel = \"o3\""));
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("/bin/other")
        );
        assert_eq!(
            doc["mcp_servers"]["vault-hq"]["command"].as_str(),
            Some("/Applications/Vault HQ.app/Contents/MacOS/vault-hq")
        );
        assert_eq!(
            doc["mcp_servers"]["vault-hq"]["args"][0].as_str(),
            Some("mcp")
        );
        let again = preview(&f.env, "codex").unwrap();
        assert!(!again.changed, "{}", again.diff);
    }

    #[test]
    fn codex_missing_inline_and_broken() {
        let f = fake();
        let p = config_path(&f.env, "codex").unwrap();
        let r = apply(&f.env, "codex", None).unwrap();
        assert!(std::fs::read_to_string(&r.path)
            .unwrap()
            .contains("[mcp_servers.vault-hq]\ncommand = "));

        write(&p, "mcp_servers = { other = { command = \"/bin/o\" } }\n");
        let d = preview(&f.env, "codex").unwrap();
        apply(&f.env, "codex", d.base_hash.as_deref()).unwrap();
        let doc: toml_edit::DocumentMut = std::fs::read_to_string(&p).unwrap().parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("/bin/o")
        );
        assert_eq!(
            doc["mcp_servers"]["vault-hq"]["args"][0].as_str(),
            Some("mcp")
        );

        write(&p, "mcp_servers = 3\n");
        assert!(matches!(preview(&f.env, "codex"), Err(Error::Invalid(_))));
        write(&p, "[[[ nope");
        assert!(matches!(
            apply(&f.env, "codex", None),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn backups_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.json");
        let a = backup(&p, "1").unwrap();
        let b = backup(&p, "2").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(a).unwrap(), "1");
    }
}
