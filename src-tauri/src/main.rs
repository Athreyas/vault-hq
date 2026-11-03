// Vault HQ 2.0: one process. `vault-hq` is the Mac app (Rust core in-process, the
// bundled React UI over Tauri commands); `vault-hq mcp` is the same binary as an MCP
// stdio server for agents, started before any GUI code runs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkeys;
mod jobs;
mod state;
mod tray;
mod windows;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::ShortcutState;

use state::{Dirs, Shell};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        std::process::exit(match hq_mcp::serve_stdio() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("vault-hq mcp: {e}");
                1
            }
        });
    }
    run();
}

fn run() {
    let app = tauri::Builder::default()
        // a second launch (Dock, Spotlight, `open`) brings the running one forward
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let bound = app.state::<Shell>().bound();
                    if bound.palette.as_ref() == Some(shortcut) {
                        windows::toggle_main(app);
                    } else if bound.capture.as_ref() == Some(shortcut) {
                        windows::toggle_capture(app, None);
                    }
                })
                .build(),
        )
        .menu(tauri::menu::Menu::default) // Edit menu: copy and paste work
        .invoke_handler(commands::handler())
        .setup(|app| {
            let dirs = Dirs {
                support: app.path().app_data_dir()?,
                cache: app.path().app_cache_dir()?,
            };
            let shell = Shell::open(Dirs {
                support: dirs.support.clone(),
                cache: dirs.cache.clone(),
            })
            .unwrap_or_else(|e| {
                eprintln!("vault-hq: {e}");
                Shell::degraded(dirs, &e)
            });
            let keys = shell.settings().hotkeys;
            let tasks_cache = shell.dirs.cache.clone();
            app.manage(shell);
            // B tasks: answers from the cached index until the first command scans
            app.manage(commands::tasks::TasksState::new(tasks_cache));
            // ---- stream A: app.manage(hq_core::hq::Hq::open(...)?) goes here ----

            if cfg!(debug_assertions) {
                let pending = commands::pending();
                eprintln!(
                    "vault-hq: {} commands not in this build yet: {}",
                    pending.len(),
                    pending.join(", ")
                );
            }

            // open at login: on by default the first time, then whatever the user picks
            let marker = app.path().app_data_dir()?.join("autostart-set");
            if !marker.exists() {
                let _ = app.autolaunch().enable();
                if let Some(dir) = marker.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&marker, "");
            }

            windows::build_main(app.handle())?;
            tray::build(app)?;
            match hotkeys::parse_all(&keys) {
                Ok(b) => {
                    if let Err(e) = hotkeys::apply(app.handle(), b) {
                        eprintln!("vault-hq: {e}");
                    }
                }
                Err(e) => eprintln!("vault-hq: hotkeys in settings: {e}"),
            }
            jobs::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // closing the main window only hides it; HQ stays one hotkey away
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // quick capture behaves like a popover: click elsewhere and it goes away
            WindowEvent::Focused(false) if window.label() == windows::CAPTURE => {
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("failed to build Vault HQ");

    app.run(|app, event| {
        // clicking the Dock icon brings the hidden window back
        if let RunEvent::Reopen { .. } = event {
            windows::show_main(app);
        }
    });
}
