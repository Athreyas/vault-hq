//! `Hq`: the one object the app and the MCP server hold. It owns the settings store, the
//! vault registry and its cached scans, and wraps the core modules so a caller passes ids
//! and text, never paths.
//!
//! It is `Send + Sync` and every method takes `&self`, so the app keeps it in an `Arc` as
//! Tauri state. Other streams' functions take `&Vault`, `&[Snapshot]`, `&Settings` or a
//! cache dir: get those from `vault()`, `snapshots()`, `settings()` and `cache_dir()`, and
//! call `invalidate()` after writing to a vault outside these methods.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use crate::error::{Error, Result};
use crate::settings::{Settings, Store};
use crate::types::{
    note_id, split_note_id, CaptureKind, CaptureResult, Day, DecideResult, Home, InboxItem,
    MoveResult, Need, NoteFilter, NoteId, NoteInput, NoteSummary, NoteView, SaveResult, Source,
    Status, TemplateInfo, Tips, VaultCandidate, VaultInfo, VaultLayout,
};
use crate::vault::registry::{self, Registry, Snapshot};
use crate::vault::{checkboxes, watch, Vault};
use crate::{capture, days, home, inbox, needs, notes, util};

struct State {
    settings: Settings,
    registry: Arc<Registry>,
}

pub struct Hq {
    store: Store,
    support_dir: PathBuf,
    cache_dir: PathBuf,
    state: RwLock<State>,
}

/// Settings keys that only the vault commands may change, so the blocked-name check and
/// the marker handling can't be bypassed by a patch.
const VAULT_KEYS: &[&str] = &["vaults", "primaryVault"];

impl Hq {
    /// Load settings from `support_dir` and open the vaults they list.
    pub fn open(support_dir: &Path, cache_dir: &Path) -> Result<Hq> {
        let store = Store::new(support_dir);
        let mut settings = store.load()?;
        let reg = Registry::load(&settings);
        if registry::reconcile(&mut settings, &reg) {
            store.save(&settings)?;
        }
        Ok(Hq {
            store,
            support_dir: support_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
            state: RwLock::new(State {
                settings,
                registry: Arc::new(reg),
            }),
        })
    }

    fn read(&self) -> RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn support_dir(&self) -> &Path {
        &self.support_dir
    }

    /// Where other streams keep rebuildable caches (vectors, the task index).
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    // ---- settings ----------------------------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.read().settings.clone()
    }

    /// Change settings with `f`, save them, and reopen the vaults when `reload` (a vault
    /// was added or removed, a name was blocked). Nothing changes if `f` or the save fails.
    pub fn change_settings<T>(
        &self,
        reload: bool,
        f: impl FnOnce(&mut Settings) -> Result<T>,
    ) -> Result<T> {
        let mut st = self.write();
        let mut next = st.settings.clone();
        let out = f(&mut next)?;
        let reg = reload.then(|| Registry::load(&next));
        if let Some(reg) = &reg {
            registry::reconcile(&mut next, reg);
        }
        self.store.save(&next)?;
        st.settings = next;
        if let Some(reg) = reg {
            st.registry = Arc::new(reg);
        }
        Ok(out)
    }

    /// `settings_update(patch)`: top-level keys of `patch` replace the current ones.
    pub fn settings_update(&self, patch: serde_json::Value) -> Result<Settings> {
        let serde_json::Value::Object(patch) = patch else {
            return Err(Error::Invalid("A settings change is a JSON object.".into()));
        };
        if let Some(k) = patch.keys().find(|k| VAULT_KEYS.contains(&k.as_str())) {
            return Err(Error::Invalid(format!(
                "\"{k}\" changes through the vault commands, not settings_update."
            )));
        }
        let reload = patch.contains_key("blockedVaultNames");
        self.change_settings(reload, |s| {
            let mut v = serde_json::to_value(&*s)?;
            if let serde_json::Value::Object(map) = &mut v {
                map.extend(patch);
            }
            *s = serde_json::from_value(v)
                .map_err(|e| Error::Invalid(format!("That settings change doesn't fit: {e}")))?;
            Ok(s.clone())
        })
    }

    // ---- vaults --------------------------------------------------------------------------------

    pub fn registry(&self) -> Arc<Registry> {
        self.read().registry.clone()
    }

    pub fn vaults(&self) -> Vec<VaultInfo> {
        self.registry().info()
    }

    pub fn vault(&self, id: &str) -> Result<Vault> {
        self.registry().get(id).cloned()
    }

    pub fn primary(&self) -> Result<Vault> {
        self.registry().primary().cloned()
    }

    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        self.registry().snapshot(id)
    }

    /// Every open vault with its current scan, primary first.
    pub fn snapshots(&self) -> Vec<Snapshot> {
        self.registry().snapshots()
    }

    /// Forget cached scans (one vault or all); the watcher calls this, so does every write.
    pub fn invalidate(&self, vault: Option<&str>) {
        self.registry().invalidate(vault);
    }

    fn info_of(&self, id: &str) -> Result<VaultInfo> {
        self.vaults()
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| Error::Internal("The vault was added but did not open.".into()))
    }

    pub fn vault_add(&self, path: &str) -> Result<VaultInfo> {
        let v = self.change_settings(true, |s| registry::add(s, Path::new(path)))?;
        self.info_of(&v.id)
    }

    pub fn vault_create(&self, path: &str, name: &str, layout: VaultLayout) -> Result<VaultInfo> {
        let v =
            self.change_settings(true, |s| registry::create(s, Path::new(path), name, layout))?;
        self.info_of(&v.id)
    }

    pub fn vault_remove(&self, id: &str) -> Result<Vec<VaultInfo>> {
        self.change_settings(true, |s| registry::remove(s, id))?;
        Ok(self.vaults())
    }

    pub fn vault_set_primary(&self, id: &str) -> Result<Vec<VaultInfo>> {
        self.change_settings(true, |s| registry::set_primary(s, id))?;
        Ok(self.vaults())
    }

    /// Folders under the usual places in `home` that look like vaults.
    pub fn vault_candidates(&self, home: &Path) -> Vec<VaultCandidate> {
        registry::candidates(&self.settings(), &registry::default_candidate_roots(home))
    }

    /// The vault and rel of a note id; the vault must be open.
    pub fn locate(&self, id: &str) -> Result<(Vault, String)> {
        let (vid, rel) = split_note_id(id).ok_or_else(|| {
            Error::Invalid(format!("\"{id}\" is not a note id (<vault>:<path>)."))
        })?;
        Ok((self.vault(vid)?, rel.to_string()))
    }

    /// The path `reveal(what)` shows in Finder: a note id, a vault id, `settings` or `cache`.
    pub fn reveal_target(&self, what: &str) -> Result<PathBuf> {
        match what {
            "settings" => return Ok(self.support_dir.join("settings.json")),
            "cache" => return Ok(self.cache_dir.clone()),
            _ => {}
        }
        if let Ok(v) = self.vault(what) {
            return Ok(v.root);
        }
        let (v, rel) = self.locate(what)?;
        notes::note_path(&v, &rel)
    }

    /// Watch every open vault. A change drops that vault's cached scan, then reaches
    /// `on_change(vault_id, rels)`. Restart the watchers after a vault is added or removed.
    pub fn watch(
        self: &Arc<Self>,
        on_change: impl Fn(&str, Vec<String>) + Send + Sync + 'static,
    ) -> Result<Vec<watch::Watcher>> {
        let on_change = Arc::new(on_change);
        let reg = self.registry();
        reg.vaults()
            .iter()
            .map(|v| {
                let me: Weak<Hq> = Arc::downgrade(self);
                let cb = on_change.clone();
                let id = v.id.clone();
                watch::watch(v, watch::DEBOUNCE, move |rels| {
                    if let Some(hq) = me.upgrade() {
                        hq.invalidate(Some(&id));
                    }
                    cb(&id, rels);
                })
            })
            .collect()
    }

    // ---- status ----------------------------------------------------------------------------------

    pub fn status(&self) -> Status {
        let today = util::today();
        let reg = self.registry();
        let snaps = reg.snapshots();
        let queue = self.needs();
        let open: Vec<_> = snaps
            .iter()
            .flat_map(|s| checkboxes::all(&s.scan.notes))
            .filter(|c| c.is_open())
            .collect();
        let d = days::day(&snaps, &today);
        let mut active: Vec<home::Project> = snaps
            .iter()
            .flat_map(home::projects)
            .filter(|p| p.status == "active")
            .collect();
        active.sort_by(|a, b| b.updated.cmp(&a.updated));
        let mut suggestions: Vec<String> = active
            .iter()
            .take(3)
            .map(|p| format!("What is left on {}?", p.title))
            .collect();
        if !d.agent_log.is_empty() {
            suggestions.push("What did the agents do today?".into());
        }
        if let Some(n) = queue.first() {
            let short: String = n.title.chars().take(70).collect();
            suggestions.push(format!("Why is \"{}\" waiting on me?", short.trim()));
        }
        Status {
            version: env!("CARGO_PKG_VERSION").into(),
            vaults: reg.info(),
            primary: reg.primary_id().map(str::to_string),
            problems: reg.problems().to_vec(),
            unreadable: snaps
                .iter()
                .flat_map(|s| {
                    s.scan
                        .unreadable
                        .iter()
                        .map(move |r| format!("{}: {r}", s.vault.name))
                })
                .collect(),
            needs: queue.len() as u32,
            tips: Tips {
                needs: queue.iter().take(3).map(|n| n.title.clone()).collect(),
                tasks_open: open.len() as u32,
                tasks_overdue: open
                    .iter()
                    .filter(|c| c.due.as_deref().is_some_and(|d| d < today.as_str()))
                    .count() as u32,
                inbox: inbox::items(&snaps, &today).len() as u32,
                today: (d.agent_log.len() + d.completed.len()) as u32,
                notes: snaps.iter().map(|s| s.scan.notes.len() as u32).sum(),
                suggestions,
            },
        }
    }

    // ---- notes -----------------------------------------------------------------------------------

    pub fn notes_list(&self, filter: &NoteFilter) -> Vec<NoteSummary> {
        notes::list(&self.snapshots(), filter)
    }

    /// A note by id, or by a reference resolved like a wiki link (primary vault first).
    pub fn note_get(&self, reference: &str) -> Result<NoteView> {
        let reg = self.registry();
        notes::get(&reg.snapshots(), reg.primary_id(), reference)
    }

    pub fn note_source(&self, id: &str) -> Result<Source> {
        let (v, rel) = self.locate(id)?;
        notes::source(&v, &rel)
    }

    pub fn note_save(&self, id: &str, text: &str, base: &str) -> Result<SaveResult> {
        let (v, rel) = self.locate(id)?;
        let r = notes::save(&v, &rel, text, base, &util::today())?;
        if !r.conflict {
            self.invalidate(Some(&v.id));
        }
        Ok(r)
    }

    pub fn note_create(&self, input: &NoteInput) -> Result<NoteView> {
        let v = self.registry().pick(input.vault.as_deref())?.clone();
        let rel = notes::create(&v, input, &util::today())?;
        self.invalidate(Some(&v.id));
        self.note_get(&note_id(&v.id, &rel))
    }

    /// Move or rename; with `dry_run` only report the notes whose links would change.
    pub fn note_move(&self, id: &str, to: &str, dry_run: bool) -> Result<MoveResult> {
        let (v, rel) = self.locate(id)?;
        let snap = self.snapshot(&v.id)?;
        let r = notes::move_note(&snap, &rel, to, dry_run, &util::today());
        if !dry_run {
            // even a failed move may have renamed the file before a link edit failed
            self.invalidate(Some(&v.id));
        }
        r
    }

    pub fn note_archive(&self, id: &str) -> Result<MoveResult> {
        let (v, rel) = self.locate(id)?;
        let snap = self.snapshot(&v.id)?;
        let r = notes::archive(&snap, &rel, &util::today());
        self.invalidate(Some(&v.id));
        r
    }

    pub fn templates_list(&self) -> Vec<TemplateInfo> {
        notes::templates(&self.snapshots())
    }

    // ---- capture -----------------------------------------------------------------------------------

    /// Quick capture. Notes land in today's capture note. Tasks and bookmarks are refused
    /// with Unavailable until the tasks (B) and bookmarks (F) streams are routed in here.
    pub fn capture(
        &self,
        text: &str,
        kind: Option<CaptureKind>,
        vault: Option<&str>,
    ) -> Result<CaptureResult> {
        let v = self.registry().pick(vault)?.clone();
        let r = match capture::classify(text, kind) {
            CaptureKind::Note => {
                capture::capture_note(&v, text, &util::today(), &util::now_hhmm())
            }
            CaptureKind::Task => Err(Error::Unavailable(
                "Capturing a task needs the tasks engine, which isn't in this build yet.".into(),
            )),
            CaptureKind::Bookmark => Err(Error::Unavailable(
                "Saving a link as a bookmark isn't in this build yet. Capture it as a note instead."
                    .into(),
            )),
        }?;
        self.invalidate(Some(&v.id));
        Ok(r)
    }

    /// One `- HH:MM who · text` line in today's daily note (MCP's `log`).
    pub fn day_log(&self, vault: Option<&str>, who: &str, text: &str) -> Result<NoteId> {
        let v = self.registry().pick(vault)?.clone();
        let today = util::today();
        let id = days::log(&v, who, text, &today, &util::now_hhmm(), &today)?;
        self.invalidate(Some(&v.id));
        Ok(id)
    }

    // ---- home, days, needs, inbox -------------------------------------------------------------------

    pub fn home(&self, date: Option<&str>) -> Result<Home> {
        let today = util::today();
        let date = match date {
            Some(d) => days::valid_date(d)?,
            None => today.clone(),
        };
        let snaps = self.snapshots();
        let queue = self.needs();
        Ok(home::home(&snaps, &date, &today, days::phase_now(), &queue))
    }

    pub fn days(&self, from: &str, span: u32) -> Result<Vec<Day>> {
        days::days(&self.snapshots(), from, span)
    }

    pub fn needs(&self) -> Vec<Need> {
        let snoozed = self.read().settings.snoozed.clone();
        needs::needs(&self.snapshots(), &util::today(), &snoozed)
    }

    pub fn needs_decide(&self, id: &str, outcome: &str, why: &str) -> Result<DecideResult> {
        let today = util::today();
        let snaps = self.snapshots();
        let snoozed = self.read().settings.snoozed.clone();
        let d = needs::decide(
            &snaps,
            id,
            outcome,
            why,
            &today,
            &util::now_hhmm(),
            &snoozed,
        );
        self.invalidate(None);
        let d = d?;
        self.change_settings(false, |s| {
            // past snoozes do nothing; drop them so settings don't grow forever
            s.snoozed.retain(|_, until| until.as_str() > today.as_str());
            if let Some((id, until)) = &d.snooze {
                s.snoozed.insert(id.clone(), until.clone());
            }
            Ok(())
        })?;
        Ok(DecideResult { result: d.result })
    }

    pub fn inbox(&self) -> Vec<InboxItem> {
        inbox::items(&self.snapshots(), &util::today())
    }

    pub fn inbox_archive(&self, id: &str) -> Result<MoveResult> {
        let (v, rel) = self.locate(id)?;
        let snap = self.snapshot(&v.id)?;
        let r = inbox::archive(&snap, &rel, &util::today());
        self.invalidate(Some(&v.id));
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_rules() {
        let support = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let hq = Hq::open(support.path(), cache.path()).unwrap();
        let s = hq
            .settings_update(serde_json::json!({ "theme": "light", "futureThing": 1 }))
            .unwrap();
        assert_eq!(s.theme, crate::settings::Theme::Light);
        assert!(hq
            .settings_update(serde_json::json!({ "vaults": [] }))
            .is_err());
        assert!(hq
            .settings_update(serde_json::json!({ "theme": 5 }))
            .is_err());
        assert!(hq.settings_update(serde_json::json!([1])).is_err());
        // saved, and read back by a new Hq
        let again = Hq::open(support.path(), cache.path()).unwrap();
        assert_eq!(again.settings().theme, crate::settings::Theme::Light);
        assert!(matches!(again.primary(), Err(Error::Unavailable(_))));
        assert!(matches!(
            again.capture("x", None, None),
            Err(Error::Unavailable(_))
        ));
    }

    #[test]
    fn is_shareable() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Hq>();
    }
}
