//! Read every note in a vault (SPEC 3). One unreadable file never fails the scan: it is
//! listed in `unreadable` (iCloud can refuse reads to background processes).

use std::collections::BTreeMap;

use walkdir::WalkDir;

use super::Vault;
use crate::frontmatter;
use crate::locks::sha256;

#[derive(Debug, Clone)]
pub struct Note {
    pub rel: String,
    pub stem: String,
    pub props: BTreeMap<String, String>,
    /// Text after the frontmatter.
    pub body: String,
    /// SHA-256 of the whole file as read.
    pub hash: String,
    /// Seconds since the epoch.
    pub mtime: i64,
}

impl Note {
    pub fn title(&self) -> &str {
        self.props
            .get("title")
            .map(String::as_str)
            .filter(|t| !t.is_empty())
            .unwrap_or(&self.stem)
    }
}

#[derive(Debug, Default)]
pub struct Scan {
    pub notes: Vec<Note>,
    pub unreadable: Vec<String>,
}

const SKIP_DIRS: &[&str] = &["_tools", "_templates", "node_modules"];

pub fn scan(vault: &Vault) -> Scan {
    let mut out = Scan::default();
    let walker = WalkDir::new(&vault.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0
                || !(name.starts_with('.')
                    || (e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref())))
        });
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if let Some(p) = e.path().and_then(|p| p.strip_prefix(&vault.root).ok()) {
                    out.unreadable.push(format!("{}/", p.display()));
                }
                continue;
            }
        };
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|x| x != "md") {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&vault.root)
            .expect("walked under root")
            .to_string_lossy()
            .into_owned();
        match std::fs::read_to_string(entry.path()) {
            Ok(text) => {
                let p = frontmatter::parse(&text);
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let stem = entry
                    .path()
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.notes.push(Note {
                    rel,
                    stem,
                    props: p.props,
                    body: p.body.to_string(),
                    hash: sha256(&text),
                    mtime,
                });
            }
            Err(_) => out.unreadable.push(rel),
        }
    }
    out.notes.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("01-Projects")).unwrap();
        std::fs::create_dir_all(r.join("_templates")).unwrap();
        std::fs::create_dir_all(r.join(".obsidian")).unwrap();
        std::fs::write(r.join("01-Projects/a.md"), "---\ntitle: A\n---\nbody\n").unwrap();
        std::fs::write(r.join("loose.md"), "no frontmatter\n").unwrap();
        std::fs::write(r.join("_templates/t.md"), "x").unwrap();
        std::fs::write(r.join(".obsidian/w.md"), "x").unwrap();
        std::fs::write(r.join("image.png"), "x").unwrap();
        let v = Vault::open(r).unwrap();
        let s = scan(&v);
        let rels: Vec<_> = s.notes.iter().map(|n| n.rel.as_str()).collect();
        assert_eq!(rels, vec!["01-Projects/a.md", "loose.md"]);
        assert_eq!(s.notes[0].title(), "A");
        assert_eq!(s.notes[1].title(), "loose");
        assert!(s.unreadable.is_empty());
    }
}
