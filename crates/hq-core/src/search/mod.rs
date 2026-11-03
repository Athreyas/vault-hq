//! Search and Ask (SPEC 6). Owned by stream C.
//!
//! 1. Every note is cut into heading-sized chunks (`chunk`).
//! 2. Chunks are ranked by BM25 (exact words, `bm25`) blended with cosine similarity from
//!    the user's embedding model (meaning, `vectors`), weighted by folder.
//! 3. Ask (`ask`) hands the top chunks to the chat model, streams its answer and saves the
//!    exchange as a conversation note (`conversations`).

pub mod ask;
pub mod bm25;
pub mod chunk;
pub mod conversations;
pub mod vectors;

use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDate;

use crate::error::{Error, Result};
use crate::locks::sha256;
use crate::ollama::{CancelToken, Embedder};
use crate::types::{note_id, HitKind, ScopeKinds, SearchHit, SearchResult, SearchScope};
use crate::vault::Note;

use bm25::Bm25;
use chunk::Chunk;
use vectors::{cosine, VectorStore};

/// Default number of hits for a search and of excerpts for a question.
pub const TOP_K: usize = 8;
/// At most this many chunks of one note, so one long note can't crowd out the rest.
const PER_NOTE: usize = 3;
const MIN_SCORE: f64 = 0.05;
const W_BM25: f64 = 0.45;
const W_COSINE: f64 = 0.55;

/// Every vault's chunks plus the BM25 statistics over them, kept in memory.
#[derive(Debug, Default)]
pub struct Index {
    chunks: Vec<Chunk>,
    bm25: Bm25,
    /// Per vault, a hash of its notes' paths and contents: unchanged vaults aren't re-cut.
    signatures: HashMap<String, String>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn vault_chunks<'a>(&'a self, vault: &'a str) -> impl Iterator<Item = &'a Chunk> + 'a {
        self.chunks.iter().filter(move |c| c.vault == vault)
    }

    /// Replace one vault's notes (from `vault::scan`). Returns false when nothing changed.
    pub fn update_vault(&mut self, vault: &str, notes: &[Note]) -> bool {
        let sig = sha256(
            &notes
                .iter()
                .map(|n| format!("{}\0{}", n.rel, n.hash))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if self.signatures.get(vault) == Some(&sig) {
            return false;
        }
        self.chunks.retain(|c| c.vault != vault);
        for n in notes.iter().filter(|n| chunk::indexed(&n.rel)) {
            self.chunks.extend(chunk::chunk_note(vault, n));
        }
        self.signatures.insert(vault.to_string(), sig);
        self.bm25 = Bm25::build(&self.chunks);
        true
    }

    /// Drop every vault not in `keep` (one removed in Settings).
    pub fn retain_vaults(&mut self, keep: &[String]) {
        let before = self.chunks.len();
        self.chunks.retain(|c| keep.contains(&c.vault));
        self.signatures.retain(|v, _| keep.contains(v));
        if self.chunks.len() != before {
            self.bm25 = Bm25::build(&self.chunks);
        }
    }
}

/// The first and last day of a scope's window, or None for all time.
pub fn window(scope: &SearchScope, today: &str) -> Result<Option<(String, String)>> {
    let days = match scope.days {
        None | Some(0) => return Ok(None),
        Some(d) => d,
    };
    let until = scope.until.as_deref().unwrap_or(today);
    let end = NaiveDate::parse_from_str(until, "%Y-%m-%d")
        .map_err(|_| Error::Invalid(format!("{until} is not a date (YYYY-MM-DD)")))?;
    let start = end - chrono::Duration::days(i64::from(days) - 1);
    Ok(Some((
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )))
}

pub fn in_scope(c: &Chunk, scope: &SearchScope, window: Option<&(String, String)>) -> bool {
    if !scope.vaults.is_empty() && !scope.vaults.contains(&c.vault) {
        return false;
    }
    let kind_ok = match scope.kinds {
        ScopeKinds::Both => true,
        ScopeKinds::Notes => c.kind == HitKind::Note,
        ScopeKinds::Bookmarks => c.kind == HitKind::Bookmark,
    };
    if !kind_ok {
        return false;
    }
    match window {
        None => true,
        Some((from, to)) => c
            .dates
            .iter()
            .any(|d| d.as_str() >= from.as_str() && d.as_str() <= to.as_str()),
    }
}

/// Living notes describe the present; session logs and captures describe the past.
pub fn folder_weight(c: &Chunk) -> f64 {
    let r = c.rel.as_str();
    if r.starts_with("01-Projects/") {
        1.2
    } else if r.starts_with("05-References/") {
        1.1
    } else if c.kind == HitKind::Bookmark {
        1.0
    } else if r.starts_with("00-Inbox/")
        || r.starts_with("06-Memory/sessions/")
        || r.starts_with("06-Memory/conversations/")
    {
        0.85
    } else {
        1.0
    }
}

/// Embeddings for one search: the model and where its vectors are cached.
pub struct Semantic<'a> {
    pub embedder: &'a dyn Embedder,
    pub cache_dir: &'a Path,
}

/// Chunk positions in the index with their scores, best first.
pub struct Ranked {
    pub picked: Vec<(usize, f64)>,
    pub semantic: bool,
    pub searched: usize,
}

/// Rank the chunks in scope for `query`. Embedding problems (server down, model missing)
/// fall back to keywords only, reported as `semantic: false`.
pub fn rank(
    index: &Index,
    query: &str,
    scope: &SearchScope,
    k: usize,
    semantic: Option<Semantic<'_>>,
    today: &str,
    cancel: Option<&CancelToken>,
) -> Result<Ranked> {
    let win = window(scope, today)?;
    let candidates: Vec<usize> = (0..index.chunks.len())
        .filter(|i| in_scope(&index.chunks[*i], scope, win.as_ref()))
        .collect();
    let q = chunk::tokenize(query);
    let lexical: Vec<f64> = candidates
        .iter()
        .map(|i| index.bm25.score(&q, *i))
        .collect();
    let max_lex = lexical.iter().cloned().fold(1e-9, f64::max);

    let cos = semantic
        .and_then(|s| cosines(index, &candidates, query, &s, cancel).ok())
        .filter(|v| !v.is_empty());

    let mut scored: Vec<(usize, f64)> = candidates
        .iter()
        .enumerate()
        .map(|(j, i)| {
            let c = cos.as_ref().map_or(0.0, |v| v[j].max(0.0));
            let s =
                folder_weight(&index.chunks[*i]) * (W_BM25 * lexical[j] / max_lex + W_COSINE * c);
            (*i, s)
        })
        .filter(|(_, s)| *s > MIN_SCORE)
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut per_note: HashMap<(&str, &str), usize> = HashMap::new();
    let picked = scored
        .into_iter()
        .filter(|(i, _)| {
            let c = &index.chunks[*i];
            let n = per_note.entry((&c.vault, &c.rel)).or_default();
            *n += 1;
            *n <= PER_NOTE
        })
        .take(k)
        .collect();
    Ok(Ranked {
        picked,
        semantic: cos.is_some(),
        searched: candidates.len(),
    })
}

/// Cosine of the query with every candidate, embedding chunks that changed first.
fn cosines(
    index: &Index,
    candidates: &[usize],
    query: &str,
    s: &Semantic<'_>,
    cancel: Option<&CancelToken>,
) -> Result<Vec<f64>> {
    let mut stores: HashMap<&str, VectorStore> = HashMap::new();
    for i in candidates {
        let v = index.chunks[*i].vault.as_str();
        stores
            .entry(v)
            .or_insert_with(|| VectorStore::open(s.cache_dir, v, s.embedder.model()));
    }
    for (vault, store) in stores.iter_mut() {
        let in_scope: Vec<&Chunk> = candidates
            .iter()
            .map(|i| &index.chunks[*i])
            .filter(|c| c.vault == *vault)
            .collect();
        store.ensure(&in_scope, s.embedder, cancel)?;
        store.retain_live(index.vault_chunks(vault));
        store.save()?;
    }
    let qv = s
        .embedder
        .embed(&[query.to_string()])?
        .pop()
        .ok_or_else(|| Error::Internal("No embedding for the question.".into()))?;
    Ok(candidates
        .iter()
        .map(|i| {
            let c = &index.chunks[*i];
            stores
                .get(c.vault.as_str())
                .and_then(|st| st.get(&c.rel, &c.hash))
                .map_or(0.0, |v| cosine(&qv, v))
        })
        .collect())
}

/// Embed every chunk of every vault that has no vector yet: a background warm-up so the
/// first question after a big change isn't slow. Returns how many were embedded.
pub fn warm(
    index: &Index,
    embedder: &dyn Embedder,
    cache_dir: &Path,
    cancel: Option<&CancelToken>,
) -> Result<usize> {
    let mut vaults: Vec<&str> = index.chunks.iter().map(|c| c.vault.as_str()).collect();
    vaults.sort();
    vaults.dedup();
    let mut n = 0;
    for v in vaults {
        let mut store = VectorStore::open(cache_dir, v, embedder.model());
        let all: Vec<&Chunk> = index.vault_chunks(v).collect();
        let r = store.ensure(&all, embedder, cancel);
        store.retain_live(all.iter().copied());
        store.save()?;
        n += r?;
    }
    Ok(n)
}

/// What a hit shows: one line, task markers and ids tidied, at most 280 characters.
pub fn excerpt(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(400));
    let mut skip_id = false;
    for word in text.split_whitespace() {
        if std::mem::take(&mut skip_id) {
            continue;
        }
        match word {
            "🆔" => {
                skip_id = true;
                continue;
            }
            "🔺" | "⏫" | "🔼" | "🔽" | "⏬" => continue,
            _ => {}
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word.replace("**", ""));
    }
    let out = out
        .replace("- [ ] ", "○ ")
        .replace("- [x] ", "✓ ")
        .replace("- [X] ", "✓ ");
    if out.chars().count() > 280 {
        let cut: String = out.chars().take(279).collect();
        format!("{}…", cut.trim_end())
    } else {
        out
    }
}

pub fn hit(c: &Chunk, score: f64) -> SearchHit {
    SearchHit {
        id: note_id(&c.vault, &c.rel),
        vault: c.vault.clone(),
        rel: c.rel.clone(),
        stem: c.stem.clone(),
        title: c.title.clone(),
        heading: c.heading.clone(),
        excerpt: excerpt(&c.text),
        score: (score * 100.0).round() / 100.0,
        kind: c.kind,
        date: c.newest_date().map(String::from),
    }
}

/// The `search` command: ranked hits for `query` in `scope`.
pub fn search(
    index: &Index,
    query: &str,
    scope: &SearchScope,
    k: usize,
    semantic: Option<Semantic<'_>>,
    today: &str,
) -> Result<SearchResult> {
    if query.trim().is_empty() {
        return Err(Error::Invalid("Type something to search for.".into()));
    }
    let r = rank(index, query, scope, k, semantic, today, None)?;
    Ok(SearchResult {
        hits: r
            .picked
            .iter()
            .map(|(i, s)| hit(&index.chunks[*i], *s))
            .collect(),
        semantic: r.semantic,
        searched: r.searched as u32,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::search::vectors::tests::FakeEmbedder;
    use std::collections::BTreeMap;

    pub(crate) fn note(rel: &str, props: &[(&str, &str)], body: &str) -> Note {
        Note {
            rel: rel.into(),
            stem: rel
                .rsplit('/')
                .next()
                .unwrap()
                .trim_end_matches(".md")
                .into(),
            props: props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            body: body.into(),
            hash: sha256(body),
            mtime: 0,
        }
    }

    pub(crate) fn fixture() -> Index {
        let mut idx = Index::new();
        idx.update_vault(
            "home",
            &[
                note(
                    "01-Projects/garden.md",
                    &[("title", "Garden"), ("updated", "2026-10-30")],
                    "## Irrigation\nLay the drip line along the fence before the frost.\n",
                ),
                note(
                    "00-Inbox/2026-10-01-capture.md",
                    &[("created", "2026-10-01")],
                    "- 09:00 drip line parts ordered from the hardware store\n",
                ),
                note(
                    "09-Planning/Daily/2026-10-29.md",
                    &[],
                    "## Work\nRewrote the invoice exporter; tests pass now.\n",
                ),
                note(
                    "10-Bookmarks/chrome/drip-guide.md",
                    &[
                        ("title", "Drip irrigation guide"),
                        ("type", "bookmark"),
                        ("added", "2026-03-04"),
                    ],
                    "A long guide to drip irrigation for vegetable beds.\n",
                ),
                note(
                    "AGENTS.md",
                    &[],
                    "Agents: drip drip drip drip, never index this file.\n",
                ),
            ],
        );
        idx.update_vault(
            "team",
            &[note(
                "01-Projects/launch.md",
                &[("updated", "2026-10-20")],
                "The launch checklist: drip campaign emails go out on Monday.\n",
            )],
        );
        idx
    }

    fn rels(r: &SearchResult) -> Vec<&str> {
        r.hits.iter().map(|h| h.rel.as_str()).collect()
    }

    #[test]
    fn keyword_ranking_and_weights() {
        let idx = fixture();
        let r = search(
            &idx,
            "drip line",
            &SearchScope::default(),
            8,
            None,
            "2026-11-02",
        )
        .unwrap();
        assert!(!r.semantic);
        assert_eq!(r.searched, 5, "AGENTS.md is not indexed");
        // the project note outranks the inbox capture with the same words (weight 1.2 vs 0.85)
        assert_eq!(rels(&r)[0], "01-Projects/garden.md");
        assert!(rels(&r).contains(&"00-Inbox/2026-10-01-capture.md"));
        assert!(!rels(&r).contains(&"09-Planning/Daily/2026-10-29.md"));
        let h = &r.hits[0];
        assert_eq!(h.id, "home:01-Projects/garden.md");
        assert_eq!(h.heading, "Garden › Irrigation");
        assert_eq!(h.date.as_deref(), Some("2026-10-30"));
        assert!(search(&idx, "  ", &SearchScope::default(), 8, None, "2026-11-02").is_err());
    }

    #[test]
    fn scope_filters() {
        let idx = fixture();
        let today = "2026-11-02";
        let only = |scope: SearchScope| {
            let r = search(&idx, "drip", &scope, 8, None, today).unwrap();
            let mut v: Vec<String> = r
                .hits
                .iter()
                .map(|h| format!("{}:{}", h.vault, h.rel))
                .collect();
            v.sort();
            v
        };
        let team = only(SearchScope {
            vaults: vec!["team".into()],
            ..Default::default()
        });
        assert_eq!(team, vec!["team:01-Projects/launch.md"]);
        let bms = only(SearchScope {
            kinds: ScopeKinds::Bookmarks,
            ..Default::default()
        });
        assert_eq!(bms, vec!["home:10-Bookmarks/chrome/drip-guide.md"]);
        let notes = only(SearchScope {
            kinds: ScopeKinds::Notes,
            ..Default::default()
        });
        assert!(!notes.iter().any(|r| r.contains("Bookmarks")));
        // the last 7 days: garden (updated 10-30) only; the capture (10-01) and bookmark are older
        let week = only(SearchScope {
            vaults: vec!["home".into()],
            days: Some(7),
            ..Default::default()
        });
        assert_eq!(week, vec!["home:01-Projects/garden.md"]);
        // a window ending on a chosen day, from the file name's date
        let day = search(
            &idx,
            "invoice",
            &SearchScope {
                days: Some(1),
                until: Some("2026-10-29".into()),
                ..Default::default()
            },
            8,
            None,
            today,
        )
        .unwrap();
        assert_eq!(rels(&day), vec!["09-Planning/Daily/2026-10-29.md"]);
        let bad = SearchScope {
            days: Some(3),
            until: Some("soon".into()),
            ..Default::default()
        };
        assert!(matches!(
            search(&idx, "drip", &bad, 8, None, today),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn semantic_blend_and_vector_cache() {
        let dir = tempfile::tempdir().unwrap();
        let idx = fixture();
        let e = FakeEmbedder::new("fake");
        let sem = || Semantic {
            embedder: &e,
            cache_dir: dir.path(),
        };
        let r = search(
            &idx,
            "invoice exporter",
            &SearchScope::default(),
            8,
            Some(sem()),
            "2026-11-02",
        )
        .unwrap();
        assert!(r.semantic);
        assert_eq!(rels(&r)[0], "09-Planning/Daily/2026-10-29.md");
        // 5 chunks embedded once (two vault files), then only the question each time
        let first: usize = e.calls.borrow().iter().sum();
        assert_eq!(first, 5 + 1);
        search(
            &idx,
            "fence",
            &SearchScope::default(),
            8,
            Some(sem()),
            "2026-11-02",
        )
        .unwrap();
        assert_eq!(e.calls.borrow().iter().sum::<usize>(), first + 1);
        assert!(VectorStore::file_for(dir.path(), "home").exists());
        assert!(VectorStore::file_for(dir.path(), "team").exists());
    }

    struct Down;
    impl Embedder for Down {
        fn model(&self) -> &str {
            "down"
        }
        fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>> {
            Err(Error::Unavailable("server down".into()))
        }
    }

    #[test]
    fn embedding_failure_falls_back_to_keywords() {
        let dir = tempfile::tempdir().unwrap();
        let idx = fixture();
        let r = search(
            &idx,
            "drip",
            &SearchScope::default(),
            8,
            Some(Semantic {
                embedder: &Down,
                cache_dir: dir.path(),
            }),
            "2026-11-02",
        )
        .unwrap();
        assert!(!r.semantic);
        assert!(!r.hits.is_empty());
    }

    #[test]
    fn unchanged_vaults_are_not_recut_and_per_note_cap() {
        let mut idx = fixture();
        let n = vec![note(
            "01-Projects/launch.md",
            &[("updated", "2026-10-20")],
            "The launch checklist: drip campaign emails go out on Monday.\n",
        )];
        assert!(!idx.update_vault("team", &n));
        let body: String = (0..6)
            .map(|i| format!("## Part {i}\nwater the drip beds, part number {i}\n"))
            .collect();
        idx.update_vault("team", &[note("05-References/water.md", &[], &body)]);
        let r = search(
            &idx,
            "drip beds",
            &SearchScope::default(),
            8,
            None,
            "2026-11-02",
        )
        .unwrap();
        assert_eq!(
            r.hits
                .iter()
                .filter(|h| h.rel == "05-References/water.md")
                .count(),
            3
        );
        idx.retain_vaults(&["home".into()]);
        assert!(idx.chunks().iter().all(|c| c.vault == "home"));
    }

    #[test]
    fn excerpts_are_tidy() {
        let e = excerpt("- [ ] **Lay** the line ⏫ 🆔 a1b2c3 📅 2026-11-01\n- [x] done one");
        assert_eq!(e, "○ Lay the line 📅 2026-11-01 ✓ done one");
        assert!(excerpt(&"word ".repeat(200)).chars().count() <= 280);
    }
}
