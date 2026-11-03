// The menu-bar icon: a left click opens quick capture under it, a right click the menu.

use tauri::image::Image;
use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::hotkeys;
use crate::state::Shell;
use crate::windows;

pub fn build(app: &App<Wry>) -> tauri::Result<()> {
    let keys = app.state::<Shell>().settings().hotkeys;
    let with_key = |label: &str, key: &str| {
        let k = hotkeys::label(key);
        if k.is_empty() {
            label.to_string()
        } else {
            format!("{label}   {k}")
        }
    };
    let at_login = app.autolaunch().is_enabled().unwrap_or(false);

    let open = MenuItem::with_id(app, "open", "Open Vault HQ", true, None::<&str>)?;
    let capture = MenuItem::with_id(
        app,
        "capture",
        with_key("Quick capture", &keys.capture),
        true,
        None::<&str>,
    )?;
    let palette = MenuItem::with_id(
        app,
        "palette",
        with_key("Command palette", &keys.palette),
        true,
        None::<&str>,
    )?;
    let prefs = MenuItem::with_id(app, "settings", "Settings…   ⌘,", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
    let login =
        CheckMenuItem::with_id(app, "login", "Open at login", true, at_login, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    // macOS's standard About window, with the credits
    let about = PredefinedMenuItem::about(
        app,
        Some("About Vault HQ"),
        Some(AboutMetadata {
            name: Some("Vault HQ".into()),
            version: Some(app.package_info().version.to_string()),
            copyright: Some("© 2026 Athreyas · MIT license".into()),
            credits: Some(
                "Your data and your knowledge stay yours: plain files in folders you choose, \
                 models on your Mac, no account, no cloud.\n\n\
                 Envisioned and architected by Athreyas (monk97.me).\n\
                 Built with Claude Code."
                    .into(),
            ),
            website: Some("https://monk97.me".into()),
            website_label: Some("monk97.me".into()),
            ..Default::default()
        }),
    )?;
    let quit = PredefinedMenuItem::quit(app, Some("Quit Vault HQ"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open, &capture, &palette, &prefs, &reload, &sep, &login, &about, &sep, &quit,
        ],
    )?;

    let login_item = login.clone();
    TrayIconBuilder::with_id("tray")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("Vault HQ")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                windows::toggle_capture(tray.app_handle(), Some(position));
            }
        })
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => windows::show_main(app),
            "capture" => windows::open_capture(app, None),
            "palette" => windows::show_main_with(app, windows::OPEN_PALETTE_JS),
            "settings" => windows::show_main_with(app, windows::OPEN_SETTINGS_JS),
            "reload" => {
                if let Some(w) = app.get_webview_window(windows::MAIN) {
                    let _ = w.eval("location.reload()");
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
    Ok(())
}
