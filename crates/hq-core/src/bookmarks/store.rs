//! Bookmark notes (SPEC 8): one note per URL in `10-Bookmarks/<source>/<slug>.md`.
//!
//! A note's identity is its normalized URL, so the same page saved in three browsers is one
//! note. `seen_in:` lists the browser profiles that have it; a re-scan adds new ones and
//! never deletes: a note whose last browser dropped it gets `status: removed-in-browser`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use walkdir::WalkDir;

use super::browsers::RawBookmark;
use super::platform;
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{note_id, split_note_id, Bookmark, BookmarkImportReport, BookmarkPreview};
use crate::util;
use crate::vault::Vault;

pub const DIR: &str = "10-Bookmarks";
pub const REMOVED: &str = "removed-in-browser";
/// `source:` of a link the user pasted; never marked removed.
pub const PASTED: &str = "pasted";
const NOTES_HEADING: &str = "## Notes";
const SAMPLE: usize = 10;

/// The identity of a link: http(s) only, lowercase host (the `url` crate does that), no
/// fragment, no `utm_*` or click-id parameters. None for anything that isn't a web page.
pub fn normalize(url: &str) -> Option<String> {
    let mut u = url::Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none_or(str::is_empty) {
        return None;
    }
    u.set_fragment(None);
    if u.query().is_some() {
        let pairs: Vec<(String, String)> = u
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let kept: Vec<&(String, String)> = pairs.iter().filter(|(k, _)| !tracking(k)).collect();
        if kept.is_empty() {
            u.set_query(None);
        } else if kept.len() != pairs.len() {
            u.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    Some(u.to_string())
}

fn tracking(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || k == "fbclid" || k == "gclid"
}

/// A frontmatter value that also reads back the same in a YAML parser (Obsidian's).
pub fn fm_value(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s = s.trim();
    let risky = s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.starts_with([
            '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '#', '-', '?',
            ',',
        ]);
    if risky {
        format!("\"{}\"", s.replace('"', "'"))
    } else {
        s.to_string()
    }
}

/// A bookmark note as read from disk.
#[derive(Debug, Clone)]
pub struct Stored {
    pub rel: String,
    pub stem: String,
    pub props: BTreeMap<String, String>,
    pub body: String,
    pub norm: String,
}

impl Stored {
    fn from_text(rel: String, text: &str) -> Option<Stored> {
        let p = frontmatter::parse(text);
        if p.props.get("type").is_some_and(|t| t != "bookmark") {
            return None;
        }
        let norm = normalize(p.props.get("url")?)?;
        let stem = Path::new(&rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(Stored {
            rel,
            stem,
            props: p.props,
            body: p.body.to_string(),
            norm,
        })
    }

    pub fn prop(&self, k: &str) -> &str {
        self.props.get(k).map(String::as_str).unwrap_or("")
    }

    pub fn seen_in(&self) -> Vec<String> {
        frontmatter::list(self.prop("seen_in"))
    }

    pub fn is_removed(&self) -> bool {
        self.prop("status") == REMOVED
    }

    pub fn view(&self, vault_id: &str) -> Bookmark {
        let (summary, notes) = split_body(&self.body);
        let url = self.prop("url").to_string();
        let title = match self.prop("title") {
            "" => self.stem.clone(),
            t => t.to_string(),
        };
        let added = [self.prop("added"), self.prop("created")]
            .into_iter()
            .find(|d| !d.is_empty())
            .unwrap_or("")
            .to_string();
        Bookmark {
            id: note_id(vault_id, &self.rel),
            title,
            platform: platform::from_name(self.prop("platform"))
                .unwrap_or_else(|| platform::detect(&url)),
            url,
            source: self.prop("source").to_string(),
            folder: self.prop("folder").to_string(),
            added,
            tags: frontmatter::list(self.prop("tags")),
            summary,
            has_notes: !notes.is_empty(),
            status: Some(self.prop("status").to_string()).filter(|s| !s.is_empty()),
        }
    }
}

/// Byte range of the `## Notes` heading line in a body.
fn notes_heading(body: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == NOTES_HEADING {
            return Some((at, at + line.len()));
        }
        at += line.len();
    }
    None
}

/// (the HQ-written paragraph above `## Notes`, the user's notes below it). A body with no
/// heading is all the user's.
pub fn split_body(body: &str) -> (Option<String>, String) {
    match notes_heading(body) {
        Some((start, end)) => (
            Some(body[..start].trim().to_string()).filter(|s| !s.is_empty()),
            body[end..].trim().to_string(),
        ),
        None => (None, body.trim().to_string()),
    }
}

/// Every bookmark note in the vault. Unreadable files are skipped, like the vault scan does.
pub fn load(vault: &Vault) -> Vec<Stored> {
    let root = vault.root.join(DIR);
    let mut out = Vec::new();
    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for e in walker.flatten() {
        if !e.file_type().is_file() || e.path().extension().is_none_or(|x| x != "md") {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&vault.root) else {
            continue;
        };
        let rel = rel.to_string_lossy().into_owned();
        if let Ok(text) = std::fs::read_to_string(e.path()) {
            if let Some(s) = Stored::from_text(rel, &text) {
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// One bookmark note by its id, checked to be a bookmark in this vault.
pub fn get(vault: &Vault, id: &str) -> Result<Stored> {
    let (v, rel) =
        split_note_id(id).ok_or_else(|| Error::Invalid(format!("\"{id}\" is not a note id.")))?;
    if v != vault.id || !rel.starts_with(&format!("{DIR}/")) {
        return Err(Error::NotFound(format!("No bookmark {id} in this vault.")));
    }
    let path = vault.resolve_rel(rel)?;
    let text = std::fs::read_to_string(&path)?;
    Stored::from_text(rel.to_string(), &text)
        .ok_or_else(|| Error::Invalid(format!("{rel} is not a bookmark note (no http(s) url:).")))
}

pub struct NewNote<'a> {
    pub title: &'a str,
    pub url: &'a str,
    pub source: &'a str,
    pub seen_in: Option<&'a str>,
    pub folder: &'a str,
    pub added: &'a str,
    pub tags: &'a [String],
    pub note: Option<&'a str>,
}

pub fn render(n: &NewNote) -> String {
    let today = util::today();
    let tags: Vec<String> = n
        .tags
        .iter()
        .map(|t| tag(t))
        .filter(|t| !t.is_empty())
        .collect();
    let mut props: Vec<(&str, String)> = vec![
        ("title", fm_value(n.title)),
        ("type", "bookmark".into()),
        ("url", fm_value(n.url)),
        ("source", n.source.into()),
        ("folder", fm_value(n.folder)),
        ("platform", platform::name(platform::detect(n.url)).into()),
        ("added", n.added.into()),
        ("created", today.clone()),
        ("updated", today),
        ("author", "hq".into()),
        ("tags", frontmatter::format_list(&tags)),
    ];
    if let Some(id) = n.seen_in {
        props.push(("seen_in", frontmatter::format_list(&[id.to_string()])));
    }
    let mut s = frontmatter::render(&props);
    s.push_str(NOTES_HEADING);
    s.push('\n');
    if let Some(note) = n.note.map(str::trim).filter(|t| !t.is_empty()) {
        s.push('\n');
        s.push_str(note);
        s.push('\n');
    }
    s
}

/// A tag as the vault writes it: lowercase words joined by dashes, `/` kept for nesting.
pub fn tag(t: &str) -> String {
    t.split('/')
        .map(|part| util::slug(part, 40))
        .filter(|p| p != "untitled")
        .collect::<Vec<_>>()
        .join("/")
}

/// A file name for a new note that no other note has, decided before anything is written
/// so a dry run shows the real paths.
fn unique_rel(
    vault: &Vault,
    dir: &str,
    title: &str,
    url: &str,
    norm: &str,
    used: &HashSet<String>,
) -> String {
    let base = if title == url || title.trim().is_empty() {
        let bare = url.split_once("://").map_or(url, |(_, r)| r);
        util::slug(bare, 60)
    } else {
        util::slug(title, 60)
    };
    let free = |rel: &str| !used.contains(&rel.to_lowercase()) && !vault.root.join(rel).exists();
    let first = format!("{DIR}/{dir}/{base}.md");
    if free(&first) {
        return first;
    }
    let h = &locks::sha256(norm)[..6];
    let second = format!("{DIR}/{dir}/{base}-{h}.md");
    if free(&second) {
        return second;
    }
    (2..)
        .map(|n| format!("{DIR}/{dir}/{base}-{h}-{n}.md"))
        .find(|r| free(r))
        .unwrap_or(second)
}

/// Remove one frontmatter key, keeping every other line.
fn remove_key(text: &str, key: &str) -> String {
    let p = frontmatter::parse(text);
    if p.len == 0 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text[..p.len].split_inclusive('\n').enumerate() {
        let is_key = i > 0 && line.split_once(':').is_some_and(|(k, _)| k.trim() == key);
        if !is_key {
            out.push_str(line);
        }
    }
    out.push_str(p.body);
    out
}

/// `seen_in` gains `source_id`; a note that had been dropped everywhere is back.
fn mark_seen(text: &str, source_id: &str) -> String {
    let p = frontmatter::parse(text);
    let mut seen = frontmatter::list(p.props.get("seen_in").map_or("", String::as_str));
    let removed = p.props.get("status").is_some_and(|s| s == REMOVED);
    if !seen.iter().any(|s| s == source_id) {
        seen.push(source_id.to_string());
    }
    let mut t = frontmatter::set(text, "seen_in", &frontmatter::format_list(&seen), false);
    if removed {
        t = remove_key(&t, "status");
    }
    frontmatter::bump_updated(&t, &util::today())
}

/// `seen_in` loses `source_id`; with no browser left, the note is marked, never deleted.
fn drop_seen(text: &str, source_id: &str) -> String {
    let p = frontmatter::parse(text);
    let mut seen = frontmatter::list(p.props.get("seen_in").map_or("", String::as_str));
    seen.retain(|s| s != source_id);
    let from_browser = !matches!(
        p.props.get("source").map(String::as_str),
        Some(PASTED | "html")
    );
    let mut t = frontmatter::set(text, "seen_in", &frontmatter::format_list(&seen), false);
    if seen.is_empty() && from_browser {
        t = frontmatter::set(&t, "status", REMOVED, false);
    }
    frontmatter::bump_updated(&t, &util::today())
}

fn edit(vault: &Vault, rel: &str, f: impl FnOnce(&str) -> String) -> Result<()> {
    let path = vault.resolve_rel(rel)?;
    locks::guarded_edit(&path, None, |cur| Ok(f(cur)))?;
    Ok(())
}

/// Bring one source's bookmarks into the vault. `browser` names the folder new notes go
/// in. HTML exports are snapshots, so they add notes but never mark any removed.
pub fn import(
    vault: &Vault,
    source_id: &str,
    browser: &str,
    raw: Vec<RawBookmark>,
    dry_run: bool,
    progress: &mut dyn FnMut(u32, u32, &str),
) -> BookmarkImportReport {
    let track = !source_id.starts_with("html:");
    let mut report = BookmarkImportReport {
        source_id: source_id.to_string(),
        dry_run,
        ..Default::default()
    };
    let existing = load(vault);
    let mut by_norm: HashMap<&str, &Stored> = HashMap::new();
    for s in &existing {
        by_norm.entry(s.norm.as_str()).or_insert(s);
    }
    let mut used: HashSet<String> = existing.iter().map(|s| s.rel.to_lowercase()).collect();

    let mut current: HashSet<String> = HashSet::new();
    let mut unique = Vec::new();
    for r in raw {
        let Some(n) = normalize(&r.url) else { continue };
        report.found += 1;
        if current.insert(n.clone()) {
            unique.push((n, r));
        } else {
            report.duplicates += 1;
        }
    }

    let total = unique.len() as u32;
    let dir = util::slug(browser, 30);
    for (i, (norm, r)) in unique.iter().enumerate() {
        if let Some(s) = by_norm.get(norm.as_str()) {
            let listed = s.seen_in().iter().any(|x| x == source_id);
            if !track || (listed && !s.is_removed()) {
                report.unchanged += 1;
            } else {
                report.updated += 1;
                if !dry_run {
                    if let Err(e) = edit(vault, &s.rel, |t| mark_seen(t, source_id)) {
                        report.errors.push(format!("{}: {e}", s.rel));
                    }
                }
            }
        } else {
            let rel = unique_rel(vault, &dir, &r.title, &r.url, norm, &used);
            used.insert(rel.to_lowercase());
            report.added += 1;
            if report.sample.len() < SAMPLE {
                report.sample.push(BookmarkPreview {
                    title: r.title.clone(),
                    url: r.url.clone(),
                    folder: r.folder.clone(),
                    rel: rel.clone(),
                });
            }
            if !dry_run {
                let today = util::today();
                let text = render(&NewNote {
                    title: &r.title,
                    url: &r.url,
                    source: browser,
                    seen_in: track.then_some(source_id),
                    folder: &r.folder,
                    added: r.added.as_deref().unwrap_or(&today),
                    tags: &r.tags,
                    note: None,
                });
                let res = vault
                    .resolve_rel(&rel)
                    .and_then(|p| locks::create_new(&p, &text));
                if let Err(e) = res {
                    report.errors.push(format!("{rel}: {e}"));
                }
            }
        }
        let done = i as u32 + 1;
        if done == total || done.is_multiple_of(25) {
            progress(done, total, &r.title);
        }
    }

    if track {
        for s in &existing {
            if s.seen_in().iter().any(|x| x == source_id) && !current.contains(&s.norm) {
                report.removed += 1;
                if !dry_run {
                    if let Err(e) = edit(vault, &s.rel, |t| drop_seen(t, source_id)) {
                        report.errors.push(format!("{}: {e}", s.rel));
                    }
                }
            }
        }
    }
    if total == 0 {
        progress(0, 0, "nothing to import");
    }
    report
}

/// Add `note` at the end of the note, under `## Notes` (added if missing).
fn append_note(text: &str, note: &str) -> String {
    let p = frontmatter::parse(text);
    let mut t = text.trim_end().to_string();
    if notes_heading(p.body).is_none() {
        t.push_str("\n\n");
        t.push_str(NOTES_HEADING);
    }
    t.push_str("\n\n");
    t.push_str(note.trim());
    t.push('\n');
    frontmatter::bump_updated(&t, &util::today())
}

/// Replace the HQ-written paragraph above `## Notes`; the user's notes stay as they are.
pub fn set_summary(text: &str, summary: &str) -> String {
    let p = frontmatter::parse(text);
    let head = &text[..p.len];
    let summary = summary.trim();
    let rest = match notes_heading(p.body) {
        Some((start, _)) => p.body[start..].to_string(),
        None => {
            let body = p.body.trim_start();
            if body.is_empty() {
                format!("{NOTES_HEADING}\n")
            } else {
                format!("{NOTES_HEADING}\n\n{body}")
            }
        }
    };
    let t = if summary.is_empty() {
        format!("{head}\n{rest}")
    } else {
        format!("{head}\n{summary}\n\n{rest}")
    };
    frontmatter::bump_updated(&t, &util::today())
}

/// Paste a link: the note exists at once, titled by its URL; the page title arrives later
/// ([`super::enrich::fetch_page`]). A link already saved gets the note appended instead.
/// Returns the bookmark and whether it is new.
pub fn add(vault: &Vault, url: &str, note: Option<&str>) -> Result<(Bookmark, bool)> {
    let url = url.trim();
    let norm = normalize(url).ok_or_else(|| {
        Error::Invalid("Only http and https links can be saved as bookmarks.".into())
    })?;
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    let existing = load(vault);
    if let Some(s) = existing.iter().find(|s| s.norm == norm) {
        if let Some(n) = note {
            edit(vault, &s.rel, |t| append_note(t, n))?;
        }
        let fresh = get(vault, &note_id(&vault.id, &s.rel))?;
        return Ok((fresh.view(&vault.id), false));
    }
    let used: HashSet<String> = existing.iter().map(|s| s.rel.to_lowercase()).collect();
    let rel = unique_rel(vault, PASTED, &norm, &norm, &norm, &used);
    let today = util::today();
    let text = render(&NewNote {
        title: url,
        url,
        source: PASTED,
        seen_in: None,
        folder: "",
        added: &today,
        tags: &[],
        note,
    });
    locks::create_new(&vault.resolve_rel(&rel)?, &text)?;
    let s = get(vault, &note_id(&vault.id, &rel))?;
    Ok((s.view(&vault.id), true))
}

/// Apply `f` to one bookmark note under the lock.
pub fn update(vault: &Vault, id: &str, f: impl FnOnce(&str) -> String) -> Result<Bookmark> {
    let s = get(vault, id)?;
    edit(vault, &s.rel, f)?;
    Ok(get(vault, id)?.view(&vault.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(url: &str, title: &str) -> RawBookmark {
        RawBookmark {
            url: url.into(),
            title: title.into(),
            folder: "Bookmarks bar".into(),
            added: Some("2025-01-02".into()),
            tags: vec![],
        }
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        (d, v)
    }

    fn quiet() -> impl FnMut(u32, u32, &str) {
        |_, _, _| {}
    }

    #[test]
    fn normalizes() {
        assert_eq!(
            normalize("HTTPS://Example.COM/Path?utm_source=x&b=2&UTM_medium=y#frag").as_deref(),
            Some("https://example.com/Path?b=2")
        );
        assert_eq!(
            normalize("https://example.com/?utm_source=x").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            normalize(" https://example.com/a?q=a%20b ").as_deref(),
            Some("https://example.com/a?q=a%20b"),
            "a query with nothing to drop is left exactly as it was"
        );
        for bad in [
            "javascript:alert(1)",
            "file:///etc/hosts",
            "place:sort=8",
            "nope",
            "mailto:a@b.c",
        ] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }

    #[test]
    fn quotes_risky_values() {
        assert_eq!(fm_value("Plain title"), "Plain title");
        assert_eq!(fm_value("Rust: the book"), "\"Rust: the book\"");
        assert_eq!(fm_value("[draft] \"x\""), "\"[draft] 'x'\"");
        assert_eq!(fm_value("two\nlines"), "two lines");
        let t = render(&NewNote {
            title: "Rust: the book",
            url: "https://a.example/",
            source: "chrome",
            seen_in: Some("chrome:Default"),
            folder: "",
            added: "2025-01-01",
            tags: &[],
            note: None,
        });
        assert_eq!(frontmatter::parse(&t).props["title"], "Rust: the book");
    }

    #[test]
    fn import_dedupes_and_rescan_marks_removed() {
        let (_d, v) = vault();
        let first = vec![
            raw("https://example.com/a", "Page A"),
            raw("https://example.com/a?utm_source=feed#top", "Page A again"),
            raw("https://www.youtube.com/watch?v=1", "A video"),
            raw("javascript:void(0)", "bookmarklet"),
        ];
        let dry = import(
            &v,
            "chrome:Default",
            "chrome",
            first.clone(),
            true,
            &mut quiet(),
        );
        assert_eq!((dry.found, dry.added, dry.duplicates), (3, 2, 1));
        assert_eq!(dry.sample[0].rel, "10-Bookmarks/chrome/page-a.md");
        assert!(load(&v).is_empty(), "a dry run writes nothing");

        let mut calls = Vec::new();
        let r = import(
            &v,
            "chrome:Default",
            "chrome",
            first.clone(),
            false,
            &mut |d, t, _| calls.push((d, t)),
        );
        assert_eq!((r.added, r.errors.len()), (2, 0));
        assert_eq!(calls.last(), Some(&(2, 2)));
        let notes = load(&v);
        assert_eq!(notes.len(), 2);
        let a = notes.iter().find(|n| n.rel.ends_with("page-a.md")).unwrap();
        assert_eq!(a.prop("folder"), "Bookmarks bar");
        assert_eq!(a.prop("added"), "2025-01-02");
        assert_eq!(a.seen_in(), vec!["chrome:Default"]);
        let yt = notes.iter().find(|n| n.rel.contains("video")).unwrap();
        assert_eq!(yt.prop("platform"), "youtube");

        // the same scan again changes nothing
        let again = import(&v, "chrome:Default", "chrome", first, false, &mut quiet());
        assert_eq!(
            (again.added, again.updated, again.unchanged, again.removed),
            (0, 0, 2, 0)
        );

        // Firefox has page A too: no new note, but the note now lists both
        let ff = import(
            &v,
            "firefox:x.default",
            "firefox",
            vec![raw("https://EXAMPLE.com/a#x", "A")],
            false,
            &mut quiet(),
        );
        assert_eq!((ff.added, ff.updated), (0, 1));

        // Chrome dropped both: the video is marked, page A still lives in Firefox
        let gone = import(&v, "chrome:Default", "chrome", vec![], false, &mut quiet());
        assert_eq!(gone.removed, 2);
        let notes = load(&v);
        assert_eq!(notes.len(), 2, "never deletes");
        let a = notes.iter().find(|n| n.rel.ends_with("page-a.md")).unwrap();
        assert!(!a.is_removed());
        assert_eq!(a.seen_in(), vec!["firefox:x.default"]);
        let yt = notes.iter().find(|n| n.rel.contains("video")).unwrap();
        assert!(yt.is_removed());

        // and it comes back
        let back = import(
            &v,
            "chrome:Default",
            "chrome",
            vec![raw("https://www.youtube.com/watch?v=1", "A video")],
            false,
            &mut quiet(),
        );
        assert_eq!(back.updated, 1);
        let yt = load(&v)
            .into_iter()
            .find(|n| n.rel.contains("video"))
            .unwrap();
        assert!(!yt.is_removed());
        assert!(!std::fs::read_to_string(v.root.join(&yt.rel))
            .unwrap()
            .contains("status:"));
    }

    #[test]
    fn same_title_different_url_gets_its_own_file() {
        let (_d, v) = vault();
        let r = import(
            &v,
            "chrome:Default",
            "chrome",
            vec![
                raw("https://a.example/", "Home"),
                raw("https://b.example/", "Home"),
            ],
            false,
            &mut quiet(),
        );
        assert_eq!(r.added, 2);
        let rels: Vec<String> = load(&v).into_iter().map(|s| s.rel).collect();
        assert_eq!(rels.len(), 2);
        assert!(rels.contains(&"10-Bookmarks/chrome/home.md".to_string()));
    }

    #[test]
    fn html_imports_never_mark_removed() {
        let (_d, v) = vault();
        import(
            &v,
            "html:/x/e.html",
            "html",
            vec![raw("https://a.example/", "A")],
            false,
            &mut quiet(),
        );
        let r = import(&v, "html:/x/e.html", "html", vec![], false, &mut quiet());
        assert_eq!(r.removed, 0);
        assert!(!load(&v)[0].is_removed());
    }

    #[test]
    fn paste_creates_then_appends() {
        let (_d, v) = vault();
        let (b, new) = add(&v, "https://example.com/x?utm_campaign=y", None).unwrap();
        assert!(new);
        assert_eq!(b.title, "https://example.com/x?utm_campaign=y");
        assert_eq!(b.source, "pasted");
        assert!(b.id.ends_with("10-Bookmarks/pasted/example-com-x.md"));
        assert!(!b.has_notes);

        let (b2, new) = add(&v, "https://example.com/x", Some("worth a reread")).unwrap();
        assert!(!new);
        assert_eq!(b2.id, b.id);
        assert!(b2.has_notes);
        let text = std::fs::read_to_string(v.root.join(split_note_id(&b.id).unwrap().1)).unwrap();
        assert!(
            text.trim_end().ends_with("## Notes\n\nworth a reread"),
            "{text}"
        );

        assert!(matches!(
            add(&v, "ftp://example.com/", None),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn summary_sits_above_notes() {
        let (_d, v) = vault();
        let (b, _) = add(&v, "https://example.com/y", Some("mine")).unwrap();
        let b = update(&v, &b.id, |t| set_summary(t, "What the page says.")).unwrap();
        assert_eq!(b.summary.as_deref(), Some("What the page says."));
        let b = update(&v, &b.id, |t| set_summary(t, "Shorter.")).unwrap();
        assert_eq!(b.summary.as_deref(), Some("Shorter."));
        let text = std::fs::read_to_string(v.root.join(split_note_id(&b.id).unwrap().1)).unwrap();
        assert!(text.contains("\nShorter.\n\n## Notes\n\nmine\n"), "{text}");
        assert!(!text.contains("What the page"));

        // a hand-written note without the heading keeps its text, below a new heading
        let hand = "---\ntitle: T\ntype: bookmark\nurl: https://h.example/\n---\nmy own words\n";
        let t = set_summary(hand, "Sum.");
        let (s, notes) = split_body(frontmatter::parse(&t).body);
        assert_eq!(
            (s.as_deref(), notes.as_str()),
            (Some("Sum."), "my own words")
        );
    }

    #[test]
    fn get_refuses_other_places() {
        let (_d, v) = vault();
        assert!(get(&v, "nope").is_err());
        assert!(get(&v, &format!("{}:01-Projects/a.md", v.id)).is_err());
        assert!(get(&v, &format!("other:{DIR}/a.md")).is_err());
        assert!(get(&v, &format!("{}:{DIR}/../x.md", v.id)).is_err());
    }
}
