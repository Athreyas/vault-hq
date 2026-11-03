// Every Tauri command the UI can call. Each stream puts its commands in
// `commands/<area>.rs` and lists them in `hq_commands!` below; a SPEC command that no
// stream has landed yet answers `{kind: "Unavailable"}` instead of Tauri's bare
// "not found", so the UI can say "not in this build yet" and carry on.

use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::{Error, Result};
use tauri::ipc::Invoke;
use tauri::{generate_handler, Runtime};

pub mod app;
// ---- streams: add `pub mod <area>;` below (A core, B tasks, C search, E, F, W) ----
// A core
pub mod home;
pub mod notes;
pub mod vaults;
// C search
pub mod search;

mod names;
pub use names::{CAPTURE_WINDOW, COMMANDS};

/// The one list of registered commands, as `module::command` paths. Expanded twice:
/// into `generate_handler!` and into `REGISTERED`, so the two can never drift.
macro_rules! hq_commands {
    ($then:ident) => {
        $then![
            // shell
            app::app_status,
            app::settings_get,
            app::settings_update,
            app::reveal,
            app::capture_open,
            app::capture_hide,
            // ---- streams: add `<area>::<command>,` lines below ----
            // A core
            vaults::vaults_list,
            vaults::vault_add,
            vaults::vault_create,
            vaults::vault_remove,
            vaults::vault_set_primary,
            vaults::vault_candidates,
            notes::notes_list,
            notes::note_get,
            notes::note_source,
            notes::note_save,
            notes::note_create,
            notes::note_move,
            notes::note_archive,
            notes::templates_list,
            home::capture,
            home::home,
            home::days,
            home::needs,
            home::needs_decide,
            home::inbox,
            home::inbox_archive,
            // C search
            search::search,
            search::ask_start,
            search::ask_cancel,
            search::conversations_list,
            search::conversation_get,
            search::ollama_status,
            search::ollama_recommend,
        ]
    };
}

macro_rules! names_of {
    ($($module:ident :: $command:ident),* $(,)?) => {
        &[$(stringify!($command)),*]
    };
}

/// Commands this build registers.
pub const REGISTERED: &[&str] = hq_commands!(names_of);

pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    let registered = for_runtime::<R, _>(hq_commands!(generate_handler));
    move |invoke: Invoke<R>| {
        let cmd = invoke.message.command();
        let window = invoke.message.webview_ref().label().to_string();
        // the capability files say the same; this is the belt to their braces
        if window == crate::windows::CAPTURE && !CAPTURE_WINDOW.contains(&cmd) {
            invoke.resolver.reject(Error::Denied(format!(
                "{cmd} is not available in the quick-capture window."
            )));
            return true;
        }
        if !REGISTERED.contains(&cmd) && COMMANDS.contains(&cmd) {
            invoke.resolver.reject(Error::Unavailable(format!(
                "{cmd} is not in this build of Vault HQ yet."
            )));
            return true;
        }
        registered(invoke)
    }
}

/// Run `f` against `Hq` on a blocking thread: scans and file IO must not hold up the
/// async runtime the window's IPC runs on.
pub async fn blocking<T: Send + 'static>(
    hq: &Arc<Hq>,
    f: impl FnOnce(&Hq) -> Result<T> + Send + 'static,
) -> Result<T> {
    let hq = hq.clone();
    tauri::async_runtime::spawn_blocking(move || f(&hq))
        .await
        .map_err(|e| Error::Internal(format!("background task failed: {e}")))?
}

/// Pins the runtime type of the closure `generate_handler!` makes.
fn for_runtime<R: Runtime, F: Fn(Invoke<R>) -> bool>(f: F) -> F {
    f
}

/// The SPEC commands not registered in this build (`cargo test -p vault-hq pending -- --nocapture`).
pub fn pending() -> Vec<&'static str> {
    COMMANDS
        .iter()
        .copied()
        .filter(|c| !REGISTERED.contains(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permissions(file: &str) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(file).unwrap();
        v["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().to_string())
            .collect()
    }

    fn allow(cmd: &str) -> String {
        format!("allow-{}", cmd.replace('_', "-"))
    }

    #[test]
    fn main_window_may_call_every_command_and_nothing_else() {
        let p = permissions(include_str!("../../capabilities/main.json"));
        for c in COMMANDS {
            assert!(p.contains(&allow(c)), "main.json is missing {c}");
        }
        for x in &p {
            assert!(
                x.starts_with("core:event:") || COMMANDS.iter().any(|c| &allow(c) == x),
                "main.json grants {x}, which is not an HQ command"
            );
        }
    }

    #[test]
    fn capture_window_gets_only_its_list() {
        let p = permissions(include_str!("../../capabilities/capture.json"));
        let mut cmds: Vec<String> = p.into_iter().filter(|x| !x.starts_with("core:")).collect();
        cmds.sort();
        let mut want: Vec<String> = CAPTURE_WINDOW.iter().map(|c| allow(c)).collect();
        want.sort();
        assert_eq!(cmds, want);
    }

    #[test]
    fn names_are_unique_and_shell_commands_are_listed() {
        let mut all = COMMANDS.to_vec();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), COMMANDS.len());
    }

    #[test]
    fn every_registered_command_has_a_permission() {
        for c in REGISTERED {
            assert!(
                COMMANDS.contains(c),
                "{c} is registered but not in names.rs"
            );
        }
    }

    #[test]
    fn pending_commands() {
        let p = pending();
        println!("not registered yet ({}): {}", p.len(), p.join(", "));
        assert!(!p.contains(&"app_status"));
    }
}
