// Background jobs. For now: watch the vaults (FSEvents, through `Hq::watch`) and emit
// `vault://changed {vault, rels}` so open views refresh when an agent, an editor or a
// sync writes a note. Each change first drops that vault's cached scan in `Hq`, so the
// UI's reload reads fresh notes. The periodic re-index, the hourly bookmark re-scan and
// widget sampling (SPEC 2) join here as their streams land.

use std::sync::{Arc, Mutex};

use hq_core::hq::Hq;
use hq_core::types::VaultChanged;
use hq_core::vault::watch::Watcher;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The running watchers; replacing them drops (stops) the old ones.
#[derive(Default)]
pub struct Watchers(Mutex<Vec<Watcher>>);

/// The one place a vault change reaches the UI.
pub fn vault_changed<R: Runtime>(app: &AppHandle<R>, change: VaultChanged) {
    // indexes first, so a view that reloads on the event reads the new state
    if let Some(tasks) = app.try_state::<crate::commands::tasks::TasksState>() {
        tasks.notes_changed(&change.vault, &change.rels);
    }
    if let Err(e) = app.emit("vault://changed", change) {
        eprintln!("vault-hq: could not emit vault://changed: {e}");
    }
}

pub fn start<R: Runtime>(app: AppHandle<R>) {
    app.manage(Watchers::default());
    restart(&app);
}

/// Watch the vaults open right now. Call after a vault is added or removed, or the
/// blocked names change; blocked vaults are never open, so they are never watched.
pub fn restart<R: Runtime>(app: &AppHandle<R>) {
    let Some(hq) = app.try_state::<Arc<Hq>>().map(|s| s.inner().clone()) else {
        return;
    };
    let handle = app.clone();
    let watchers: Vec<Watcher> = hq
        .watch(move |vault, rels| {
            vault_changed(
                &handle,
                VaultChanged {
                    vault: vault.to_string(),
                    rels,
                },
            )
        })
        .into_iter()
        .filter_map(|w| w.map_err(|e| eprintln!("vault-hq: {e}")).ok())
        .collect();
    if let Some(slot) = app.try_state::<Watchers>() {
        *slot.0.lock().unwrap_or_else(|e| e.into_inner()) = watchers;
    }
}
