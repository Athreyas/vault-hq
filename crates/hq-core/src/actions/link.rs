//! Where an action item belongs and what it is about: a project note it names by title or
//! file name (filed in its `## Tasks`), the notes and bookmarks the existing search finds
//! for it (linked in a `related:` annotation), and their tags (offered, not applied).

use std::collections::HashSet;

use crate::error::Result;
use crate::search::chunk::{tokenize, Chunk};
use crate::search::{rank, Index, Semantic};
use crate::types::{note_id, RelatedLink, ScopeKinds, SearchScope};

/// Related links shown at most.
pub const MAX_RELATED: usize = 5;
/// Scores below these are not "about the same thing". Keyword-only scores top out at
/// 0.45 × folder weight, so they need a lower bar than blended ones.
/// (Blended scores of unrelated notes sit around 0.25–0.45 with nomic-embed-text.)
pub const RELATED_MIN_SEMANTIC: f64 = 0.4;
pub const RELATED_MIN_KEYWORDS: f64 = 0.18;
/// Links scoring at least this are ticked by default; the rest are offered unticked.
/// (Embedding similarity in a small vault is noisy: "book the dentist" is near a book club.)
pub const RELATED_TICK_SEMANTIC: f64 = 0.75;
pub const RELATED_TICK_KEYWORDS: f64 = 0.35;
/// Logs, not subjects: a day's notes or a capture mention everything once.
const NOT_RELATED: &[&str] = &[
    "09-Planning/Daily/",
    "00-Inbox/",
    "06-Memory/conversations/",
];
const GENERIC_TAGS: &[&str] = &[
    "project",
    "conversation",
    "bookmark",
    "index",
    "daily",
    "note",
];

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectNote {
    pub vault: String,
    pub rel: String,
    pub title: String,
    pub stem: String,
    pub tags: Vec<String>,
}

impl ProjectNote {
    pub fn id(&self) -> String {
        note_id(&self.vault, &self.rel)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Linked {
    pub project: Option<ProjectNote>,
    pub related: Vec<RelatedLink>,
    pub suggested_tags: Vec<String>,
}

fn is_project(c: &Chunk) -> bool {
    c.note_type.as_deref() == Some("project")
        || (c.rel.starts_with("01-Projects/") && c.note_type.is_none())
}

/// Lowercase words, for matching a project's name inside a sentence.
fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

fn contains_run(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Project notes in the index, one per note.
pub fn projects(index: &Index, vaults: &[String]) -> Vec<ProjectNote> {
    let mut seen = HashSet::new();
    index
        .chunks()
        .iter()
        .filter(|c| is_project(c) && (vaults.is_empty() || vaults.contains(&c.vault)))
        .filter(|c| seen.insert((c.vault.clone(), c.rel.clone())))
        .map(|c| ProjectNote {
            vault: c.vault.clone(),
            rel: c.rel.clone(),
            title: c.title.clone(),
            stem: c.stem.clone(),
            tags: c.tags.clone(),
        })
        .collect()
}

/// The project a sentence names: its title or file name as whole words ("the Lantern app"
/// finds `lantern-app.md` titled "Lantern app"). The longest name wins, so "Lantern app v2"
/// beats "Lantern".
pub fn by_name(projects: &[ProjectNote], text: &str, hint: Option<&str>) -> Option<ProjectNote> {
    let hay = words(text);
    let hint_words = hint.map(words).unwrap_or_default();
    projects
        .iter()
        .filter_map(|p| {
            let names = [words(&p.title), words(&p.stem.replace(['-', '_'], " "))];
            names
                .iter()
                .filter(|n| {
                    contains_run(&hay, n)
                        || (!hint_words.is_empty()
                            && (**n == hint_words || contains_run(&hint_words, n)))
                })
                .map(|n| n.iter().map(String::len).sum::<usize>())
                .max()
                .map(|len| (len, p))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p.clone())
}

/// Search for the action's notes and bookmarks, pick its project, collect related links.
pub fn link(
    index: &Index,
    text: &str,
    query: &str,
    hint: Option<&str>,
    vaults: &[String],
    semantic: Option<Semantic<'_>>,
    today: &str,
) -> Result<Linked> {
    let all = projects(index, vaults);
    // only a project the text names gets the task: search alone confuses "book the
    // dentist" with a book club, and a wrong note is worse than the loose list
    let project = by_name(&all, text, hint);
    let scope = SearchScope {
        vaults: vaults.to_vec(),
        kinds: ScopeKinds::Both,
        ..Default::default()
    };
    let search_q = match hint {
        Some(h) if !query.to_lowercase().contains(&h.to_lowercase()) => format!("{query} {h}"),
        _ => query.to_string(),
    };
    let ranked = if search_q.trim().is_empty() {
        None
    } else {
        Some(rank(index, &search_q, &scope, 12, semantic, today, None)?)
    };
    let mut related = Vec::new();
    let mut tag_sources: Vec<Vec<String>> = Vec::new();
    if let Some(r) = &ranked {
        let (min, tick) = if r.semantic {
            (RELATED_MIN_SEMANTIC, RELATED_TICK_SEMANTIC)
        } else {
            (RELATED_MIN_KEYWORDS, RELATED_TICK_KEYWORDS)
        };
        // a related note must share a real word with the task, not only a vague likeness
        let words: HashSet<String> = tokenize(query)
            .into_iter()
            .filter(|w| w.chars().count() >= 4)
            .collect();
        let mut seen = HashSet::new();
        for (i, score) in &r.picked {
            let c = &index.chunks()[*i];
            if *score < min
                || NOT_RELATED.iter().any(|p| c.rel.starts_with(p))
                || !c.tokens.iter().any(|t| words.contains(t))
                || project
                    .as_ref()
                    .is_some_and(|p| p.vault == c.vault && p.rel == c.rel)
                || !seen.insert((c.vault.clone(), c.rel.clone()))
            {
                continue;
            }
            if related.len() < MAX_RELATED {
                tag_sources.push(c.tags.clone());
                related.push(RelatedLink {
                    id: note_id(&c.vault, &c.rel),
                    stem: c.stem.clone(),
                    title: c.title.clone(),
                    kind: c.kind,
                    score: (score * 100.0).round() / 100.0,
                    include: *score >= tick,
                });
            }
        }
    }
    if let Some(p) = &project {
        tag_sources.insert(0, p.tags.clone());
    }
    let mut suggested: Vec<String> = Vec::new();
    for t in tag_sources.into_iter().take(4).flatten() {
        let t = t.trim().trim_start_matches('#').to_lowercase();
        if !t.is_empty()
            && !GENERIC_TAGS.contains(&t.as_str())
            && !t.starts_with("project/")
            && !suggested.contains(&t)
        {
            suggested.push(t);
        }
    }
    suggested.truncate(6);
    Ok(Linked {
        project,
        related,
        suggested_tags: suggested,
    })
}
