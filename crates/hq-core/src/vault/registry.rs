//! The set of vaults this Mac shows (SPEC 3, 13): opened from `Settings`, one primary,
//! a cached scan per vault that a watcher or a write invalidates.
//!
//! Blocked folder names are checked on every component of the path, raw and resolved,
//! before anything inside the folder is read or written: a vault nested in a blocked one,
//! or a symlink to it, is refused the same way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use super::{layout, scan, Marker, Scan, Vault, MARKER};
use crate::error::{Error, Result};
use crate::settings::{self, Settings, VaultEntry};
use crate::types::{VaultCandidate, VaultInfo, VaultLayout};
use crate::{locks, util};

/// A vault together with the scan the caller is working from.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub vault: Vault,
    pub scan: Arc<Scan>,
}

#[derive(Default)]
struct Cache {
    scans: HashMap<String, Arc<Scan>>,
    /// Bumped on every invalidation, so a scan that started before it is not cached.
    generation: HashMap<String, u64>,
}

pub struct Registry {
    vaults: Vec<Vault>,
    primary: Option<String>,
    problems: Vec<String>,
    /// Entries refused on purpose (blocked names): id -> why, so a lookup says Denied.
    refused: HashMap<String, String>,
    cache: Mutex<Cache>,
}

impl Registry {
    /// Open every vault in `settings` that may be opened. A missing or blocked folder is
    /// reported in `problems()`, not an error: one bad entry must not hide the others.
    pub fn load(settings: &Settings) -> Registry {
        let mut vaults: Vec<Vault> = Vec::new();
        let mut problems = Vec::new();
        let mut refused = HashMap::new();
        for entry in &settings.vaults {
            match open_checked(settings, Path::new(&entry.path)) {
                Ok(v) if vaults.iter().any(|x| x.id == v.id) => {
                    problems.push(format!("{}: listed twice", entry.name));
                }
                Ok(v) => vaults.push(v),
                Err(e) => {
                    if let Error::Denied(why) = &e {
                        refused.insert(entry.id.clone(), why.clone());
                    }
                    problems.push(format!("{}: {e}", entry.name));
                }
            }
        }
        let primary = settings
            .primary_vault
            .clone()
            .filter(|p| vaults.iter().any(|v| &v.id == p))
            .or_else(|| vaults.first().map(|v| v.id.clone()));
        Registry {
            vaults,
            primary,
            problems,
            refused,
            cache: Mutex::new(Cache::default()),
        }
    }

    pub fn vaults(&self) -> &[Vault] {
        &self.vaults
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn primary_id(&self) -> Option<&str> {
        self.primary.as_deref()
    }

    pub fn get(&self, id: &str) -> Result<&Vault> {
        self.vaults
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| match self.refused.get(id) {
                Some(why) => Error::Denied(why.clone()),
                None => Error::NotFound(format!("No vault with id {id} is open.")),
            })
    }

    /// Where captures and new notes go when the caller names no vault.
    pub fn primary(&self) -> Result<&Vault> {
        match &self.primary {
            Some(id) => self.get(id),
            None => Err(Error::Unavailable(
                "No vault yet. Add or create one in Settings.".into(),
            )),
        }
    }

    /// The named vault, or the primary one.
    pub fn pick(&self, id: Option<&str>) -> Result<&Vault> {
        match id {
            Some(id) => self.get(id),
            None => self.primary(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Cache> {
        // a panic while holding the cache lock leaves only a cache behind: keep going
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The cached scan of a vault, scanning it if there is none.
    pub fn scan(&self, id: &str) -> Result<Arc<Scan>> {
        let vault = self.get(id)?;
        let generation = {
            let cache = self.lock();
            if let Some(s) = cache.scans.get(id) {
                return Ok(s.clone());
            }
            cache.generation.get(id).copied().unwrap_or(0)
        };
        // scan outside the lock: a big vault must not stall every other caller
        let fresh = Arc::new(scan(vault));
        let mut cache = self.lock();
        if cache.generation.get(id).copied().unwrap_or(0) == generation {
            cache.scans.insert(id.to_string(), fresh.clone());
        }
        Ok(fresh)
    }

    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        Ok(Snapshot {
            vault: self.get(id)?.clone(),
            scan: self.scan(id)?,
        })
    }

    /// Every open vault with its scan, primary first.
    pub fn snapshots(&self) -> Vec<Snapshot> {
        let mut out: Vec<Snapshot> = self
            .vaults
            .iter()
            .filter_map(|v| self.snapshot(&v.id).ok())
            .collect();
        out.sort_by_key(|s| Some(s.vault.id.as_str()) != self.primary.as_deref());
        out
    }

    /// Drop the cached scan of one vault (or all): the next read rescans.
    pub fn invalidate(&self, id: Option<&str>) {
        let mut cache = self.lock();
        let ids: Vec<String> = match id {
            Some(id) => vec![id.to_string()],
            None => self.vaults.iter().map(|v| v.id.clone()).collect(),
        };
        for id in ids {
            cache.scans.remove(&id);
            *cache.generation.entry(id).or_insert(0) += 1;
        }
    }

    pub fn info(&self) -> Vec<VaultInfo> {
        self.snapshots()
            .into_iter()
            .map(|s| VaultInfo {
                primary: Some(s.vault.id.as_str()) == self.primary.as_deref(),
                id: s.vault.id,
                name: s.vault.name,
                path: s.vault.root.to_string_lossy().into_owned(),
                notes: s.scan.notes.len() as u32,
                unreadable: s.scan.unreadable.len() as u32,
            })
            .collect()
    }
}

/// Why `dir` may not be opened on this Mac, checking every component of the path as
/// given and as resolved, without reading anything inside it.
pub fn refusal(settings: &Settings, dir: &Path) -> Option<String> {
    let resolved = std::fs::canonicalize(dir).ok();
    let paths = std::iter::once(dir.to_path_buf()).chain(resolved);
    for p in paths {
        let mut cur: Option<&Path> = Some(p.as_path());
        while let Some(c) = cur {
            if let Some(why) = settings::blocked(settings, c) {
                return Some(why);
            }
            cur = c.parent();
        }
    }
    None
}

/// Open a vault folder after the blocked-name check.
pub fn open_checked(settings: &Settings, dir: &Path) -> Result<Vault> {
    if let Some(why) = refusal(settings, dir) {
        return Err(Error::Denied(why));
    }
    if !dir.is_absolute() {
        return Err(Error::Invalid(format!(
            "{} is not an absolute path.",
            dir.display()
        )));
    }
    Vault::open(dir)
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Add an existing folder (an Obsidian vault, a folder of notes, a vault from another Mac).
/// Adding a folder twice returns the vault already there. The first vault becomes primary.
pub fn add(settings: &mut Settings, dir: &Path) -> Result<Vault> {
    if let Some(e) = settings
        .vaults
        .iter()
        .find(|e| same_folder(Path::new(&e.path), dir))
    {
        return open_checked(settings, Path::new(&e.path.clone()));
    }
    let v = open_checked(settings, dir)?;
    if settings.vaults.iter().any(|e| e.id == v.id) {
        return Err(Error::Invalid(format!(
            "{} is already added from another folder (same vault id).",
            v.name
        )));
    }
    settings.vaults.push(VaultEntry {
        id: v.id.clone(),
        path: v.root.to_string_lossy().into_owned(),
        name: v.name.clone(),
    });
    if settings.primary_vault.is_none() {
        settings.primary_vault = Some(v.id.clone());
    }
    Ok(v)
}

/// Make a new vault: the folder (which must be empty or missing), its marker named `name`,
/// and with `VaultLayout::Default` the SPEC 3 folders. Onboarding adds the notes.
pub fn create(
    settings: &mut Settings,
    dir: &Path,
    name: &str,
    vault_layout: VaultLayout,
) -> Result<Vault> {
    if let Some(why) = refusal(settings, dir) {
        return Err(Error::Denied(why));
    }
    if !dir.is_absolute() {
        return Err(Error::Invalid(format!(
            "{} is not an absolute path.",
            dir.display()
        )));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("A vault needs a name.".into()));
    }
    if dir.exists() {
        let mut entries = std::fs::read_dir(dir)?;
        if entries.any(|e| {
            e.map(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .unwrap_or(true)
        }) {
            return Err(Error::Invalid(format!(
                "{} is not empty. Add it as an existing vault instead.",
                dir.display()
            )));
        }
    }
    std::fs::create_dir_all(dir)?;
    let marker = Marker {
        format: 1,
        id: util::random_hex(16),
        name: name.to_string(),
        created: util::today(),
    };
    locks::create_new(&dir.join(MARKER), &serde_json::to_string_pretty(&marker)?)?;
    if vault_layout == VaultLayout::Default {
        for f in layout::FOLDERS {
            std::fs::create_dir_all(dir.join(f))?;
        }
    }
    add(settings, dir)
}

/// Forget a vault. Its folder and notes are left exactly as they are.
pub fn remove(settings: &mut Settings, id: &str) -> Result<()> {
    let before = settings.vaults.len();
    settings.vaults.retain(|e| e.id != id);
    if settings.vaults.len() == before {
        return Err(Error::NotFound(format!("No vault with id {id}.")));
    }
    if settings.primary_vault.as_deref() == Some(id) {
        settings.primary_vault = settings.vaults.first().map(|e| e.id.clone());
    }
    Ok(())
}

pub fn set_primary(settings: &mut Settings, id: &str) -> Result<()> {
    if !settings.vaults.iter().any(|e| e.id == id) {
        return Err(Error::NotFound(format!("No vault with id {id}.")));
    }
    settings.primary_vault = Some(id.to_string());
    Ok(())
}

/// Keep settings' ids and names in step with the markers of the vaults that opened.
/// Returns true when an entry changed (the caller saves settings).
pub fn reconcile(settings: &mut Settings, reg: &Registry) -> bool {
    let mut changed = false;
    for e in settings.vaults.iter_mut() {
        if let Some(v) = reg
            .vaults()
            .iter()
            .find(|v| same_folder(&v.root, Path::new(&e.path)))
        {
            if e.id != v.id || e.name != v.name {
                if settings.primary_vault.as_deref() == Some(e.id.as_str()) {
                    settings.primary_vault = Some(v.id.clone());
                }
                e.id = v.id.clone();
                e.name = v.name.clone();
                changed = true;
            }
        }
    }
    changed
}

/// Where vaults usually live on a Mac: iCloud Drive's Obsidian folder, Documents, home.
pub fn default_candidate_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library/Mobile Documents/iCloud~md~obsidian/Documents"),
        home.join("Library/Mobile Documents/com~apple~CloudDocs"),
        home.join("Documents"),
        home.to_path_buf(),
    ]
}

/// Folders directly under `roots` that look like vaults. Blocked folders are skipped by
/// name before anything inside them is touched.
pub fn candidates(settings: &Settings, roots: &[PathBuf]) -> Vec<VaultCandidate> {
    let mut out: Vec<VaultCandidate> = Vec::new();
    for root in roots {
        if refusal(settings, root).is_some() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if name.starts_with('.')
                || !e.file_type().is_ok_and(|t| t.is_dir())
                || refusal(settings, &path).is_some()
            {
                continue;
            }
            let kind = if path.join(MARKER).is_file() {
                "vault-hq"
            } else if path.join(".obsidian").is_dir() {
                "obsidian"
            } else {
                continue;
            };
            if out.iter().any(|c| Path::new(&c.path) == path) {
                continue;
            }
            out.push(VaultCandidate {
                added: settings
                    .vaults
                    .iter()
                    .any(|v| same_folder(Path::new(&v.path), &path)),
                path: path.to_string_lossy().into_owned(),
                name,
                kind: kind.into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked() -> Settings {
        Settings {
            blocked_vault_names: vec!["secret-vault".into()],
            ..Default::default()
        }
    }

    #[test]
    fn add_create_remove_primary() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        let a = create(
            &mut s,
            &dir.path().join("home"),
            "home",
            VaultLayout::Default,
        )
        .unwrap();
        assert!(dir.path().join("home/00-Inbox").is_dir());
        assert_eq!(s.primary_vault.as_deref(), Some(a.id.as_str()));
        std::fs::create_dir(dir.path().join("team")).unwrap();
        let b = add(&mut s, &dir.path().join("team")).unwrap();
        // adding the same folder again is a no-op
        assert_eq!(add(&mut s, &dir.path().join("team")).unwrap().id, b.id);
        assert_eq!(s.vaults.len(), 2);
        set_primary(&mut s, &b.id).unwrap();
        let reg = Registry::load(&s);
        assert_eq!(reg.primary().unwrap().id, b.id);
        assert_eq!(reg.snapshots()[0].vault.id, b.id, "primary first");
        remove(&mut s, &b.id).unwrap();
        assert_eq!(s.primary_vault.as_deref(), Some(a.id.as_str()));
        assert!(remove(&mut s, &b.id).is_err());
        // creating into a non-empty folder is refused
        std::fs::write(dir.path().join("team/n.md"), "x").unwrap();
        assert!(create(&mut s, &dir.path().join("team"), "t", VaultLayout::Empty).is_err());
    }

    #[test]
    fn blocked_is_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("Secret-Vault");
        std::fs::create_dir_all(bad.join("inner")).unwrap();
        let mut s = blocked();
        assert!(matches!(add(&mut s, &bad), Err(Error::Denied(_))));
        assert!(matches!(
            add(&mut s, &bad.join("inner")),
            Err(Error::Denied(_))
        ));
        assert!(!bad.join(MARKER).exists(), "no marker written");
        assert!(!bad.join("inner").join(MARKER).exists());
        // a symlink with an innocent name pointing at it
        let link = dir.path().join("innocent");
        std::os::unix::fs::symlink(&bad, &link).unwrap();
        assert!(matches!(add(&mut s, &link), Err(Error::Denied(_))));
        assert!(s.vaults.is_empty());
        // an entry already in settings is not opened once its name is blocked
        let mut open = Settings::default();
        let ok = dir.path().join("fine");
        std::fs::create_dir(&ok).unwrap();
        add(&mut open, &ok).unwrap();
        open.blocked_vault_names = vec!["fine".into()];
        let reg = Registry::load(&open);
        assert!(reg.vaults().is_empty());
        assert_eq!(reg.problems().len(), 1);
        assert!(matches!(reg.get(&open.vaults[0].id), Err(Error::Denied(_))));
    }

    #[test]
    fn scan_cache_and_invalidate() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        let v = create(&mut s, &dir.path().join("v"), "v", VaultLayout::Empty).unwrap();
        let reg = Registry::load(&s);
        assert_eq!(reg.scan(&v.id).unwrap().notes.len(), 0);
        std::fs::write(v.root.join("a.md"), "x").unwrap();
        assert_eq!(reg.scan(&v.id).unwrap().notes.len(), 0, "cached");
        reg.invalidate(Some(&v.id));
        assert_eq!(reg.scan(&v.id).unwrap().notes.len(), 1);
        assert_eq!(reg.info()[0].notes, 1);
        assert!(reg.get("nope").is_err());
    }

    #[test]
    fn finds_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("obs/.obsidian")).unwrap();
        std::fs::create_dir_all(r.join("plain")).unwrap();
        std::fs::create_dir_all(r.join("secret-vault/.obsidian")).unwrap();
        let s = blocked();
        let c = candidates(&s, &[r.to_path_buf(), r.join("missing")]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "obs");
        assert_eq!(c[0].kind, "obsidian");
        assert!(!c[0].added);
    }
}
