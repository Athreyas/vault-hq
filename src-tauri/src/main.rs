// Vault HQ as a Mac app. The vault itself is served by the local HQ server (the
// `local.vault-hq` LaunchAgent, scripts/service.sh); this app is its native front:
// a window that stays alive when closed, a menu-bar icon, a global hotkey and
// "open at login". Pages from anywhere but the local server open in their own app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::thread;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const PORT: u16 = 4320;
const SERVICE: &str = "local.vault-hq";
const HOTKEY_LABEL: &str = "⌃⌥Space";

// Links inside HQ pages: anything that isn't the local server (obsidian://, the web)
// is turned into a navigation, which `on_navigation` below hands to macOS instead.
const LINKS_JS: &str = r#"
document.addEventListener("click", (e) => {
  const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
  if (!a) return;
  const u = new URL(a.getAttribute("href"), location.href);
  if (u.origin !== location.origin) { e.preventDefault(); location.href = u.href; }
}, true);
"#;

// Open Settings (the page's own ⌘, handler) unless it is already open.
const SETTINGS_JS: &str = r#"
if (!document.querySelector(".settings")) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: ",", metaKey: true, bubbles: true }));
}
"#;

// Open the Ctrl+K palette unless it is already open.
const PALETTE_JS: &str = r#"
if (!document.querySelector(".palette")) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
}
"#;

fn server_url() -> Url {
    Url::parse(&format!("http://127.0.0.1:{PORT}/")).expect("static url")
}

fn server_up() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Ask launchd to start the HQ service if it isn't answering (it normally runs from login).
fn start_service() {
    let uid = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    if let Some(uid) = uid {
        let _ = Command::new("/bin/launchctl")
            .args(["kickstart", &format!("gui/{uid}/{SERVICE}")])
            .status();
    }
}

fn is_local(url: &Url) -> bool {
    url.scheme() == "tauri"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
            && url.port() == Some(PORT))
}

fn open_outside(url: &Url) {
    if matches!(url.scheme(), "http" | "https" | "obsidian" | "mailto") {
        let _ = Command::new("/usr/bin/open").arg(url.as_str()).status();
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn show(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The hotkey: bring HQ up with the palette open, or put it away if it is in front.
fn toggle(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        let _ = w.hide();
    } else {
        show(app);
        let _ = w.eval(PALETTE_JS);
    }
}

fn main() {
    // ---- G mcp: `vault-hq mcp` is the stdio MCP server, before any window, plugin or
    // single-instance check starts (an agent's server must not wake or focus the app)
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        if let Err(e) = hq_mcp::serve_stdio() {
            eprintln!("vault-hq mcp: {e}");
            std::process::exit(1);
        }
        return;
    }
    // ---- end G mcp

    let hotkey = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);

    let app = tauri::Builder::default()
        // a second launch (Dock, Spotlight) just brings the running one forward
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if *shortcut == hotkey && event.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .menu(|handle| Menu::default(handle)) // Edit menu, so copy and paste work in the page
        .setup(move |app| {
            let handle = app.handle().clone();

            let w = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Vault HQ")
                .inner_size(1440.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .theme(Some(tauri::Theme::Dark))
                .background_color(tauri::window::Color(18, 18, 18, 255))
                .initialization_script(LINKS_JS)
                .on_navigation(|url| {
                    eprintln!("vault-hq: navigation to {url} ({})", if is_local(url) { "in app" } else { "handed to macOS" });
                    if is_local(url) {
                        true
                    } else {
                        open_outside(url);
                        false
                    }
                })
                .build()?;

            // swap the "starting" page for HQ as soon as the server answers
            thread::spawn(move || {
                if !server_up() {
                    eprintln!("vault-hq: server not answering on :{PORT}, asking launchd to start {SERVICE}");
                    start_service();
                }
                for _ in 0..240 {
                    if server_up() {
                        let r = w.navigate(server_url());
                        eprintln!("vault-hq: server up, opening HQ: {r:?}");
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });

            // open at login: on by default the first time, then whatever the user picks
            let marker = app.path().app_data_dir()?.join("autostart-set");
            if !marker.exists() {
                let _ = app.autolaunch().enable();
                if let Some(dir) = marker.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&marker, "");
            }
            let at_login = app.autolaunch().is_enabled().unwrap_or(false);

            let open = MenuItem::with_id(app, "open", "Open Vault HQ", true, None::<&str>)?;
            let palette = MenuItem::with_id(app, "palette", format!("Command palette   {HOTKEY_LABEL}"), true, None::<&str>)?;
            let prefs = MenuItem::with_id(app, "settings", "Settings…   ⌘,", true, None::<&str>)?;
            let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "Open at login", true, at_login, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = PredefinedMenuItem::quit(app, Some("Quit Vault HQ"))?;
            let menu = Menu::with_items(app, &[&open, &palette, &prefs, &reload, &sep, &login, &sep, &quit])?;

            let login_item = login.clone();
            TrayIconBuilder::with_id("tray")
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("Vault HQ")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => show(app),
                    "palette" => {
                        show(app);
                        if let Some(w) = window(app) {
                            let _ = w.eval(PALETTE_JS);
                        }
                    }
                    "settings" => {
                        show(app);
                        if let Some(w) = window(app) {
                            let _ = w.eval(SETTINGS_JS);
                        }
                    }
                    "reload" => {
                        if let Some(w) = window(app) {
                            let _ = w.navigate(server_url());
                        }
                    }
                    "login" => {
                        let auto = app.autolaunch();
                        let on = auto.is_enabled().unwrap_or(false);
                        let _ = if on { auto.disable() } else { auto.enable() };
                        let _ = login_item.set_checked(auto.is_enabled().unwrap_or(!on));
                    }
                    _ => {}
                })
                .build(app)?;

            handle.global_shortcut().register(hotkey)?;
            Ok(())
        })
        // closing the window only hides it; HQ stays one hotkey away
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Vault HQ");

    app.run(|app, event| {
        // clicking the Dock icon brings the hidden window back
        if let RunEvent::Reopen { .. } = event {
            show(app);
        }
    });
}
