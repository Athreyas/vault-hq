// Onboarding commands (SPEC 10 "onboarding", stream E). Thin: validate, call
// hq_core::onboarding, map errors. Settings are read and written through
// hq_core::settings::Store in the app's support folder; once the shell's settings state
// lands, swap `store()` for it so its cache never goes stale.

use std::path::{Path, PathBuf};

use hq_core::onboarding::{self, agents, import, restructure, scaffold};
use hq_core::settings::{self, OnboardingState, Settings, Store};
use hq_core::types::{
    Agent, ConfigApplied, ConfigDiff, ImportAnalysis, ImportPlan, LocationSuggestion,
    RestructureReport, ScaffoldReport,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use tauri::{AppHandle, Manager, Runtime};

fn store<R: Runtime>(app: &AppHandle<R>) -> Result<Store> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::Internal(format!("no app support folder: {e}")))?;
    Ok(Store::new(&dir))
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| Error::Internal("HOME is not set".into()))
}

/// The real places: this Mac's home, /Applications, and this very binary (agents start it
/// as `<exe> mcp`).
fn env() -> Result<agents::Env> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| Error::Internal(format!("can't find the Vault HQ binary: {e}")))?;
    Ok(agents::Env {
        home: home()?,
        apps: PathBuf::from("/Applications"),
        exe,
    })
}

fn checked_dir(s: &Settings, path: &str) -> Result<PathBuf> {
    let dir = PathBuf::from(path.trim());
    if !dir.is_absolute() {
        return Err(Error::Invalid(format!("{path} is not an absolute path.")));
    }
    if let Some(why) = settings::blocked(s, &dir) {
        return Err(Error::Denied(why));
    }
    Ok(dir)
}

fn vault_by_id(s: &Settings, id: &str) -> Result<Vault> {
    let entry = s
        .vaults
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| Error::NotFound(format!("No vault with id {id}.")))?;
    let dir = checked_dir(s, &entry.path)?;
    Vault::open(&dir)
}

#[tauri::command]
pub fn onboarding_state<R: Runtime>(app: AppHandle<R>) -> Result<OnboardingState> {
    onboarding::state(&store(&app)?)
}

#[tauri::command]
pub fn onboarding_set_step<R: Runtime>(app: AppHandle<R>, step: String) -> Result<OnboardingState> {
    onboarding::set_step(&store(&app)?, &step)
}

/// Where a new vault could go (iCloud Drive first when it is on).
#[tauri::command]
pub fn onboarding_locations() -> Result<Vec<LocationSuggestion>> {
    Ok(onboarding::locations(&home()?))
}

/// Add the agent contract, per-folder notes, templates and (optionally) the welcome note
/// to a vault already registered (`vault_create` / `vault_add`). Never overwrites.
#[tauri::command(async)]
pub fn onboarding_scaffold<R: Runtime>(
    app: AppHandle<R>,
    vault_id: String,
    welcome: Option<bool>,
) -> Result<ScaffoldReport> {
    let s = store(&app)?.load()?;
    let v = vault_by_id(&s, &vault_id)?;
    scaffold::scaffold(
        &v.root,
        &scaffold::Options {
            name: &v.name,
            welcome: welcome.unwrap_or(true),
        },
    )
}

#[tauri::command]
pub fn agents_catalog() -> Result<Vec<Agent>> {
    Ok(agents::catalog(&env()?))
}

#[tauri::command]
pub fn agent_prompt<R: Runtime>(
    app: AppHandle<R>,
    agent_id: String,
    vault_id: String,
) -> Result<String> {
    let s = store(&app)?.load()?;
    let entry = s
        .vaults
        .iter()
        .find(|v| v.id == vault_id)
        .ok_or_else(|| Error::NotFound(format!("No vault with id {vault_id}.")))?;
    agents::prompt(
        &agent_id,
        &agents::PromptVault {
            name: entry.name.clone(),
            path: entry.path.clone(),
        },
    )
}

#[tauri::command]
pub fn agent_config_preview(agent_id: String) -> Result<ConfigDiff> {
    agents::preview(&env()?, &agent_id)
}

/// Writes only after the user saw the preview; `base_hash` (from it) makes a config that
/// changed in between come back as a conflict instead of being overwritten.
#[tauri::command]
pub fn agent_config_apply(agent_id: String, base_hash: Option<String>) -> Result<ConfigApplied> {
    agents::apply(&env()?, &agent_id, base_hash.as_deref())
}

#[tauri::command(async)]
pub fn import_analyze<R: Runtime>(app: AppHandle<R>, path: String) -> Result<ImportAnalysis> {
    let s = store(&app)?.load()?;
    let dir = checked_dir(&s, &path)?;
    // Stream C's model client plugs in here as the tag suggester once it lands.
    import::analyze(&dir, None)
}

#[tauri::command(async)]
pub fn import_apply<R: Runtime>(app: AppHandle<R>, plan: ImportPlan) -> Result<RestructureReport> {
    let s = store(&app)?.load()?;
    checked_dir(&s, &plan.root)?;
    restructure::apply(&plan)
}

/// Undo one `import_apply`, by the journal path its report returned.
#[tauri::command(async)]
pub fn import_undo<R: Runtime>(
    app: AppHandle<R>,
    path: String,
    journal: String,
) -> Result<RestructureReport> {
    let s = store(&app)?.load()?;
    let dir = checked_dir(&s, &path)?;
    restructure::undo(Path::new(&dir), &journal)
}
