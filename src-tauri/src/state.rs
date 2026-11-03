// The app's managed state: where its files live and the settings every command reads.
//
// Seam for stream A: when `hq_core::hq::Hq` lands, `setup` in main.rs also does
// `app.manage(Hq::open(&shell.dirs.support, &shell.dirs.cache)?)`, the other streams'
// commands take `State<'_, Hq>`, and `Shell::settings`/`Shell::update_settings` delegate
// to the facade so there is one copy of the settings in memory. Until then the shell
// owns the settings file through `hq_core::settings::Store`.

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use hq_core::settings::{Settings, Store};
use hq_core::{Error, Result};
use tauri_plugin_global_shortcut::Shortcut;

pub struct Dirs {
    /// `~/Library/Application Support/<identifier>/`: settings.json.
    pub support: PathBuf,
    /// `~/Library/Caches/<identifier>/`: indexes, vectors (rebuildable).
    pub cache: PathBuf,
}

/// The global shortcuts currently registered, so the handler knows which one fired.
#[derive(Default, Clone, Copy)]
pub struct Bound {
    pub palette: Option<Shortcut>,
    pub capture: Option<Shortcut>,
}

pub struct Shell {
    pub dirs: Dirs,
    store: Store,
    settings: RwLock<Settings>,
    pub hotkeys: Mutex<Bound>,
    /// Set when settings.json could not be read: the app runs on defaults and refuses
    /// to save, so the user's file is never overwritten by a reset.
    pub broken: Option<String>,
}

impl Shell {
    /// A settings file that does not parse is an error the user must see, not a reset:
    /// starting with defaults would silently drop their vault list on the next save.
    pub fn open(dirs: Dirs) -> Result<Shell> {
        let store = Store::new(&dirs.support);
        let settings = store.load()?;
        Ok(Shell {
            dirs,
            store,
            settings: RwLock::new(settings),
            hotkeys: Mutex::new(Bound::default()),
            broken: None,
        })
    }

    /// Defaults in memory, saves refused: what the app runs on when `open` failed.
    pub fn degraded(dirs: Dirs, why: &Error) -> Shell {
        let store = Store::new(&dirs.support);
        Shell {
            dirs,
            store,
            settings: RwLock::new(Settings::default()),
            hotkeys: Mutex::new(Bound::default()),
            broken: Some(format!(
                "Settings could not be read ({why}). HQ is running on defaults and will not save until settings.json is fixed or removed."
            )),
        }
    }

    pub fn settings(&self) -> Settings {
        match self.settings.read() {
            Ok(s) => s.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Apply `f` to a copy, save it, and only then make it current, so a failed save
    /// leaves memory and disk in agreement.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings) -> Result<()>) -> Result<Settings> {
        if let Some(why) = &self.broken {
            return Err(Error::Unavailable(why.clone()));
        }
        let mut guard = self
            .settings
            .write()
            .map_err(|_| Error::Internal("settings lock poisoned".into()))?;
        let mut next = guard.clone();
        f(&mut next)?;
        self.store.save(&next)?;
        *guard = next.clone();
        Ok(next)
    }

    pub fn bound(&self) -> Bound {
        self.hotkeys.lock().map(|b| *b).unwrap_or_default()
    }

    pub fn set_bound(&self, b: Bound) {
        if let Ok(mut g) = self.hotkeys.lock() {
            *g = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(dir: &std::path::Path) -> Shell {
        Shell::open(Dirs {
            support: dir.join("support"),
            cache: dir.join("cache"),
        })
        .unwrap()
    }

    #[test]
    fn update_saves_then_applies() {
        let dir = tempfile::tempdir().unwrap();
        let s = shell(dir.path());
        s.update_settings(|x| {
            x.blocked_vault_names = vec!["private".into()];
            Ok(())
        })
        .unwrap();
        assert_eq!(s.settings().blocked_vault_names, vec!["private"]);
        // a fresh open reads what was saved
        assert_eq!(
            shell(dir.path()).settings().blocked_vault_names,
            vec!["private"]
        );
    }

    #[test]
    fn failed_update_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = shell(dir.path());
        let r = s.update_settings(|x| {
            x.blocked_vault_names = vec!["x".into()];
            Err(Error::Invalid("no".into()))
        });
        assert!(r.is_err());
        assert!(s.settings().blocked_vault_names.is_empty());
    }

    #[test]
    fn unreadable_settings_refuse_to_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("support")).unwrap();
        std::fs::write(dir.path().join("support/settings.json"), "{ not json").unwrap();
        let r = Shell::open(Dirs {
            support: dir.path().join("support"),
            cache: dir.path().join("cache"),
        });
        let Err(e) = r else { panic!("must not open") };
        assert!(matches!(e, Error::Invalid(_)));
        let d = Shell::degraded(
            Dirs {
                support: dir.path().join("support"),
                cache: dir.path().join("cache"),
            },
            &e,
        );
        assert!(d.update_settings(|_| Ok(())).is_err());
        // the broken file is left exactly as it was
        assert_eq!(
            std::fs::read_to_string(dir.path().join("support/settings.json")).unwrap(),
            "{ not json"
        );
    }
}
