//! Onboarding (SPEC 11): the first-run wizard's core. Owned by stream E.
//!
//! - `scaffold`: a new vault's folders, agent contract, templates and welcome note.
//! - `agents`: the agents catalog, each agent's prompt/skill text, and the MCP config writers.
//! - `import`: analysis of an existing (Obsidian) vault and the plan the user ticks.
//! - `restructure`: applying the ticked plan, guarded, with an undo journal.
//!
//! Wizard state lives in settings (`onboarding.step`, `onboarding.done`).

pub mod agents;
pub mod import;
pub mod restructure;
pub mod scaffold;

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::settings::{OnboardingState, Settings, Store, VaultEntry};
use crate::types::LocationSuggestion;
use crate::vault::Vault;

/// The wizard's steps, in order (SPEC 11).
pub const STEPS: [&str; 7] = [
    "welcome",
    "brain",
    "agents",
    "tasks",
    "ai",
    "bookmarks",
    "done",
];

pub fn state(store: &Store) -> Result<OnboardingState> {
    let s = store.load()?;
    let mut st = s.onboarding;
    if st.step.is_empty() {
        st.step = STEPS[0].into();
    }
    Ok(st)
}

/// Move the wizard to `step`. Reaching "done" marks onboarding finished; re-running it
/// later from Settings goes back to an earlier step without clearing `done`.
pub fn set_step(store: &Store, step: &str) -> Result<OnboardingState> {
    if !STEPS.contains(&step) {
        return Err(Error::Invalid(format!(
            "\"{step}\" is not an onboarding step (one of {})",
            STEPS.join(", ")
        )));
    }
    let mut s = store.load()?;
    s.onboarding.step = step.into();
    if step == "done" {
        s.onboarding.done = true;
    }
    store.save(&s)?;
    Ok(s.onboarding)
}

/// Add a vault to settings (once, by id); the first vault, or `primary`, becomes primary.
/// Stream A's registry owns this long term; onboarding needs it before that lands.
pub fn register_vault(settings: &mut Settings, vault: &Vault, primary: bool) {
    let path = vault.root.to_string_lossy().into_owned();
    match settings.vaults.iter_mut().find(|v| v.id == vault.id) {
        Some(v) => {
            v.path = path;
            v.name = vault.name.clone();
        }
        None => settings.vaults.push(VaultEntry {
            id: vault.id.clone(),
            path,
            name: vault.name.clone(),
        }),
    }
    if primary || settings.primary_vault.is_none() {
        settings.primary_vault = Some(vault.id.clone());
    }
}

/// iCloud Drive's folder under a home directory.
pub fn icloud_drive(home: &Path) -> PathBuf {
    home.join("Library/Mobile Documents/com~apple~CloudDocs")
}

/// Where a new vault could go. Only checks that the parent folders exist; never reads them.
pub fn locations(home: &Path) -> Vec<LocationSuggestion> {
    let mut out = Vec::new();
    let icloud = icloud_drive(home);
    if icloud.is_dir() {
        out.push(LocationSuggestion {
            label: "iCloud Drive".into(),
            path: icloud.join("Vault HQ").to_string_lossy().into_owned(),
            synced: true,
            note: "Best for several Macs: iCloud keeps the folder in step, so agents on every Mac write to the same brain.".into(),
        });
    }
    out.push(LocationSuggestion {
        label: "Documents".into(),
        path: home
            .join("Documents/Vault HQ")
            .to_string_lossy()
            .into_owned(),
        synced: false,
        note: "This Mac only (unless Documents is in iCloud). Move it later by moving the folder."
            .into(),
    });
    out
}

/// `~/x` for a path under `home`, for display.
pub fn tilde(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert_eq!(state(&store).unwrap().step, "welcome");
        assert!(!set_step(&store, "agents").unwrap().done);
        assert!(set_step(&store, "nope").is_err());
        assert!(set_step(&store, "done").unwrap().done);
        let again = set_step(&store, "brain").unwrap();
        assert!(again.done, "re-running keeps onboarding finished");
        assert_eq!(state(&store).unwrap().step, "brain");
    }

    #[test]
    fn registers_once_and_first_is_primary() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let va = Vault::open(a.path()).unwrap();
        let vb = Vault::open(b.path()).unwrap();
        let mut s = Settings::default();
        register_vault(&mut s, &va, false);
        register_vault(&mut s, &va, false);
        register_vault(&mut s, &vb, false);
        assert_eq!(s.vaults.len(), 2);
        assert_eq!(s.primary_vault.as_deref(), Some(va.id.as_str()));
        register_vault(&mut s, &vb, true);
        assert_eq!(s.primary_vault.as_deref(), Some(vb.id.as_str()));
    }

    #[test]
    fn suggests_icloud_only_when_present() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(locations(home.path()).len(), 1);
        std::fs::create_dir_all(icloud_drive(home.path())).unwrap();
        let l = locations(home.path());
        assert!(l[0].synced && l[0].path.ends_with("com~apple~CloudDocs/Vault HQ"));
        assert_eq!(tilde(home.path(), &home.path().join("a/b")), "~/a/b");
    }
}
