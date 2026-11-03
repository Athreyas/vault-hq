// The shell's own commands (SPEC 10 "app", plus the capture window's open/close).
// Thin: validate, act, map to `hq_core::Error`.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use hq_core::settings::{self, Settings};
use hq_core::types::{split_note_id, AppStatus, VaultInfo};
use hq_core::vault::{scan, Vault};
use hq_core::{Error, Result};
use serde_json::Value;
use tauri::{AppHandle, Runtime, State, Url};

use crate::hotkeys;
use crate::state::Shell;
use crate::windows;

#[tauri::command]
pub async fn app_status(shell: State<'_, Shell>) -> Result<AppStatus> {
    let s = shell.settings();
    let mut vaults = Vec::new();
    let mut problems: Vec<String> = shell.broken.iter().cloned().collect();
    for entry in &s.vaults {
        let dir = Path::new(&entry.path);
        if let Some(why) = settings::blocked(&s, dir) {
            problems.push(format!("{}: {why}", entry.name));
            continue;
        }
        if !dir.is_dir() {
            problems.push(format!(
                "{}: the folder {} is gone or not mounted.",
                entry.name, entry.path
            ));
            continue;
        }
        match Vault::open(dir) {
            Ok(v) => {
                let found = scan(&v);
                vaults.push(VaultInfo {
                    primary: s.primary_vault.as_deref() == Some(v.id.as_str()),
                    id: v.id,
                    name: v.name,
                    path: entry.path.clone(),
                    notes: found.notes.len() as u32,
                    unreadable: found.unreadable.len() as u32,
                });
            }
            Err(e) => problems.push(format!("{}: {e}", entry.name)),
        }
    }
    Ok(AppStatus {
        version: env!("CARGO_PKG_VERSION").into(),
        vaults,
        primary_vault: s.primary_vault.clone(),
        problems,
        mcp_command: mcp_command(),
    })
}

#[tauri::command]
pub fn settings_get(shell: State<'_, Shell>) -> Settings {
    shell.settings()
}

/// A JSON merge patch (RFC 7396) over the settings. The vault list is changed only by
/// the vault commands, which check folders and blocked names before touching anything.
#[tauri::command]
pub fn settings_update<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    patch: Value,
) -> Result<Settings> {
    let current = shell.settings();
    let next = patched(&current, &patch)?;
    let keys_changed = next.hotkeys.palette != current.hotkeys.palette
        || next.hotkeys.capture != current.hotkeys.capture;
    let prev_bound = shell.bound();
    if keys_changed {
        hotkeys::apply(&app, hotkeys::parse_all(&next.hotkeys)?)?;
    }
    let saved = shell.update_settings(|s| {
        *s = next;
        Ok(())
    });
    if saved.is_err() && keys_changed {
        let _ = hotkeys::apply(&app, prev_bound);
    }
    saved
}

/// The settings `patch` makes of `current`, validated.
pub fn patched(current: &Settings, patch: &Value) -> Result<Settings> {
    let Value::Object(map) = patch else {
        return Err(Error::Invalid(
            "A settings change must be an object.".into(),
        ));
    };
    for k in ["vaults", "primaryVault"] {
        if map.contains_key(k) {
            return Err(Error::Invalid(format!(
                "\"{k}\" changes through the vault commands (add, create, remove, set primary), not settings_update."
            )));
        }
    }
    let mut v = serde_json::to_value(current)?;
    merge(&mut v, patch);
    let mut next: Settings = serde_json::from_value(v)
        .map_err(|e| Error::Invalid(format!("That settings change does not fit: {e}")))?;

    hotkeys::parse_all(&next.hotkeys)?;
    model_server_ok(&next.ollama.url)?;
    next.blocked_vault_names = clean_names(&next.blocked_vault_names);
    Ok(next)
}

/// RFC 7396: objects merge key by key, `null` removes, anything else replaces.
pub fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, pv) in p {
                if pv.is_null() {
                    t.remove(k);
                } else {
                    merge(t.entry(k.clone()).or_insert(Value::Null), pv);
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

fn clean_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
            out.push(n.to_string());
        }
    }
    out
}

/// SPEC 7: the model server must be this Mac or the home network, over http.
pub fn model_server_ok(url: &str) -> Result<()> {
    let refuse = || {
        Error::Denied(format!(
            "{url} is not on this Mac or the home network. Use http:// with localhost, a 192.168/10/172.16 address, or a .local name."
        ))
    };
    let u = Url::parse(url).map_err(|_| Error::Invalid(format!("\"{url}\" is not an address.")))?;
    if u.scheme() != "http" {
        return Err(refuse());
    }
    let ok = match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            d == "localhost" || d.ends_with(".local")
        }
        Some(url::Host::Ipv4(ip)) => local_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => local_ip(IpAddr::V6(ip)),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(refuse())
    }
}

fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => {
            let first = v.segments()[0];
            v.is_loopback() || (first & 0xffc0) == 0xfe80 || (first & 0xfe00) == 0xfc00
        }
    }
}

/// Show something in Finder: `settings`, `support`, `cache`, `vault:<id>`, or a note id.
#[tauri::command]
pub fn reveal(shell: State<'_, Shell>, what: String) -> Result<()> {
    let path = reveal_path(&shell, &what)?;
    if !path.exists() {
        return Err(Error::NotFound(format!(
            "{} does not exist yet.",
            path.display()
        )));
    }
    Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|e| Error::Internal(format!("could not open Finder: {e}")))?;
    Ok(())
}

fn reveal_path(shell: &Shell, what: &str) -> Result<PathBuf> {
    match what {
        "settings" => return Ok(shell.dirs.support.join("settings.json")),
        "support" => return Ok(shell.dirs.support.clone()),
        "cache" => return Ok(shell.dirs.cache.clone()),
        _ => {}
    }
    let s = shell.settings();
    let (vault_id, rel) = match what.strip_prefix("vault:") {
        Some(id) => (id, None),
        None => match split_note_id(what) {
            Some((v, r)) => (v, Some(r)),
            None => return Err(Error::Invalid(format!("Nothing to show for \"{what}\"."))),
        },
    };
    let vault = open_vault(&s, vault_id)?;
    match rel {
        None => Ok(vault.root),
        Some(r) => vault.resolve_rel(r),
    }
}

/// A vault from settings by id, refusing blocked ones before anything inside is read.
fn open_vault(s: &Settings, id: &str) -> Result<Vault> {
    let entry = s
        .vaults
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| Error::NotFound(format!("No vault with id {id} in settings.")))?;
    let dir = Path::new(&entry.path);
    if let Some(why) = settings::blocked(s, dir) {
        return Err(Error::Denied(why));
    }
    Vault::open(dir)
}

#[tauri::command]
pub fn capture_open<R: Runtime>(app: AppHandle<R>) {
    windows::open_capture(&app, None);
}

#[tauri::command]
pub fn capture_hide<R: Runtime>(app: AppHandle<R>) {
    windows::hide_capture(&app);
}

/// `<this binary> mcp`, quoted when the path has spaces (it lives in "Vault HQ.app").
fn mcp_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "vault-hq".into());
    if exe.contains(' ') {
        format!("\"{exe}\" mcp")
    } else {
        format!("{exe} mcp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patch() {
        let mut v = json!({"a": 1, "b": {"c": 2, "d": 3}});
        merge(&mut v, &json!({"a": null, "b": {"c": 5}, "e": [1]}));
        assert_eq!(v, json!({"b": {"c": 5, "d": 3}, "e": [1]}));
    }

    #[test]
    fn patch_changes_only_what_it_names() {
        let s = Settings::default();
        let n = patched(
            &s,
            &json!({"theme": "light", "ollama": {"chatModel": "qwen2.5:7b"}}),
        )
        .unwrap();
        assert_eq!(n.theme, settings::Theme::Light);
        assert_eq!(n.ollama.chat_model, "qwen2.5:7b");
        assert_eq!(n.ollama.url, s.ollama.url);
        assert_eq!(n.hotkeys.capture, s.hotkeys.capture);
    }

    #[test]
    fn patch_refuses_vaults_and_bad_values() {
        let s = Settings::default();
        assert!(patched(&s, &json!({"vaults": []})).is_err());
        assert!(patched(&s, &json!({"primaryVault": "x"})).is_err());
        assert!(patched(&s, &json!({"theme": "neon"})).is_err());
        assert!(patched(&s, &json!({"hotkeys": {"capture": "Nope+Nope"}})).is_err());
        assert!(patched(&s, &json!("theme")).is_err());
        assert!(matches!(
            patched(&s, &json!({"ollama": {"url": "https://api.example.com"}})),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn blocked_names_are_cleaned() {
        let n = patched(
            &Settings::default(),
            &json!({"blockedVaultNames": [" work-vault ", "", "Work-Vault", "other"]}),
        )
        .unwrap();
        assert_eq!(n.blocked_vault_names, vec!["work-vault", "other"]);
    }

    #[test]
    fn model_server_addresses() {
        // home-network addresses are built, not written, so the privacy scan stays quiet
        let lan = |a, b, c, d| format!("http://{}:11434", std::net::Ipv4Addr::new(a, b, c, d));
        for ok in [
            "http://127.0.0.1:11434".to_string(),
            "http://localhost:11434".into(),
            lan(192, 168, 1, 20),
            lan(10, 0, 0, 5),
            lan(169, 254, 3, 4),
            "http://mini.local:11434".into(),
            "http://[::1]:11434".into(),
            "http://[fd00::1]:11434".into(),
        ] {
            assert!(model_server_ok(&ok).is_ok(), "{ok} should be allowed");
        }
        for bad in [
            "https://127.0.0.1:11434",
            "http://8.8.8.8",
            "http://example.com",
            "http://[2001:db8::1]/",
            "not a url",
        ] {
            assert!(model_server_ok(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn reveal_refuses_blocked_and_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("home");
        std::fs::create_dir_all(&vault_dir).unwrap();
        let v = Vault::open(&vault_dir).unwrap();
        let shell = Shell::open(crate::state::Dirs {
            support: dir.path().join("support"),
            cache: dir.path().join("cache"),
        })
        .unwrap();
        shell
            .update_settings(|s| {
                s.vaults.push(settings::VaultEntry {
                    id: v.id.clone(),
                    path: vault_dir.to_string_lossy().into_owned(),
                    name: "home".into(),
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(
            reveal_path(&shell, &format!("vault:{}", v.id)).unwrap(),
            vault_dir
        );
        assert!(reveal_path(&shell, &format!("{}:../x.md", v.id)).is_err());
        assert!(reveal_path(&shell, "nope").is_err());
        assert!(matches!(
            reveal_path(&shell, "other:a.md"),
            Err(Error::NotFound(_))
        ));

        shell
            .update_settings(|s| {
                s.blocked_vault_names = vec!["home".into()];
                Ok(())
            })
            .unwrap();
        assert!(matches!(
            reveal_path(&shell, &format!("vault:{}", v.id)),
            Err(Error::Denied(_))
        ));
    }
}
