//! Vault commands (SPEC 10): list, add, create, remove, set primary, candidates.

use std::path::PathBuf;
use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::types::{VaultCandidate, VaultInfo, VaultLayout};
use hq_core::{Error, Result};
use tauri::{AppHandle, Runtime, State};

use super::blocking;
use crate::jobs;

#[tauri::command]
pub async fn vaults_list(hq: State<'_, Arc<Hq>>) -> Result<Vec<VaultInfo>> {
    blocking(&hq, |hq| Ok(hq.vaults())).await
}

#[tauri::command]
pub async fn vault_add<R: Runtime>(
    app: AppHandle<R>,
    hq: State<'_, Arc<Hq>>,
    path: String,
) -> Result<VaultInfo> {
    let v = blocking(&hq, move |hq| hq.vault_add(&path)).await?;
    jobs::restart(&app);
    Ok(v)
}

#[tauri::command]
pub async fn vault_create<R: Runtime>(
    app: AppHandle<R>,
    hq: State<'_, Arc<Hq>>,
    path: String,
    name: String,
    layout: Option<VaultLayout>,
) -> Result<VaultInfo> {
    let v = blocking(&hq, move |hq| {
        hq.vault_create(&path, &name, layout.unwrap_or_default())
    })
    .await?;
    jobs::restart(&app);
    Ok(v)
}

/// Forget a vault; its folder is left as it is.
#[tauri::command]
pub async fn vault_remove<R: Runtime>(
    app: AppHandle<R>,
    hq: State<'_, Arc<Hq>>,
    id: String,
) -> Result<Vec<VaultInfo>> {
    let v = blocking(&hq, move |hq| hq.vault_remove(&id)).await?;
    jobs::restart(&app);
    Ok(v)
}

#[tauri::command]
pub async fn vault_set_primary(hq: State<'_, Arc<Hq>>, id: String) -> Result<Vec<VaultInfo>> {
    blocking(&hq, move |hq| hq.vault_set_primary(&id)).await
}

#[tauri::command]
pub async fn vault_candidates(hq: State<'_, Arc<Hq>>) -> Result<Vec<VaultCandidate>> {
    blocking(&hq, |hq| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Unavailable("No home folder is set.".into()))?;
        Ok(hq.vault_candidates(&home))
    })
    .await
}
