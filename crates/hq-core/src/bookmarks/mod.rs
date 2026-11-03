//! Bookmarks (SPEC 8). Owned by stream F.
//!
//! Browsers are read from their own bookmark files, never through a plugin or a login, and
//! only once the user ticked that browser profile (`Settings.browsers`). Every bookmark
//! becomes a plain note under `10-Bookmarks/`, so agents and search see them like any note.

pub mod browsers;
pub mod chromium;
pub mod enrich;
pub mod firefox;
pub mod netscape;
pub mod platform;
pub mod resurface;
pub mod safari;
pub mod store;

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::settings::{MemoriesState, Settings};
use crate::types::{Bookmark, BookmarkFilter, BookmarkImportReport, BookmarkPage, Facet, Memory};
use crate::vault::Vault;

pub use browsers::{detect, RawBookmark};
pub use resurface::dismiss as memory_dismiss;

const DEFAULT_LIMIT: usize = 500;

pub(crate) fn http_only(items: &[RawBookmark]) -> impl Iterator<Item = &RawBookmark> {
    items.iter().filter(|b| store::normalize(&b.url).is_some())
}

/// The user ticked this source. An HTML file is allowed by picking it.
pub fn is_allowed(settings: &Settings, source_id: &str) -> bool {
    source_id.starts_with("html:") || settings.browsers.get(source_id).copied().unwrap_or(false)
}

/// Read one allowed source and bring it into the vault (dry run: report only).
pub fn import(
    vault: &Vault,
    home: &Path,
    settings: &Settings,
    source_id: &str,
    dry_run: bool,
    progress: &mut dyn FnMut(u32, u32, &str),
) -> Result<BookmarkImportReport> {
    let r = browsers::resolve(home, source_id)?;
    if !is_allowed(settings, source_id) {
        return Err(Error::Denied(format!(
            "Vault HQ hasn't been allowed to read {}. Tick it under Bookmarks first.",
            r.label()
        )));
    }
    let items = browsers::read_ref(&r)?;
    Ok(store::import(
        vault, source_id, &r.browser, items, dry_run, progress,
    ))
}

/// Every allowed browser profile, one after the other: what the hourly re-scan runs. One
/// source failing doesn't stop the others; its error is in its report.
pub fn rescan_allowed(
    vault: &Vault,
    home: &Path,
    settings: &Settings,
) -> Vec<BookmarkImportReport> {
    settings
        .browsers
        .iter()
        .filter(|(id, on)| **on && !id.starts_with("html:"))
        .map(|(id, _)| {
            import(vault, home, settings, id, false, &mut |_, _, _| {}).unwrap_or_else(|e| {
                BookmarkImportReport {
                    source_id: id.clone(),
                    errors: vec![e.to_string()],
                    ..Default::default()
                }
            })
        })
        .collect()
}

fn matches(b: &Bookmark, words: &[String], notes: &str) -> bool {
    let title = b.title.to_lowercase();
    let url = b.url.to_lowercase();
    let folder = b.folder.to_lowercase();
    let summary = b.summary.as_deref().unwrap_or("").to_lowercase();
    words.iter().all(|w| {
        title.contains(w)
            || url.contains(w)
            || folder.contains(w)
            || b.tags.iter().any(|t| t.contains(w))
            || summary.contains(w)
            || notes.contains(w)
    })
}

fn facets(counts: BTreeMap<String, u32>) -> Vec<Facet> {
    let mut v: Vec<Facet> = counts
        .into_iter()
        .map(|(value, count)| Facet { value, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    v
}

/// Search and filter bookmarks, newest first. Every word of `q` must appear somewhere:
/// title, URL, folder, tags, summary or the user's notes.
pub fn query(vault: &Vault, q: &str, filter: &BookmarkFilter) -> BookmarkPage {
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    let all: Vec<(Bookmark, String)> = store::load(vault)
        .into_iter()
        .map(|s| {
            let notes = store::split_body(&s.body).1.to_lowercase();
            (s.view(&vault.id), notes)
        })
        .collect();

    let mut sources = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut platforms = BTreeMap::new();
    for (b, _) in &all {
        *sources.entry(b.source.clone()).or_insert(0) += 1;
        *platforms
            .entry(platform::name(b.platform).to_string())
            .or_insert(0) += 1;
        for t in &b.tags {
            *tags.entry(t.clone()).or_insert(0) += 1;
        }
    }

    let mut items: Vec<Bookmark> = all
        .into_iter()
        .filter(|(b, n)| {
            filter.source.as_ref().is_none_or(|s| &b.source == s)
                && filter.tag.as_ref().is_none_or(|t| b.tags.contains(t))
                && filter.platform.is_none_or(|p| b.platform == p)
                && !(filter.hide_removed && b.status.is_some())
                && matches(b, &words, n)
        })
        .map(|(b, _)| b)
        .collect();
    items.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.title.cmp(&b.title)));
    let total = items.len() as u32;
    let limit = match filter.limit {
        0 => DEFAULT_LIMIT,
        n => n as usize,
    };
    items.truncate(limit);
    BookmarkPage {
        items,
        total,
        sources: facets(sources),
        tags: facets(tags),
        platforms: facets(platforms),
    }
}

/// Today's memories, recorded in `state` (the caller saves settings).
pub fn memories(vault: &Vault, state: &mut MemoriesState, date: &str, salt: u32) -> Vec<Memory> {
    let items: Vec<Bookmark> = store::load(vault)
        .into_iter()
        .map(|s| s.view(&vault.id))
        .collect();
    let picked = resurface::pick(&items, date, salt, state);
    resurface::record(state, &picked, date);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Platform;

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A fake home with one profile of every format, an HTML export, and a vault.
    fn setup() -> (tempfile::TempDir, std::path::PathBuf, Vault) {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        put(
            &home.join("Library/Application Support/BraveSoftware/Brave-Browser/Default/Bookmarks"),
            r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
                {"type":"url","name":"Rust","url":"https://www.rust-lang.org/","date_added":"13340000000000000"},
                {"type":"url","name":"Clip","url":"https://youtu.be/abc"}]}}}"#,
        );
        let ff = home.join("Library/Application Support/Firefox/Profiles/ab.default-release");
        std::fs::create_dir_all(&ff).unwrap();
        firefox::tests::make_places(&ff.join("places.sqlite"));
        put(
            &home.join("Library/Safari/Bookmarks.plist"),
            safari::tests::FIXTURE,
        );
        put(&d.path().join("export.html"), netscape::tests::FIXTURE);
        let root = d.path().join("vault");
        std::fs::create_dir_all(&root).unwrap();
        let v = Vault::open(&root).unwrap();
        (d, home, v)
    }

    #[test]
    fn end_to_end_over_a_fake_home() {
        let (d, home, v) = setup();
        let mut s = Settings::default();
        let detected = detect(&home, &s);
        let ids: Vec<String> = detected.iter().map(|x| x.id.clone()).collect();
        assert_eq!(
            ids,
            vec!["brave:Default", "firefox:ab.default-release", "safari"]
        );
        assert!(detected.iter().all(|x| !x.allowed && x.count.is_none()));

        // nothing is read before the user says yes
        let refused = import(&v, &home, &s, "brave:Default", true, &mut |_, _, _| {});
        assert!(matches!(refused, Err(Error::Denied(_))));
        assert!(store::load(&v).is_empty());

        for id in &ids {
            s.browsers.insert(id.clone(), true);
        }
        let counts: Vec<Option<u32>> = detect(&home, &s).iter().map(|x| x.count).collect();
        assert_eq!(counts, vec![Some(2), Some(2), Some(3)]);

        let reports = rescan_allowed(&v, &home, &s);
        assert_eq!(reports.len(), 3);
        assert!(reports.iter().all(|r| r.errors.is_empty()), "{reports:?}");
        let html = format!("html:{}", d.path().join("export.html").display());
        let r = import(&v, &home, &s, &html, false, &mut |_, _, _| {}).unwrap();
        assert_eq!((r.found, r.added), (3, 3));

        let page = query(&v, "", &BookmarkFilter::default());
        assert_eq!(page.total, 10);
        assert!(page
            .sources
            .iter()
            .any(|f| f.value == "safari" && f.count == 3));
        let only = |f: BookmarkFilter| query(&v, "", &f);
        let yt = only(BookmarkFilter {
            platform: Some(Platform::Youtube),
            ..Default::default()
        });
        assert_eq!(yt.total, 2);
        let baking = only(BookmarkFilter {
            tag: Some("baking".into()),
            ..Default::default()
        });
        assert_eq!(baking.items[0].title, "Bread page");
        assert_eq!(query(&v, "RUST lang", &BookmarkFilter::default()).total, 2);
        assert_eq!(query(&v, "rust repo", &BookmarkFilter::default()).total, 1);
        let limited = only(BookmarkFilter {
            limit: 2,
            ..Default::default()
        });
        assert_eq!((limited.items.len(), limited.total), (2, 10));

        // a profile that vanished is an error in its own report, not a failed re-scan
        s.browsers.insert("chrome:Default".into(), true);
        let reports = rescan_allowed(&v, &home, &s);
        let chrome = reports
            .iter()
            .find(|r| r.source_id == "chrome:Default")
            .unwrap();
        assert!(!chrome.errors.is_empty());
    }

    #[test]
    fn memories_are_recorded() {
        let (_d, home, v) = setup();
        let mut s = Settings::default();
        s.browsers.insert("brave:Default".into(), true);
        import(&v, &home, &s, "brave:Default", false, &mut |_, _, _| {}).unwrap();
        // the Rust bookmark dates from 2023; the clip has no date, so it counts as added today
        let today = crate::util::today();
        let m = memories(&v, &mut s.memories, &today, 0);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].title, "Rust");
        assert!(s.memories.last_surfaced.contains_key(&m[0].id));
        memory_dismiss(&mut s.memories, &m[0].id);
        assert!(memories(&v, &mut s.memories, &today, 0).is_empty());
    }
}
