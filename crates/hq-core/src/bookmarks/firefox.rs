//! Firefox `places.sqlite`. Firefox keeps the database locked (and recent writes in the
//! `-wal` file) while it runs, so HQ copies both to a private temp folder and reads the copy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::browsers::{date_from_unix, RawBookmark};
use crate::error::{Error, Result};
use crate::util;

/// Firefox's fixed root folders, by guid. The tags root holds tag folders, not bookmarks.
const ROOTS: &[(&str, &str)] = &[
    ("menu________", "Bookmarks Menu"),
    ("toolbar_____", "Bookmarks Toolbar"),
    ("unfiled_____", "Other Bookmarks"),
    ("mobile______", "Mobile Bookmarks"),
];
const TAGS_ROOT: &str = "tags________";
const ROOT: &str = "root________";

/// Removes the copy however the read ends.
struct TempCopy(PathBuf);

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unavailable(e: impl std::fmt::Display) -> Error {
    Error::Unavailable(format!("Could not read Firefox's bookmarks: {e}"))
}

pub fn read(places: &Path) -> Result<Vec<RawBookmark>> {
    let dir = std::env::temp_dir().join(format!("vault-hq-places-{}", util::random_hex(8)));
    std::fs::create_dir_all(&dir)?;
    let tmp = TempCopy(dir);
    let copy = tmp.0.join("places.sqlite");
    std::fs::copy(places, &copy)?;
    let wal = places.with_file_name("places.sqlite-wal");
    if wal.is_file() {
        std::fs::copy(&wal, tmp.0.join("places.sqlite-wal"))?;
    }
    // Read-write on our own copy: SQLite needs to build the -shm index to read a WAL.
    let conn = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(unavailable)?;
    query(&conn).map_err(unavailable)
}

struct Folder {
    parent: i64,
    title: String,
    guid: String,
}

fn query(conn: &Connection) -> rusqlite::Result<Vec<RawBookmark>> {
    let mut folders: HashMap<i64, Folder> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT id, parent, IFNULL(title, ''), IFNULL(guid, '') FROM moz_bookmarks WHERE type = 2",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Folder {
                parent: r.get(1)?,
                title: r.get(2)?,
                guid: r.get(3)?,
            },
        ))
    })?;
    for row in rows {
        let (id, f) = row?;
        folders.insert(id, f);
    }

    // (folder path, or None when under the tags root) for each folder id.
    let path_of = |mut id: i64| -> Option<String> {
        let mut parts = Vec::new();
        for _ in 0..64 {
            let f = folders.get(&id)?;
            if f.guid == TAGS_ROOT {
                return None;
            }
            if f.guid == ROOT {
                break;
            }
            if let Some((_, name)) = ROOTS.iter().find(|(g, _)| *g == f.guid) {
                parts.push(name.to_string());
                break;
            }
            parts.push(f.title.clone());
            id = f.parent;
        }
        parts.reverse();
        Some(parts.join("/"))
    };

    let mut stmt = conn.prepare(
        "SELECT b.parent, IFNULL(b.title, ''), IFNULL(b.dateAdded, 0), p.url, IFNULL(p.title, ''), b.fk
         FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk
         WHERE b.type = 1 ORDER BY b.parent, b.position",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
    let mut place_of = Vec::new();
    for row in rows {
        let (parent, title, added_us, url, place_title, fk) = row?;
        match path_of(parent) {
            Some(folder) => {
                let title = [title, place_title]
                    .into_iter()
                    .find(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| url.clone());
                place_of.push(fk);
                out.push(RawBookmark {
                    url,
                    title: title.trim().to_string(),
                    folder,
                    added: date_from_unix(added_us / 1_000_000),
                    tags: Vec::new(),
                });
            }
            // An entry in a tag folder: the folder's title is a tag on that place.
            None => {
                if let Some(tag) = folders.get(&parent).map(|f| f.title.clone()) {
                    if !tag.is_empty() {
                        tags.entry(fk).or_default().push(tag);
                    }
                }
            }
        }
    }
    for (b, fk) in out.iter_mut().zip(place_of) {
        if let Some(t) = tags.get(&fk) {
            b.tags = t.clone();
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small places.sqlite with Firefox's tables (only the columns HQ reads) and its roots.
    pub(crate) fn make_places(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL,
               parent INTEGER, position INTEGER, title LONGVARCHAR, dateAdded INTEGER,
               lastModified INTEGER, guid TEXT);
             INSERT INTO moz_bookmarks VALUES (1, 2, NULL, 0, 0, '', 0, 0, 'root________');
             INSERT INTO moz_bookmarks VALUES (2, 2, NULL, 1, 0, 'menu', 0, 0, 'menu________');
             INSERT INTO moz_bookmarks VALUES (3, 2, NULL, 1, 1, 'toolbar', 0, 0, 'toolbar_____');
             INSERT INTO moz_bookmarks VALUES (4, 2, NULL, 1, 2, 'tags', 0, 0, 'tags________');
             INSERT INTO moz_bookmarks VALUES (5, 2, NULL, 1, 3, 'unfiled', 0, 0, 'unfiled_____');
             INSERT INTO moz_bookmarks VALUES (6, 2, NULL, 3, 0, 'Recipes', 0, 0, 'folderRecipe');
             INSERT INTO moz_places VALUES (10, 'https://cooking.example/bread', 'Bread page');
             INSERT INTO moz_places VALUES (11, 'https://github.com/rust-lang/rust', NULL);
             INSERT INTO moz_places VALUES (12, 'place:sort=8&maxResults=10', NULL);
             INSERT INTO moz_bookmarks VALUES (20, 1, 10, 6, 0, NULL, 1700000000000000, 0, 'bm1');
             INSERT INTO moz_bookmarks VALUES (21, 1, 11, 2, 0, 'Rust repo', 1700000000000000, 0, 'bm2');
             INSERT INTO moz_bookmarks VALUES (22, 1, 12, 3, 1, 'Most visited', 0, 0, 'bm3');
             INSERT INTO moz_bookmarks VALUES (30, 2, NULL, 4, 0, 'baking', 0, 0, 'tagBaking');
             INSERT INTO moz_bookmarks VALUES (31, 1, 10, 30, 0, NULL, 0, 0, 'tagEntry');",
        )
        .unwrap();
    }

    #[test]
    fn reads_a_copy_with_folders_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("places.sqlite");
        make_places(&p);
        let b = read(&p).unwrap();
        assert_eq!(b.len(), 3, "the tag entry is not a bookmark: {b:?}");
        let rust = b.iter().find(|x| x.url.contains("github")).unwrap();
        assert_eq!(rust.title, "Rust repo");
        assert_eq!(rust.folder, "Bookmarks Menu");
        let bread = b.iter().find(|x| x.url.contains("bread")).unwrap();
        assert_eq!(bread.title, "Bread page", "falls back to the page title");
        assert_eq!(bread.folder, "Bookmarks Toolbar/Recipes");
        assert_eq!(bread.tags, vec!["baking"]);
        assert!(bread.added.as_deref().unwrap().starts_with("2023-11-1"));
    }

    #[test]
    fn temp_copy_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("copy");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("places.sqlite"), "x").unwrap();
        drop(TempCopy(inner.clone()));
        assert!(!inner.exists());
    }

    #[test]
    fn not_a_database() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("places.sqlite");
        std::fs::write(
            &p,
            "definitely not sqlite, just some text that is long enough",
        )
        .unwrap();
        assert!(matches!(read(&p), Err(Error::Unavailable(_))));
    }
}
