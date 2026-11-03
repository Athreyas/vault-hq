//! Conversations as notes (SPEC 6): `06-Memory/conversations/YYYY-MM-DD-<slug>.md`,
//! `type: conversation`, `author: human`. The note is the record, so a conversation is
//! searchable knowledge and survives the app being deleted.
//!
//! Each turn is two level-2 headings the parser keys on:
//! ```text
//! ## You · 2026-11-02 14:05
//! <question>
//!
//! ## HQ · qwen2.5:7b
//! <answer>
//!
//! Sources: [[garden]] · [[fence]]
//! ```
//! Headings of level 1–2 inside a question or answer are pushed down to level 3, so text
//! can never be mistaken for a turn boundary.

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{note_id, split_note_id, Conversation, ConversationSummary, Turn};
use crate::util;
use crate::vault::Vault;

pub const FOLDER: &str = "06-Memory/conversations";
const YOU: &str = "## You · ";
const HQ: &str = "## HQ · ";
const SOURCES: &str = "Sources: ";
const STOPPED: &str = "_(stopped)_";

/// Turns sent back to the model with a follow-up.
pub const FOLLOW_UP_TURNS: usize = 4;

fn demote(text: &str) -> String {
    text.trim()
        .lines()
        .map(|l| {
            let h = l.bytes().take_while(|b| *b == b'#').count();
            if (1..=2).contains(&h) && l[h..].starts_with(' ') {
                format!("###{}", &l[h..])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The markdown of one turn, ready to append.
pub fn render_turn(t: &Turn, stopped: bool) -> String {
    let mut s = format!(
        "\n{YOU}{}\n\n{}\n\n{HQ}{}\n\n",
        t.at,
        demote(&t.question),
        t.model
    );
    let answer = demote(&t.answer);
    if !answer.is_empty() {
        s.push_str(&answer);
        s.push('\n');
    }
    if stopped {
        if !answer.is_empty() {
            s.push('\n');
        }
        s.push_str(STOPPED);
        s.push('\n');
    }
    if !t.sources.is_empty() {
        let links: Vec<String> = t.sources.iter().map(|s| format!("[[{s}]]")).collect();
        s.push_str(&format!("\n{SOURCES}{}\n", links.join(" · ")));
    }
    s
}

/// Turns parsed back out of a conversation note's body.
pub fn parse_turns(body: &str) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    // 0: before any turn, 1: in a question, 2: in an answer
    let mut state = 0;
    let mut q: Vec<&str> = vec![];
    let mut a: Vec<&str> = vec![];
    let finish = |turns: &mut Vec<Turn>, q: &mut Vec<&str>, a: &mut Vec<&str>| {
        if let Some(t) = turns.last_mut() {
            t.question = q.join("\n").trim().to_string();
            let mut answer_lines: Vec<&str> = vec![];
            for l in a.iter() {
                if let Some(src) = l.strip_prefix(SOURCES) {
                    t.sources = src
                        .split('·')
                        .filter_map(|x| {
                            x.trim()
                                .strip_prefix("[[")
                                .and_then(|x| x.strip_suffix("]]"))
                                .map(String::from)
                        })
                        .collect();
                } else if l.trim() != STOPPED {
                    answer_lines.push(l);
                }
            }
            t.answer = answer_lines.join("\n").trim().to_string();
        }
        q.clear();
        a.clear();
    };
    for line in body.lines() {
        if let Some(at) = line.strip_prefix(YOU) {
            if state != 0 {
                finish(&mut turns, &mut q, &mut a);
            }
            turns.push(Turn {
                at: at.trim().to_string(),
                ..Default::default()
            });
            state = 1;
        } else if let Some(model) = line.strip_prefix(HQ).filter(|_| state == 1) {
            if let Some(t) = turns.last_mut() {
                t.model = model.trim().to_string();
            }
            state = 2;
        } else if state == 1 {
            q.push(line);
        } else if state == 2 {
            a.push(line);
        }
    }
    if state != 0 {
        finish(&mut turns, &mut q, &mut a);
    }
    turns
}

fn title_for(question: &str) -> String {
    let one_line = question.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = one_line.chars().take(80).collect();
    // a title is a frontmatter value on one line: keep it plain
    t.replace([':', '#', '[', ']'], " ").trim().to_string()
}

/// Start a conversation note with its first turn. Returns its id.
pub fn create(vault: &Vault, first: &Turn, stopped: bool) -> Result<String> {
    let today = util::today();
    let slug = util::slug(&first.question, 48);
    let title = title_for(&first.question);
    let head = frontmatter::render(&[
        (
            "title",
            if title.is_empty() {
                "Conversation".into()
            } else {
                title.clone()
            },
        ),
        ("type", "conversation".into()),
        ("created", today.clone()),
        ("updated", today.clone()),
        ("author", "human".into()),
        ("tags", "[conversation]".into()),
    ]);
    let text = format!(
        "{head}# {}\n{}",
        if title.is_empty() {
            "Conversation"
        } else {
            &title
        },
        render_turn(first, stopped)
    );
    for n in 1..100 {
        let rel = if n == 1 {
            format!("{FOLDER}/{today}-{slug}.md")
        } else {
            format!("{FOLDER}/{today}-{slug}-{n}.md")
        };
        let path = vault.resolve_rel(&rel)?;
        match locks::create_new(&path, &text) {
            Ok(()) => return Ok(note_id(&vault.id, &rel)),
            Err(Error::Invalid(_)) => continue, // that name is taken
            Err(e) => return Err(e),
        }
    }
    Err(Error::Internal(
        "Too many conversations with that name today.".into(),
    ))
}

/// Add a turn to an existing conversation and bump its `updated:`.
pub fn append(vault: &Vault, rel: &str, turn: &Turn, stopped: bool) -> Result<()> {
    check_rel(rel)?;
    let path = vault.resolve_rel(rel)?;
    if !path.is_file() {
        return Err(Error::NotFound(format!("No conversation at {rel}")));
    }
    locks::append(&path, String::new, &render_turn(turn, stopped))?;
    let today = util::today();
    locks::guarded_edit(&path, None, |cur| {
        Ok(frontmatter::bump_updated(cur, &today))
    })?;
    Ok(())
}

fn check_rel(rel: &str) -> Result<()> {
    if rel.starts_with(&format!("{FOLDER}/")) && rel.ends_with(".md") {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{rel} is not a conversation")))
    }
}

pub fn load(vault: &Vault, rel: &str) -> Result<Conversation> {
    check_rel(rel)?;
    let path = vault.resolve_rel(rel)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| Error::NotFound(format!("No conversation at {rel}")))?;
    let p = frontmatter::parse(&text);
    let stem = rel
        .rsplit('/')
        .next()
        .unwrap_or(rel)
        .trim_end_matches(".md");
    Ok(Conversation {
        id: note_id(&vault.id, rel),
        title: p
            .props
            .get("title")
            .cloned()
            .unwrap_or_else(|| stem.to_string()),
        created: p.props.get("created").cloned().unwrap_or_default(),
        updated: p.props.get("updated").cloned().unwrap_or_default(),
        turns: parse_turns(p.body),
    })
}

/// Load by `"<vaultId>:<rel>"` among the given vaults.
pub fn get(vaults: &[Vault], id: &str) -> Result<Conversation> {
    let (v, rel) =
        split_note_id(id).ok_or_else(|| Error::Invalid(format!("{id} is not a note id")))?;
    let vault = vaults
        .iter()
        .find(|x| x.id == v)
        .ok_or_else(|| Error::NotFound(format!("No open vault has id {v}")))?;
    load(vault, rel)
}

/// Every conversation in the given vaults, most recently updated first.
pub fn list(vaults: &[Vault]) -> Vec<ConversationSummary> {
    let mut out = Vec::new();
    for v in vaults {
        let Ok(dir) = std::fs::read_dir(v.root.join(FOLDER)) else {
            continue;
        };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !name.ends_with(".md") {
                continue;
            }
            if let Ok(c) = load(v, &format!("{FOLDER}/{name}")) {
                out.push(ConversationSummary {
                    id: c.id,
                    title: c.title,
                    created: c.created,
                    updated: c.updated,
                    turns: c.turns.len() as u32,
                });
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(q: &str, a: &str) -> Turn {
        Turn {
            at: "2026-11-02 14:05".into(),
            question: q.into(),
            answer: a.into(),
            model: "qwen2.5:7b".into(),
            sources: vec!["garden".into(), "fence".into()],
        }
    }

    #[test]
    fn create_append_load_list() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let id = create(
            &v,
            &turn(
                "Where is the drip line going?",
                "Along the fence [[garden]].\n# Big heading\nmore",
            ),
            false,
        )
        .unwrap();
        let (_, rel) = split_note_id(&id).unwrap();
        assert!(
            rel.starts_with("06-Memory/conversations/")
                && rel.ends_with("-where-is-the-drip-line-going.md"),
            "{rel}"
        );
        let text = std::fs::read_to_string(v.root.join(rel)).unwrap();
        assert!(text.contains("type: conversation\n"));
        assert!(text.contains("author: human\n"));
        assert!(text.contains("### Big heading"));

        append(&v, rel, &turn("And when?", "Before the frost."), true).unwrap();
        let c = get(std::slice::from_ref(&v), &id).unwrap();
        assert_eq!(c.title, "Where is the drip line going?");
        assert_eq!(c.turns.len(), 2);
        assert_eq!(c.turns[0].question, "Where is the drip line going?");
        assert_eq!(
            c.turns[0].answer,
            "Along the fence [[garden]].\n### Big heading\nmore"
        );
        assert_eq!(c.turns[0].sources, vec!["garden", "fence"]);
        assert_eq!(c.turns[1].model, "qwen2.5:7b");
        assert_eq!(c.turns[1].answer, "Before the frost.");

        // a second conversation with the same first question gets its own file
        let id2 = create(&v, &turn("Where is the drip line going?", "x"), false).unwrap();
        assert_ne!(id, id2);
        let l = list(std::slice::from_ref(&v));
        assert_eq!(l.len(), 2);
        assert!(l.iter().any(|s| s.id == id && s.turns == 2));
    }

    #[test]
    fn refuses_paths_outside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert!(load(&v, "01-Projects/garden.md").is_err());
        assert!(load(&v, "06-Memory/conversations/../../x.md").is_err());
        assert!(append(
            &v,
            "06-Memory/conversations/missing.md",
            &turn("q", "a"),
            false
        )
        .is_err());
        assert!(get(&[v], "nope").is_err());
    }

    #[test]
    fn hand_edited_notes_still_parse() {
        let body = "# T\nintro the user wrote\n\n## You · 2026-11-01 09:00\nfirst?\n\n## HQ · m\n\nyes.\n\n## Notes\nuser's own section, kept with the answer\n";
        let t = parse_turns(body);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].question, "first?");
        assert!(t[0].answer.starts_with("yes."));
        assert!(t[0].sources.is_empty());
    }
}
