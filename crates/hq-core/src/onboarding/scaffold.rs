//! A new vault (SPEC 3): the standard folders, the marker, the agent contract (`AGENTS.md`
//! at the root and a short one per folder), a template per note type and a welcome note.
//! Never overwrites: anything already there is left alone and reported as kept, so
//! running it again, or on an existing vault, is safe.

use std::path::Path;

use super::agents::{mcp_section, rules, FOLDERS};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::ScaffoldReport;
use crate::util;
use crate::vault::{Marker, Vault, MARKER};

/// Folders below the standard ones.
const SUBFOLDERS: &[&str] = &["09-Planning/Daily", "09-Planning/Weekly", "_templates"];

pub struct Options<'a> {
    pub name: &'a str,
    /// Add the welcome note (a new vault yes; an imported one no).
    pub welcome: bool,
}

pub fn scaffold(root: &Path, opts: &Options) -> Result<ScaffoldReport> {
    let name = opts.name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("Give the vault a name.".into()));
    }
    if root.exists() && !root.is_dir() {
        return Err(Error::Invalid(format!(
            "{} is a file, not a folder.",
            root.display()
        )));
    }
    std::fs::create_dir_all(root)?;
    let mut created = Vec::new();
    let mut kept = Vec::new();

    // The marker carries the chosen name; Vault::open then reads it (or keeps an old one).
    let marker = root.join(MARKER);
    if marker.exists() {
        kept.push(MARKER.to_string());
    } else {
        let m = Marker {
            format: 1,
            id: util::random_hex(16),
            name: name.to_string(),
            created: util::today(),
        };
        locks::create_new(&marker, &serde_json::to_string_pretty(&m)?)?;
        created.push(MARKER.to_string());
    }
    let vault = Vault::open(root)?;

    for dir in FOLDERS
        .iter()
        .map(|(f, _)| *f)
        .chain(SUBFOLDERS.iter().copied())
    {
        let p = root.join(dir);
        if p.is_dir() {
            kept.push(format!("{dir}/"));
        } else {
            std::fs::create_dir_all(&p)?;
            created.push(format!("{dir}/"));
        }
    }

    let today = util::today();
    let mut files: Vec<(String, String)> =
        vec![("AGENTS.md".into(), root_agents(&vault.name, &today))];
    for (folder, what) in FOLDERS {
        files.push((
            format!("{folder}/AGENTS.md"),
            folder_agents(folder, what, &today),
        ));
    }
    files.push(("09-Planning/Tasks.md".into(), tasks_note(&today)));
    files.push(("09-Planning/decision-log.md".into(), decision_log(&today)));
    for (ty, body) in TEMPLATES {
        files.push((format!("_templates/{ty}.md"), template(ty, body)));
    }
    if opts.welcome {
        files.push((
            "00-Inbox/welcome-to-vault-hq.md".into(),
            welcome(&vault.name, &today),
        ));
    }

    for (rel, text) in files {
        let p = vault.resolve_rel(&rel)?;
        if p.exists() {
            kept.push(rel);
            continue;
        }
        match locks::create_new(&p, &text) {
            Ok(()) => created.push(rel),
            // created by someone else a moment ago: theirs wins
            Err(Error::Invalid(_)) => kept.push(rel),
            Err(e) => return Err(e),
        }
    }

    Ok(ScaffoldReport {
        root: root.to_string_lossy().into_owned(),
        vault_id: vault.id,
        name: vault.name,
        created,
        kept,
    })
}

fn fm(title: &str, ty: &str, date: &str) -> String {
    frontmatter::render(&[
        ("title", title.to_string()),
        ("type", ty.to_string()),
        ("created", date.to_string()),
        ("updated", date.to_string()),
        ("author", "hq".to_string()),
    ])
}

fn root_agents(name: &str, date: &str) -> String {
    format!(
        "{fm}# Agent contract for {name}\n\n\
         This folder is a second brain shared by a human and their AI agents. Every agent\n\
         that reads or writes here follows this file. Use your own name as `author`\n\
         (`claude`, `cursor`, `codex`, …). Each folder has a short `AGENTS.md` of its own.\n\n\
         {rules}{mcp}",
        fm = fm("Agent contract", "index", date),
        rules = rules("<your-name>"),
        mcp = mcp_section(),
    )
}

fn folder_agents(folder: &str, what: &str, date: &str) -> String {
    let extra = match folder {
        "00-Inbox" => "\nAgents write here first. Name files `YYYY-MM-DD-<slug>.md`. A human moves them on.\n",
        "06-Memory" => "\nOne topic per note (`type: memory`). Update a fact in place rather than adding a contradicting one.\n",
        "07-Agents" => "\nOne note per agent (`type: agent`): what it may read, write and decide here.\n",
        "08-Archive" => "\nOnly a human archives. Agents never move notes here.\n",
        "09-Planning" => "\nDaily notes have `## Quick Wins`, `## Work`, `## Learning`, `## Tomorrow` and `## Agent log`. Agents only append to `## Agent log`.\n",
        "10-Bookmarks" => "\nWritten by Vault HQ from your browsers and pasted links. One note per URL.\n",
        _ => "",
    };
    format!(
        "{fm}# {folder}\n\nWhat goes here: {what}.\n{extra}\nThe vault's rules are in [[AGENTS]] at the root.\n",
        fm = fm(&format!("{folder}: agent notes"), "index", date),
    )
}

fn tasks_note(date: &str) -> String {
    format!(
        "{}# Tasks\n\nTasks that belong to no project. One checkbox line each:\n`- [ ] Do the thing #tag 📅 YYYY-MM-DD`.\n\n## Tasks\n",
        fm("Tasks", "note", date)
    )
}

fn decision_log(date: &str) -> String {
    format!(
        "{}# Decision log\n\nOne entry per decision: `## YYYY-MM-DD: what was decided`, then why and what else was considered.\n",
        fm("Decision log", "note", date)
    )
}

/// `(type, body after the frontmatter)`. `{{title}}` and `{{date}}` are filled on create.
const TEMPLATES: &[(&str, &str)] = &[
    ("note", "# {{title}}\n\n"),
    (
        "project",
        "# {{title}}\n\n## Goal\n\n## Notes\n\n## Tasks\n",
    ),
    (
        "area",
        "# {{title}}\n\nWhat this area covers and what \"good\" looks like.\n\n## Notes\n",
    ),
    (
        "reference",
        "# {{title}}\n\n## Summary\n\n## Details\n\n## Sources\n",
    ),
    (
        "findings",
        "# {{title}}\n\nNewest first. One `## YYYY-MM-DD: finding` per entry.\n",
    ),
    (
        "daily",
        "# {{date}}\n\n## Quick Wins\n\n## Work\n\n## Learning\n\n## Tomorrow\n\n## Agent log\n",
    ),
    ("capture", "# Captures {{date}}\n\n"),
    (
        "session",
        "# {{title}}\n\n## What was done\n\n## Decisions\n\n## Open questions\n\n## Next\n",
    ),
    (
        "proposal",
        "# {{title}}\n\n## Proposal\n\n## Why\n\n## What changes\n",
    ),
    (
        "memory",
        "# {{title}}\n\nOne fact or preference per line, with where it came from.\n",
    ),
    (
        "agent",
        "# {{title}}\n\n## May read\n\n## May write\n\n## Never\n",
    ),
    ("bookmark", "# {{title}}\n\n## Notes\n"),
];

fn template(ty: &str, body: &str) -> String {
    let mut props = vec![
        ("title", "{{title}}".to_string()),
        ("type", ty.to_string()),
        ("created", "{{date}}".to_string()),
        ("updated", "{{date}}".to_string()),
        ("author", "human".to_string()),
        ("tags", "[]".to_string()),
    ];
    match ty {
        "project" => props.push(("status", "active".into())),
        "bookmark" => {
            props.push(("url", String::new()));
            props.push(("source", "manual".into()));
            props.push(("added", "{{date}}".into()));
        }
        "daily" => props[0].1 = "{{date}}".into(),
        _ => {}
    }
    format!("{}{body}", frontmatter::render(&props))
}

fn welcome(name: &str, date: &str) -> String {
    format!(
        "{fm}# Welcome to {name}\n\n\
         This is your brain: plain markdown files you, your editors and your AI agents all\n\
         write into. Nothing leaves this Mac.\n\n\
         - **Captures** land in `00-Inbox/`. So do agents' notes and proposals: look through\n  \
         the inbox now and then and file what is worth keeping.\n\
         - **Projects** live in `01-Projects/`, one note each, with their tasks inside.\n\
         - **Your day** is `09-Planning/Daily/`. Agents log what they did there.\n\
         - **Agents** follow [[AGENTS]] at the root. Change it to change how they behave.\n\
         - **Templates** for each kind of note are in `_templates/`.\n\n\
         When you have read this, archive it (move it to `08-Archive/`).\n",
        fm = fm(&format!("Welcome to {name}"), "note", date),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::scan;

    #[test]
    fn creates_everything_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("brain");
        let opts = Options {
            name: "home",
            welcome: true,
        };
        let r = scaffold(&root, &opts).unwrap();
        assert_eq!(r.name, "home");
        assert!(r.kept.is_empty());
        for p in [
            ".vault-hq/vault.json",
            "AGENTS.md",
            "00-Inbox/AGENTS.md",
            "10-Bookmarks/",
            "09-Planning/Daily/",
            "09-Planning/Tasks.md",
            "_templates/project.md",
            "_templates/daily.md",
            "00-Inbox/welcome-to-vault-hq.md",
        ] {
            assert!(r.created.iter().any(|c| c == p), "{p} not created");
        }
        let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(agents.contains("Inbox first") && agents.contains("Never delete"));
        let tpl = std::fs::read_to_string(root.join("_templates/project.md")).unwrap();
        assert!(tpl.contains("status: active") && tpl.contains("## Tasks"));

        // idempotent: same id, nothing new, user edits survive
        std::fs::write(root.join("AGENTS.md"), "mine\n").unwrap();
        let again = scaffold(&root, &opts).unwrap();
        assert_eq!(again.vault_id, r.vault_id);
        assert!(again.created.is_empty(), "{:?}", again.created);
        assert_eq!(again.kept.len(), r.created.len());
        assert_eq!(
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn notes_have_required_frontmatter_and_templates_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        scaffold(
            dir.path(),
            &Options {
                name: "team",
                welcome: true,
            },
        )
        .unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert_eq!(v.name, "team");
        let s = scan(&v);
        assert!(s.notes.iter().all(|n| !n.rel.starts_with("_templates/")));
        for n in &s.notes {
            for k in ["title", "type", "created", "updated", "author"] {
                assert!(n.props.contains_key(k), "{} lacks {k}", n.rel);
            }
        }
    }

    #[test]
    fn existing_vault_keeps_its_marker_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = Vault::open(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("01-Projects")).unwrap();
        std::fs::write(dir.path().join("01-Projects/AGENTS.md"), "theirs").unwrap();
        let r = scaffold(
            dir.path(),
            &Options {
                name: "renamed",
                welcome: false,
            },
        )
        .unwrap();
        assert_eq!(r.vault_id, old.id);
        assert!(r.kept.contains(&"01-Projects/".to_string()));
        assert!(r.kept.contains(&"01-Projects/AGENTS.md".to_string()));
        assert!(!dir.path().join("00-Inbox/welcome-to-vault-hq.md").exists());
    }

    #[test]
    fn refuses_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("file");
        std::fs::write(&f, "x").unwrap();
        let o = Options {
            name: "x",
            welcome: false,
        };
        assert!(scaffold(&f, &o).is_err());
        let blank = Options {
            name: "  ",
            welcome: false,
        };
        assert!(scaffold(dir.path(), &blank).is_err());
    }
}
