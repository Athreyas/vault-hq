//! App settings (SPEC 13): one JSON file in the app's support folder. Unknown keys are
//! kept, so an older build never drops a newer build's settings. Streams add fields only
//! in their own section, always with `#[serde(default)]`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // ---- shared / A core
    pub vaults: Vec<VaultEntry>,
    pub primary_vault: Option<String>,
    /// Folder names this Mac refuses to open, checked before reading anything inside.
    pub blocked_vault_names: Vec<String>,
    pub theme: Theme,
    pub hotkeys: Hotkeys,
    /// Needs-you items hidden until a date: need id -> `YYYY-MM-DD` it comes back.
    pub snoozed: BTreeMap<String, String>,
    // ---- C search
    pub ollama: OllamaSettings,
    // ---- E onboarding
    pub onboarding: OnboardingState,
    // ---- F bookmarks
    pub browsers: BTreeMap<String, bool>,
    pub enrich_bookmarks: bool,
    pub memories: MemoriesState,
    // ---- W widgets
    #[ts(type = "unknown")]
    pub widgets: serde_json::Value,

    #[serde(flatten)]
    #[ts(skip)]
    pub other: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntry {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub palette: String,
    pub capture: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys {
            palette: "Control+Alt+Space".into(),
            capture: "Alt+Super+Space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct OllamaSettings {
    pub url: String,
    pub chat_model: String,
    pub embed_model: String,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        OllamaSettings {
            url: "http://127.0.0.1:11434".into(),
            chat_model: "llama3.1:8b".into(),
            embed_model: "nomic-embed-text".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct OnboardingState {
    pub done: bool,
    pub step: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoriesState {
    pub dismissed: Vec<String>,
    pub last_surfaced: BTreeMap<String, String>,
}

/// `<support dir>/settings.json`.
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(support_dir: &Path) -> Self {
        Store {
            path: support_dir.join("settings.json"),
        }
    }

    pub fn load(&self) -> Result<Settings> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => {
                serde_json::from_str(&s).map_err(|e| Error::Invalid(format!("settings.json: {e}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Temp file + rename: settings are the app's own file, not a vault note.
    pub fn save(&self, s: &Settings) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(s)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

/// Why this Mac refuses the vault folder `dir`, or None.
pub fn blocked(settings: &Settings, dir: &Path) -> Option<String> {
    let name = dir.file_name()?.to_string_lossy().to_lowercase();
    settings
        .blocked_vault_names
        .iter()
        .any(|b| b.to_lowercase() == name)
        .then(|| format!("\"{name}\" is blocked on this Mac."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"theme":"light","futureThing":{"a":1}}"#,
        )
        .unwrap();
        let store = Store::new(dir.path());
        let s = store.load().unwrap();
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.ollama.embed_model, "nomic-embed-text");
        store.save(&s).unwrap();
        assert!(std::fs::read_to_string(dir.path().join("settings.json"))
            .unwrap()
            .contains("futureThing"));
    }

    #[test]
    fn blocks_by_name() {
        let s = Settings {
            blocked_vault_names: vec!["Work-Vault".into()],
            ..Default::default()
        };
        assert!(blocked(&s, Path::new("/x/work-vault")).is_some());
        assert!(blocked(&s, Path::new("/x/home-vault")).is_none());
    }
}
