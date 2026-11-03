//! `Hq`: the one object the app and the MCP server hold. It owns the settings store, the
//! vault registry and its cached scans, and wraps the core modules so a caller passes ids
//! and text, never paths.
//!
//! It is `Send + Sync` and every method takes `&self`, so the app keeps it in an `Arc` as
//! Tauri state. It is the only writer of settings.json in the app: the shell's settings
//! commands go through `change_settings` too. Other streams' functions take `&Vault`, `&[Snapshot]`, `&Settings` or a
//! cache dir: get those from `vault()`, `snapshots()`, `settings()` and `cache_dir()`, and
//! call `invalidate()` after writing to a vault outside these methods.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use crate::error::{Error, Result};
use crate::settings::{Settings, Store};
use crate::types::TaskInput;
use crate::types::{
    note_id, split_note_id, CaptureKind, CaptureResult, Day, DecideResult, Home, InboxItem,
    MoveResult, Need, NoteFilter, NoteId, NoteInput, NoteSummary, NoteView, SaveResult, Source,
    TemplateInfo, VaultCandidate, VaultInfo, VaultLayout,
};
use crate::vault::registry::{self, Registry, Snapshot};
use crate::vault::{watch, Vault};
use crate::{capture, days, home, inbox, needs, notes, tasks, util};

struct State {
    settings: Settings,
    registry: Arc<Registry>,
}

pub struct Hq {
    store: Store,
    support_dir: PathBuf,
    cache_dir: PathBuf,
    state: RwLock<State>,
    /// Set when settings.json could not be read: HQ runs on defaults and refuses to save,
    /// so the user's file is never overwritten by a reset.
    broken: Option<String>,
}

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
            broken: None,
        })
    }

    /// Defaults in memory, no vaults, saves refused: what the app runs on when `open`
    /// failed because settings.json doesn't parse.
    pub fn degraded(support_dir: &Path, cache_dir: &Path, why: &Error) -> Hq {
        let settings = Settings::default();
        Hq {
            store: Store::new(support_dir),
            support_dir: support_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
            state: RwLock::new(State {
                registry: Arc::new(Registry::load(&settings)),
                settings,
            }),
            broken: Some(format!(
                "Settings could not be read ({why}). HQ is running on defaults and will not save until settings.json is fixed or removed."
            )),
        }
    }

    /// Why settings can't be saved, when they couldn't be read at startup.
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
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

    /// Change settings with `f` on a copy, save it, and only then make it current, so a
    /// failed `f` or save changes nothing. The vaults are reopened when the vault list,
    /// the primary or the blocked names changed.
    pub fn change_settings<T>(&self, f: impl FnOnce(&mut Settings) -> Result<T>) -> Result<T> {
        if let Some(why) = &self.broken {
            return Err(Error::Unavailable(why.clone()));
        }
        let mut st = self.write();
        let mut next = st.settings.clone();
        let out = f(&mut next)?;
        let reload = next.vaults != st.settings.vaults
            || next.primary_vault != st.settings.primary_vault
            || next.blocked_vault_names != st.settings.blocked_vault_names;
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
        let v = self.change_settings(|s| registry::add(s, Path::new(path)))?;
        self.info_of(&v.id)
    }

    pub fn vault_create(&self, path: &str, name: &str, layout: VaultLayout) -> Result<VaultInfo> {
        let v = self.change_settings(|s| registry::create(s, Path::new(path), name, layout))?;
        self.info_of(&v.id)
    }

    pub fn vault_remove(&self, id: &str) -> Result<Vec<VaultInfo>> {
        self.change_settings(|s| registry::remove(s, id))?;
        Ok(self.vaults())
    }

    pub fn vault_set_primary(&self, id: &str) -> Result<Vec<VaultInfo>> {
        self.change_settings(|s| registry::set_primary(s, id))?;
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

    /// The path `reveal(what)` shows in Finder: `settings`, `support`, `cache`,
    /// `vault:<id>` (or a bare vault id), or a note id.
    pub fn reveal_target(&self, what: &str) -> Result<PathBuf> {
        match what {
            "settings" => return Ok(self.support_dir.join("settings.json")),
            "support" => return Ok(self.support_dir.clone()),
            "cache" => return Ok(self.cache_dir.clone()),
            _ => {}
        }
        if let Some(id) = what.strip_prefix("vault:") {
            return Ok(self.vault(id)?.root);
        }
        if let Ok(v) = self.vault(what) {
            return Ok(v.root);
        }
        let (v, rel) = self.locate(what)?;
        notes::note_path(&v, &rel)
    }

    /// Watch every open vault. A change drops that vault's cached scan, then reaches
    /// `on_change(vault_id, rels)`. One result per vault, so one that can't be watched
    /// doesn't stop the others. Restart the watchers after the open vaults change.
    pub fn watch(
        self: &Arc<Self>,
        on_change: impl Fn(&str, Vec<String>) + Send + Sync + 'static,
    ) -> Vec<Result<watch::Watcher>> {
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
        self.invalidate(Some(&v.id));
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

    /// Quick capture. Notes land in today's capture note, tasks go through the tasks
    /// engine (B); bookmarks are refused with Unavailable until stream F is routed in here.
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
            CaptureKind::Task => self.capture_task(&v, text),
            CaptureKind::Bookmark => Err(Error::Unavailable(
                "Saving a link as a bookmark isn't in this build yet. Capture it as a note instead."
                    .into(),
            )),
        }?;
        self.invalidate(Some(&v.id));
        Ok(r)
    }

    /// A task from the capture box, through stream B's `add` (to `09-Planning/Tasks.md`).
    fn capture_task(&self, v: &Vault, text: &str) -> Result<CaptureResult> {
        let description = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if description.is_empty() {
            return Err(Error::Invalid("Nothing to capture.".into()));
        }
        let vaults = std::slice::from_ref(v);
        let mut index = tasks::TaskIndex::build(vaults);
        let mut ctx = tasks::mutate::Ctx {
            vaults,
            index: &mut index,
            today: tasks::resolve_today(None)?,
        };
        let input = TaskInput {
            description,
            vault: Some(v.id.clone()),
            ..Default::default()
        };
        let t = tasks::mutate::add(&mut ctx, &input)?;
        Ok(CaptureResult {
            kind: CaptureKind::Task,
            id: t.note,
            line: t.raw,
        })
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
        self.change_settings(|s| {
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
    fn settings_are_saved_and_degraded_refuses() {
        let support = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let hq = Hq::open(support.path(), cache.path()).unwrap();
        hq.change_settings(|s| {
            s.theme = crate::settings::Theme::Light;
            Ok(())
        })
        .unwrap();
        // a failed change leaves memory and disk alone
        let r: Result<()> = hq.change_settings(|s| {
            s.theme = crate::settings::Theme::Dark;
            Err(Error::Invalid("no".into()))
        });
        assert!(r.is_err());
        assert_eq!(hq.settings().theme, crate::settings::Theme::Light);
        let again = Hq::open(support.path(), cache.path()).unwrap();
        assert_eq!(again.settings().theme, crate::settings::Theme::Light);
        assert!(matches!(again.primary(), Err(Error::Unavailable(_))));
        assert!(matches!(
            again.capture("x", None, None),
            Err(Error::Unavailable(_))
        ));
        assert_eq!(
            again.reveal_target("support").unwrap(),
            support.path().to_path_buf()
        );

        std::fs::write(support.path().join("settings.json"), "{ not json").unwrap();
        let e = match Hq::open(support.path(), cache.path()) {
            Err(e) => e,
            Ok(_) => panic!("must not open"),
        };
        let d = Hq::degraded(support.path(), cache.path(), &e);
        assert!(d.broken().is_some());
        assert!(d.change_settings(|_| Ok(())).is_err());
        assert_eq!(
            std::fs::read_to_string(support.path().join("settings.json")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn is_shareable() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Hq>();
    }
}
