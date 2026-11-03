//! App commands (SPEC 10): status, settings, reveal in Finder. Thin: each one hands its
//! arguments to `hq_core::hq::Hq` on a blocking thread, so a scan never stalls the UI.

use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::settings::Settings;
use hq_core::types::Status;
use hq_core::{Error, Result};
use tauri::State;

/// Run `f` against `Hq` off the async runtime's worker threads (file IO and scans block).
pub async fn blocking<T: Send + 'static>(
    hq: &Arc<Hq>,
    f: impl FnOnce(&Hq) -> Result<T> + Send + 'static,
) -> Result<T> {
    let hq = hq.clone();
    tauri::async_runtime::spawn_blocking(move || f(&hq))
        .await
        .map_err(|e| Error::Internal(format!("background task failed: {e}")))?
}

#[tauri::command]
pub async fn app_status(hq: State<'_, Arc<Hq>>) -> Result<Status> {
    blocking(&hq, |hq| Ok(hq.status())).await
}

#[tauri::command]
pub async fn settings_get(hq: State<'_, Arc<Hq>>) -> Result<Settings> {
    blocking(&hq, |hq| Ok(hq.settings())).await
}

/// `patch`: top-level settings keys to replace; vaults change through the vault commands.
#[tauri::command]
pub async fn settings_update(hq: State<'_, Arc<Hq>>, patch: serde_json::Value) -> Result<Settings> {
    blocking(&hq, move |hq| hq.settings_update(patch)).await
}

/// Show a note (by id), a vault (by id), `settings` or `cache` in Finder.
#[tauri::command]
pub async fn reveal(hq: State<'_, Arc<Hq>>, what: String) -> Result<()> {
    blocking(&hq, move |hq| {
        let path = hq.reveal_target(&what)?;
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&path)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Internal(format!(
                "Finder could not show {}.",
                path.display()
            )))
        }
    })
    .await
}
