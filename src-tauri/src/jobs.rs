// Background jobs. For now: watch the vaults (FSEvents, through `Hq::watch`) and emit
// `vault://changed {vault, rels}` so open views refresh when an agent, an editor or a
// sync writes a note. Each change first drops that vault's cached scan in `Hq`, so the
// UI's reload reads fresh notes. The hourly bookmark re-scan (F) runs here too; the
// periodic re-index and widget sampling (SPEC 2) join as their streams land.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hq_core::hq::Hq;
use hq_core::types::VaultChanged;
use hq_core::vault::watch::Watcher;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::state::Shell;

/// SPEC 8: allowed browser profiles are re-read hourly.
const BOOKMARK_RESCAN: Duration = Duration::from_secs(60 * 60);

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
    start_bookmark_rescan(app);
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

/// F bookmarks: re-read every browser profile the user allowed, once an hour. The first
/// pass waits an hour too; the user's own import already ran when they ticked a browser.
fn start_bookmark_rescan<R: Runtime>(app: AppHandle<R>) {
    let spawned = thread::Builder::new()
        .name("hq-bookmarks".into())
        .spawn(move || loop {
            thread::sleep(BOOKMARK_RESCAN);
            let settings = app.state::<Shell>().settings();
            crate::commands::bookmarks::rescan(&app, &settings);
        });
    if let Err(e) = spawned {
        eprintln!("vault-hq: could not start the bookmark re-scan: {e}");
    }
}

/// Widget sampling (SPEC 12): tick the "This Mac" sampler every 5 s for its sparklines.
/// Without it the widget still samples on demand, just without history between asks.
pub fn start_widget_sampler<R: Runtime>(app: &AppHandle<R>) {
    use hq_core::widgets::system;
    let sampler = app
        .state::<crate::commands::widgets::WidgetsState>()
        .sampler();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Err(e) = system::spawn(sampler, system::TICK, stop) {
        eprintln!("vault-hq: could not start widget sampling: {e}");
    }
}
