// The two windows: the main one (closing hides it; HQ stays a hotkey away) and the small
// quick-capture window (menu-bar click or the capture hotkey; hides when it loses focus).
// Both load the bundled UI; anything else a page tries to open goes to the default app.

use std::process::Command;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const MAIN: &str = "main";
pub const CAPTURE: &str = "capture";

const CAPTURE_W: f64 = 560.0;
const CAPTURE_H: f64 = 148.0;

// A click on a link to anywhere outside the app becomes a navigation, which
// `on_navigation` below hands to macOS instead of loading it in the window.
const LINKS_JS: &str = r#"
document.addEventListener("click", (e) => {
  const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
  if (!a) return;
  const u = new URL(a.getAttribute("href"), location.href);
  if (u.origin !== location.origin) { e.preventDefault(); location.href = u.href; }
}, true);
"#;

/// The UI listens for these (App.tsx): the tray and hotkeys reach the page without IPC.
pub const OPEN_PALETTE_JS: &str = r#"window.dispatchEvent(new CustomEvent("hq:palette"));"#;
pub const OPEN_SETTINGS_JS: &str = r#"window.dispatchEvent(new CustomEvent("hq:settings"));"#;

/// The bundled UI (`tauri://localhost`, `http://tauri.localhost`) or, in a debug build,
/// the Vite dev server.
pub fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "http" | "https" => match url.host_str() {
            Some("tauri.localhost") => true,
            Some("localhost") | Some("127.0.0.1") => {
                cfg!(debug_assertions) && url.port() == Some(5173)
            }
            _ => false,
        },
        _ => false,
    }
}

/// Only these schemes are handed to macOS; anything else (file:, javascript:) is dropped.
pub fn opens_outside(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto")
}

fn navigation(url: &Url) -> bool {
    if is_app_url(url) {
        return true;
    }
    if opens_outside(url) {
        let _ = Command::new("/usr/bin/open").arg(url.as_str()).status();
    }
    false
}

pub fn build_main<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Vault HQ")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(18, 18, 18, 255))
        .initialization_script(LINKS_JS)
        .on_navigation(navigation)
        .build()
}

fn build_capture<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(app, CAPTURE, WebviewUrl::App("capture.html".into()))
        .title("Quick capture")
        .inner_size(CAPTURE_W, CAPTURE_H)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .background_color(tauri::window::Color(18, 18, 18, 255))
        .initialization_script(LINKS_JS)
        .on_navigation(navigation)
        .build()
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let w = match app.get_webview_window(MAIN) {
        Some(w) => w,
        None => match build_main(app) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("vault-hq: could not open the main window: {e}");
                return;
            }
        },
    };
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
}

/// Bring HQ forward and run `js` in it (open the palette or settings).
pub fn show_main_with<R: Runtime>(app: &AppHandle<R>, js: &str) {
    show_main(app);
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.eval(js);
    }
}

/// The palette hotkey: HQ in front with the palette open, or put away if it is in front.
pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
            let _ = w.hide();
            return;
        }
    }
    show_main_with(app, OPEN_PALETTE_JS);
}

/// Show the capture window, under the menu-bar icon when `near` is the click position,
/// else centred. The page clears and focuses its box on `capture://open`.
pub fn open_capture<R: Runtime>(app: &AppHandle<R>, near: Option<PhysicalPosition<f64>>) {
    let w = match app.get_webview_window(CAPTURE) {
        Some(w) => w,
        None => match build_capture(app) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("vault-hq: could not open quick capture: {e}");
                return;
            }
        },
    };
    match near {
        Some(p) => {
            let width = w.outer_size().map(|s| s.width as f64).unwrap_or(CAPTURE_W);
            let _ = w.set_position(PhysicalPosition::new(p.x - width / 2.0, p.y + 12.0));
        }
        None => {
            let _ = w.center();
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to(CAPTURE, "capture://open", ());
}

pub fn toggle_capture<R: Runtime>(app: &AppHandle<R>, near: Option<PhysicalPosition<f64>>) {
    if let Some(w) = app.get_webview_window(CAPTURE) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            return;
        }
    }
    open_capture(app, near);
}

pub fn hide_capture<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(CAPTURE) {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn app_urls_stay_in() {
        assert!(is_app_url(&u("tauri://localhost/index.html")));
        assert!(is_app_url(&u("http://tauri.localhost/capture.html")));
        assert!(!is_app_url(&u("https://example.com/")));
        assert!(!is_app_url(&u("http://localhost:4320/"))); // the v1 server is not the app
    }

    #[test]
    fn only_web_and_mail_leave() {
        assert!(opens_outside(&u("https://example.com/a")));
        assert!(opens_outside(&u("mailto:a@example.com")));
        assert!(!opens_outside(&u("file:///etc/hosts")));
        assert!(!opens_outside(&u("javascript:alert(1)")));
    }
}
