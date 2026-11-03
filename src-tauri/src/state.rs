// The app's managed state: where its files live, the hotkeys bound, and the core.
//
// Settings live in one place, `hq_core::hq::Hq` (also managed on its own as `Arc<Hq>`
// for the core commands): `Shell::settings`/`update_settings` delegate to it, so there is
// one copy in memory and one writer of settings.json.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hq_core::hq::Hq;
use hq_core::settings::Settings;
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
    /// The same `Hq` the core commands get as `State<Arc<Hq>>`.
    pub hq: Arc<Hq>,
    pub hotkeys: Mutex<Bound>,
}

impl Shell {
    /// A settings file that does not parse is an error the user must see, not a reset:
    /// starting with defaults would silently drop their vault list on the next save.
    pub fn open(dirs: Dirs) -> Result<Shell> {
        let hq = Hq::open(&dirs.support, &dirs.cache)?;
        Ok(Shell {
            hq: Arc::new(hq),
            hotkeys: Mutex::new(Bound::default()),
        })
    }

    /// Defaults in memory, saves refused: what the app runs on when `open` failed.
    pub fn degraded(dirs: Dirs, why: &Error) -> Shell {
        let hq = Hq::degraded(&dirs.support, &dirs.cache, why);
        Shell {
            hq: Arc::new(hq),
            hotkeys: Mutex::new(Bound::default()),
        }
    }

    /// Why settings can't be saved this run, if they couldn't be read.
    pub fn broken(&self) -> Option<&str> {
        self.hq.broken()
    }

    pub fn settings(&self) -> Settings {
        self.hq.settings()
    }

    /// Apply `f` to a copy, save it, and only then make it current, so a failed save
    /// leaves memory and disk in agreement. Vaults reopen if the list or blocks changed.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings) -> Result<()>) -> Result<Settings> {
        self.hq.change_settings(|s| {
            f(s)?;
            Ok(s.clone())
        })
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
