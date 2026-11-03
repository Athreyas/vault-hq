// Global hotkeys from settings (`hotkeys.palette`, `hotkeys.capture`), written the way
// the global-shortcut crate reads them: "Control+Alt+Space", "Alt+Super+Space".

use std::str::FromStr;

use hq_core::settings::Hotkeys;
use hq_core::{Error, Result};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::{Bound, Shell};

/// An empty string turns that hotkey off.
pub fn parse(s: &str) -> Result<Option<Shortcut>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    Shortcut::from_str(s).map(Some).map_err(|e| {
        Error::Invalid(format!(
            "\"{s}\" is not a hotkey HQ understands ({e}). Write it like Alt+Super+Space."
        ))
    })
}

pub fn parse_all(h: &Hotkeys) -> Result<Bound> {
    let b = Bound {
        palette: parse(&h.palette)?,
        capture: parse(&h.capture)?,
    };
    if b.palette.is_some() && b.palette == b.capture {
        return Err(Error::Invalid(
            "The palette and quick-capture hotkeys must differ.".into(),
        ));
    }
    Ok(b)
}

/// "Alt+Super+Space" → "⌥⌘Space", for menus and hints.
pub fn label(s: &str) -> String {
    let mut mods = String::new();
    let mut key = String::new();
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => mods.push('⌃'),
            "alt" | "option" => mods.push('⌥'),
            "shift" => mods.push('⇧'),
            "super" | "cmd" | "command" | "commandorcontrol" | "cmdorctrl" => mods.push('⌘'),
            _ => key = part.strip_prefix("Key").unwrap_or(part).to_string(),
        }
    }
    format!("{mods}{key}")
}

/// Swap the registered hotkeys for `next`. If the system refuses one (another app owns
/// it), the previous ones are put back and the error names the hotkey.
pub fn apply<R: Runtime>(app: &AppHandle<R>, next: Bound) -> Result<()> {
    let shell = app.state::<Shell>();
    let gs = app.global_shortcut();
    let prev = shell.bound();
    for s in [prev.palette, prev.capture].into_iter().flatten() {
        let _ = gs.unregister(s);
    }
    let mut done = Vec::new();
    for s in [next.palette, next.capture].into_iter().flatten() {
        if let Err(e) = gs.register(s) {
            for d in done {
                let _ = gs.unregister(d);
            }
            for p in [prev.palette, prev.capture].into_iter().flatten() {
                let _ = gs.register(p);
            }
            return Err(Error::Unavailable(format!(
                "{} could not be registered; another app may be using it ({e}).",
                s.into_string()
            )));
        }
        done.push(s);
    }
    shell.set_bound(next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let b = parse_all(&Hotkeys::default()).unwrap();
        assert!(b.palette.is_some() && b.capture.is_some());
    }

    #[test]
    fn empty_is_off_and_garbage_is_refused() {
        assert_eq!(parse("").unwrap(), None);
        assert!(matches!(parse("Hyper+Banana"), Err(Error::Invalid(_))));
    }

    #[test]
    fn same_hotkey_twice_is_refused() {
        let h = Hotkeys {
            palette: "Alt+Super+Space".into(),
            capture: "Alt+Super+Space".into(),
        };
        assert!(parse_all(&h).is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(label("Alt+Super+Space"), "⌥⌘Space");
        assert_eq!(label("Control+Alt+Space"), "⌃⌥Space");
        assert_eq!(label("Shift+Super+KeyK"), "⇧⌘K");
        assert_eq!(label(""), "");
    }
}
