//! A filesystem watcher per vault (FSEvents on macOS via `notify`). Changes are debounced
//! so a burst (a sync landing, an editor's save dance) arrives as one batch of
//! vault-relative paths.
//!
//! Paths with any component starting with `.` are ignored: `.vault-hq/` is HQ's own, and
//! `.obsidian/`, `.git/`, `.DS_Store` are never notes (the scan skips them too).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};

use super::Vault;
use crate::error::{Error, Result};

/// The quiet time after the last event before a batch is delivered.
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// A steady stream of changes still delivers at least this often.
const MAX_WAIT: Duration = Duration::from_secs(2);

/// Stops watching when dropped.
pub struct Watcher {
    _inner: notify::RecommendedWatcher,
    pub vault_id: String,
}

/// Watch `vault`, calling `on_change` from a background thread with the sorted, deduped
/// vault-relative paths that changed (files and folders; a folder rename shows up as
/// the folder).
pub fn watch(
    vault: &Vault,
    debounce: Duration,
    on_change: impl Fn(Vec<String>) + Send + 'static,
) -> Result<Watcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut inner = notify::recommended_watcher(tx)
        .map_err(|e| Error::Unavailable(format!("Can't watch {}: {e}", vault.name)))?;
    inner
        .watch(&vault.root, RecursiveMode::Recursive)
        .map_err(|e| Error::Unavailable(format!("Can't watch {}: {e}", vault.name)))?;
    // FSEvents reports resolved paths (/private/var/… for /var/…): strip either root
    let roots: Vec<PathBuf> = std::iter::once(vault.root.clone())
        .chain(std::fs::canonicalize(&vault.root).ok())
        .collect();
    std::thread::Builder::new()
        .name(format!("watch-{}", vault.name))
        .spawn(move || run(rx, roots, debounce, on_change))
        .map_err(|e| Error::Internal(format!("watcher thread: {e}")))?;
    Ok(Watcher {
        _inner: inner,
        vault_id: vault.id.clone(),
    })
}

fn run(
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    roots: Vec<PathBuf>,
    debounce: Duration,
    on_change: impl Fn(Vec<String>),
) {
    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut first: Option<Instant> = None;
    loop {
        let wait = match first {
            None => Duration::from_secs(3600),
            Some(_) => debounce,
        };
        match rx.recv_timeout(wait) {
            Ok(Ok(event)) => {
                for p in &event.paths {
                    if let Some(rel) = relative(&roots, p) {
                        pending.insert(rel);
                    }
                }
                if !pending.is_empty() && first.is_none() {
                    first = Some(Instant::now());
                }
                // keep collecting while it is busy, but not forever
                if first.is_some_and(|t| t.elapsed() < MAX_WAIT) {
                    continue;
                }
            }
            Ok(Err(_)) => continue, // a dropped event; the next one or a rescan catches up
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // the watcher was dropped: deliver what is left and stop
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !pending.is_empty() {
                    on_change(std::mem::take(&mut pending).into_iter().collect());
                }
                return;
            }
        }
        if !pending.is_empty() {
            on_change(std::mem::take(&mut pending).into_iter().collect());
        }
        first = None;
    }
}

/// The vault-relative path of `p`, or None when it is outside the vault or hidden.
pub fn relative(roots: &[PathBuf], p: &Path) -> Option<String> {
    let rel = roots.iter().find_map(|r| p.strip_prefix(r).ok())?;
    let s = rel.to_string_lossy();
    if s.is_empty() || s.split('/').any(|c| c.starts_with('.')) {
        return None;
    }
    Some(s.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn relative_paths() {
        let roots = vec![PathBuf::from("/v"), PathBuf::from("/private/v")];
        assert_eq!(
            relative(&roots, Path::new("/private/v/01-Projects/a.md")).as_deref(),
            Some("01-Projects/a.md")
        );
        assert_eq!(relative(&roots, Path::new("/v/.vault-hq/vault.json")), None);
        assert_eq!(relative(&roots, Path::new("/v/a/.DS_Store")), None);
        assert_eq!(relative(&roots, Path::new("/elsewhere/a.md")), None);
        assert_eq!(relative(&roots, Path::new("/v")), None);
    }

    #[test]
    fn reports_changes_debounced() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
        let sink = seen.clone();
        let _w = watch(&v, DEBOUNCE, move |rels| sink.lock().unwrap().push(rels)).unwrap();
        // FSEvents needs a moment to start delivering
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(dir.path().join("a.md"), "one").unwrap();
        std::fs::write(dir.path().join("b.md"), "two").unwrap();
        std::fs::write(dir.path().join(".vault-hq/x.json"), "{}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let all: BTreeSet<String> = seen.lock().unwrap().iter().flatten().cloned().collect();
            if all.contains("a.md") && all.contains("b.md") {
                assert!(!all.iter().any(|r| r.starts_with(".vault-hq")));
                break;
            }
            assert!(Instant::now() < deadline, "no change seen: {all:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
