// Background jobs. For now: watch the vaults and emit `vault://changed {vault, rels}` so
// open views refresh when an agent, an editor or a sync writes a note.
//
// Seam for stream A: `hq_core::vault::watch` will deliver changes from FSEvents. When it
// lands, `start` subscribes to it instead of polling and passes each batch to
// `vault_changed`, which is also where the `Hq` facade is told to refresh its indexes
// (tasks, search) before the UI hears about it. The periodic re-index, the hourly
// bookmark re-scan and widget sampling (SPEC 2) join here as their streams land.

use std::collections::BTreeMap;
use std::path::Path;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use hq_core::settings;
use hq_core::types::VaultChanged;
use hq_core::vault::Vault;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use walkdir::WalkDir;

use crate::state::Shell;

const POLL: Duration = Duration::from_secs(3);

/// vault-relative path → (modified time in ms, size): enough to notice an edit without
/// reading any file.
pub type Snapshot = BTreeMap<String, (u128, u64)>;

/// What the scanner would read: `.md` files, skipping dot folders and tool folders.
pub fn snapshot(root: &Path) -> Snapshot {
    const SKIP: &[&str] = &["_tools", "node_modules"];
    let mut out = Snapshot::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0
                || !(name.starts_with('.')
                    || (e.file_type().is_dir() && SKIP.contains(&name.as_ref())))
        });
    for entry in walker.flatten() {
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|x| x != "md") {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        out.insert(rel.to_string_lossy().into_owned(), (mtime, meta.len()));
    }
    out
}

/// Paths added, removed or changed between two snapshots, sorted.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out: Vec<String> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect();
    out.extend(before.keys().filter(|k| !after.contains_key(*k)).cloned());
    out.sort();
    out
}

/// The one place a vault change reaches the UI.
pub fn vault_changed<R: Runtime>(app: &AppHandle<R>, change: VaultChanged) {
    if let Err(e) = app.emit("vault://changed", change) {
        eprintln!("vault-hq: could not emit vault://changed: {e}");
    }
}

/// The vaults to watch right now: those in settings that are not blocked and open.
fn watched(s: &settings::Settings) -> Vec<Vault> {
    s.vaults
        .iter()
        .filter_map(|v| {
            let p = Path::new(&v.path);
            if settings::blocked(s, p).is_some() {
                return None;
            }
            Vault::open(p).ok()
        })
        .collect()
}

pub fn start<R: Runtime>(app: AppHandle<R>) {
    let spawned = thread::Builder::new()
        .name("hq-watch".into())
        .spawn(move || {
            let mut seen: BTreeMap<String, Snapshot> = BTreeMap::new();
            loop {
                let settings = app.state::<Shell>().settings();
                let vaults = watched(&settings);
                seen.retain(|id, _| vaults.iter().any(|v| &v.id == id));
                for v in vaults {
                    let now = snapshot(&v.root);
                    match seen.get(&v.id) {
                        // the first look at a vault is a baseline, not a change
                        None => {}
                        Some(before) => {
                            let rels = diff(before, &now);
                            if !rels.is_empty() {
                                vault_changed(
                                    &app,
                                    VaultChanged {
                                        vault: v.id.clone(),
                                        rels,
                                    },
                                );
                            }
                        }
                    }
                    seen.insert(v.id, now);
                }
                thread::sleep(POLL);
            }
        });
    if let Err(e) = spawned {
        eprintln!("vault-hq: could not start the vault watcher: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_add_change_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("01-Projects")).unwrap();
        std::fs::create_dir_all(r.join(".obsidian")).unwrap();
        std::fs::write(r.join("01-Projects/a.md"), "a").unwrap();
        std::fs::write(r.join("gone.md"), "x").unwrap();
        let before = snapshot(r);
        assert_eq!(before.len(), 2);

        std::fs::write(r.join("01-Projects/a.md"), "a longer body").unwrap();
        std::fs::write(r.join("01-Projects/b.md"), "b").unwrap();
        std::fs::remove_file(r.join("gone.md")).unwrap();
        std::fs::write(r.join(".obsidian/w.md"), "ignored").unwrap();
        std::fs::write(r.join("image.png"), "ignored").unwrap();
        let after = snapshot(r);

        assert_eq!(
            diff(&before, &after),
            vec!["01-Projects/a.md", "01-Projects/b.md", "gone.md"]
        );
        assert!(diff(&after, &after).is_empty());
    }

    #[test]
    fn blocked_vaults_are_not_watched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("private-vault");
        std::fs::create_dir_all(&p).unwrap();
        let s = settings::Settings {
            vaults: vec![settings::VaultEntry {
                id: "x".into(),
                path: p.to_string_lossy().into_owned(),
                name: "private-vault".into(),
            }],
            blocked_vault_names: vec!["private-vault".into()],
            ..Default::default()
        };
        assert!(watched(&s).is_empty());
        // and nothing was written inside it (no marker)
        assert!(!p.join(".vault-hq").exists());
    }
}
