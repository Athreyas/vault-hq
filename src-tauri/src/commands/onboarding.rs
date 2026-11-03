// Onboarding commands (SPEC 10 "onboarding", stream E). Thin: validate, call
// hq_core::onboarding, map errors. Settings go through the shell's `Shell` state, the one
// writer of settings.json. Slow work (analysis, applying a plan) runs on a blocking thread.

use std::path::PathBuf;

use hq_core::ollama::{self, Models};
use hq_core::onboarding::{self, agents, import, restructure, scaffold};
use hq_core::settings::{self, OnboardingState, Settings};
use hq_core::types::{
    Agent, ConfigApplied, ConfigDiff, ImportAnalysis, ImportPlan, LocationSuggestion,
    RestructureReport, ScaffoldReport,
};
use hq_core::vault::Vault;
use hq_core::{Error, Result};
use tauri::State;

use crate::state::Shell;

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

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Internal(format!("background work failed: {e}")))?
}

#[tauri::command]
pub fn onboarding_state(shell: State<'_, Shell>) -> OnboardingState {
    onboarding::state(&shell.settings())
}

#[tauri::command]
pub fn onboarding_set_step(shell: State<'_, Shell>, step: String) -> Result<OnboardingState> {
    let s = shell.update_settings(|s| onboarding::set_step(s, &step))?;
    Ok(onboarding::state(&s))
}

/// Where a new vault could go (iCloud Drive first when it is on).
#[tauri::command]
pub fn onboarding_locations() -> Result<Vec<LocationSuggestion>> {
    Ok(onboarding::locations(&home()?))
}

/// Add the agent contract, per-folder notes, templates and (optionally) the welcome note
/// to a vault already registered (`vault_create` / `vault_add`). Never overwrites.
#[tauri::command]
pub async fn onboarding_scaffold(
    shell: State<'_, Shell>,
    vault_id: String,
    welcome: Option<bool>,
) -> Result<ScaffoldReport> {
    let s = shell.settings();
    blocking(move || {
        let v = vault_by_id(&s, &vault_id)?;
        scaffold::scaffold(
            &v.root,
            &scaffold::Options {
                name: &v.name,
                welcome: welcome.unwrap_or(true),
            },
        )
    })
    .await
}

#[tauri::command]
pub fn agents_catalog() -> Result<Vec<Agent>> {
    Ok(agents::catalog(&env()?))
}

#[tauri::command]
pub fn agent_prompt(shell: State<'_, Shell>, agent_id: String, vault_id: String) -> Result<String> {
    let s = shell.settings();
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

/// The user's chat model, when the server answers and has it; None means heuristics only.
fn chat_models(s: &Settings) -> Option<Models> {
    let models = Models::new(&s.ollama).ok()?;
    let installed = models.client.tags().ok()?;
    ollama::has_model(&installed, &models.chat_model).then_some(models)
}

#[tauri::command]
pub async fn import_analyze(shell: State<'_, Shell>, path: String) -> Result<ImportAnalysis> {
    let s = shell.settings();
    blocking(move || {
        let dir = checked_dir(&s, &path)?;
        match chat_models(&s) {
            Some(models) => {
                let chat = models.chat();
                let tagger = import::ModelTagger::new(&chat);
                let suggest = |text: &str| tagger.suggest(text);
                import::analyze(&dir, Some(&suggest))
            }
            None => import::analyze(&dir, None),
        }
    })
    .await
}

#[tauri::command]
pub async fn import_apply(shell: State<'_, Shell>, plan: ImportPlan) -> Result<RestructureReport> {
    let s = shell.settings();
    blocking(move || {
        checked_dir(&s, &plan.root)?;
        restructure::apply(&plan)
    })
    .await
}

/// Undo one `import_apply`, by the journal path its report returned.
#[tauri::command]
pub async fn import_undo(
    shell: State<'_, Shell>,
    path: String,
    journal: String,
) -> Result<RestructureReport> {
    let s = shell.settings();
    blocking(move || {
        let dir = checked_dir(&s, &path)?;
        restructure::undo(&dir, &journal)
    })
    .await
}
