//! Vaults: the marker file and scanning (Phase 0); registry, layout, watcher (stream A).

pub mod checkboxes;
pub mod layout;
pub mod registry;
pub mod scan;
pub mod watch;

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::util;

pub use scan::{scan, Note, Scan};

/// `.vault-hq/vault.json` (SPEC 3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    pub format: u32,
    pub id: String,
    pub name: String,
    pub created: String,
}

pub const MARKER: &str = ".vault-hq/vault.json";

#[derive(Debug, Clone)]
pub struct Vault {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

impl Vault {
    /// Open a vault folder, writing its marker the first time (an Obsidian vault just added).
    pub fn open(root: &Path) -> Result<Vault> {
        if !root.is_dir() {
            return Err(Error::NotFound(format!("No folder at {}", root.display())));
        }
        let marker_path = root.join(MARKER);
        let marker = match std::fs::read_to_string(&marker_path) {
            Ok(s) => serde_json::from_str::<Marker>(&s)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let m = Marker {
                    format: 1,
                    id: util::random_hex(16),
                    name: root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "vault".into()),
                    created: util::today(),
                };
                std::fs::create_dir_all(root.join(".vault-hq"))?;
                std::fs::write(&marker_path, serde_json::to_string_pretty(&m)?)?;
                m
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Vault {
            id: marker.id,
            name: marker.name,
            root: root.to_path_buf(),
        })
    }

    /// The absolute path of a vault-relative path; anything that would leave the vault is refused.
    pub fn resolve_rel(&self, rel: &str) -> Result<PathBuf> {
        let p = Path::new(rel);
        if rel.is_empty()
            || p.is_absolute()
            || p.components().any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::Denied(format!(
                "{rel} is not a path inside the vault"
            )));
        }
        Ok(self.root.join(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = Vault::open(dir.path()).unwrap();
        let b = Vault::open(dir.path()).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.id.len(), 32);
    }

    #[test]
    fn rel_paths_stay_inside() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert!(v.resolve_rel("01-Projects/a.md").is_ok());
        for bad in ["../x.md", "/etc/hosts", "a/../../b.md", ""] {
            assert!(v.resolve_rel(bad).is_err(), "{bad} must be refused");
        }
    }
}
