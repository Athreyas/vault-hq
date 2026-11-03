//! Which browsers and profiles exist on this Mac, and reading one of them.
//!
//! Detection only looks at folder and file names. A source's bookmark file is opened only
//! by [`read`], and the callers in `mod.rs` call that only for sources the user allowed.
//! Every path is built from `home`, so tests run against a fake home folder.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};
use crate::settings::Settings;
use crate::types::{BookmarkFormat, BrowserSource};

use super::{chromium, firefox, netscape, safari};

/// One bookmark as a browser stores it, before it becomes a note.
#[derive(Debug, Clone, PartialEq)]
pub struct RawBookmark {
    pub url: String,
    pub title: String,
    /// Folder path inside the browser, `/`-separated ("Bookmarks bar/Reading").
    pub folder: String,
    /// `YYYY-MM-DD`, local time, when the browser records it.
    pub added: Option<String>,
    /// Firefox and HTML exports can carry tags.
    pub tags: Vec<String>,
}

struct Chromium {
    browser: &'static str,
    name: &'static str,
    /// Under `~/Library/Application Support/`.
    dir: &'static str,
}

const CHROMIUM: &[Chromium] = &[
    Chromium {
        browser: "chrome",
        name: "Chrome",
        dir: "Google/Chrome",
    },
    Chromium {
        browser: "brave",
        name: "Brave",
        dir: "BraveSoftware/Brave-Browser",
    },
    Chromium {
        browser: "edge",
        name: "Edge",
        dir: "Microsoft Edge",
    },
    Chromium {
        browser: "arc",
        name: "Arc",
        dir: "Arc/User Data",
    },
    Chromium {
        browser: "dia",
        name: "Dia",
        dir: "Dia/User Data",
    },
    Chromium {
        browser: "vivaldi",
        name: "Vivaldi",
        dir: "Vivaldi",
    },
    Chromium {
        browser: "chromium",
        name: "Chromium",
        dir: "Chromium",
    },
];

const FIREFOX_PROFILES: &str = "Library/Application Support/Firefox/Profiles";
const SAFARI_DIR: &str = "Library/Safari";

/// Profiles Chromium keeps for itself; they never hold the user's bookmarks.
const SKIP_PROFILES: &[&str] = &["System Profile", "Guest Profile"];

/// What HQ tells the user when macOS keeps Safari's file from it.
pub const SAFARI_GUIDANCE: &str = "macOS protects Safari's bookmarks. To let Vault HQ read them, \
open System Settings → Privacy & Security → Full Disk Access, turn on Vault HQ, then scan again. \
Or, in Safari, choose File → Export → Bookmarks… and import that HTML file here instead.";

/// Where a source id points, after checking it can't name a path outside the browser's folder.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRef {
    pub id: String,
    pub browser: String,
    pub name: String,
    pub profile: String,
    pub format: BookmarkFormat,
    pub path: PathBuf,
}

impl SourceRef {
    pub fn label(&self) -> String {
        if self.profile.is_empty() {
            self.name.clone()
        } else {
            format!("{} · {}", self.name, self.profile)
        }
    }
}

/// A profile folder name must be one plain path component.
fn plain_component(s: &str) -> bool {
    let mut c = Path::new(s).components();
    matches!((c.next(), c.next()), (Some(Component::Normal(_)), None)) && !s.contains('/')
}

/// Turn a source id into its file, refusing anything malformed.
pub fn resolve(home: &Path, id: &str) -> Result<SourceRef> {
    let bad = || Error::Invalid(format!("\"{id}\" is not a bookmark source HQ knows."));
    if id == "safari" {
        return Ok(SourceRef {
            id: id.into(),
            browser: "safari".into(),
            name: "Safari".into(),
            profile: String::new(),
            format: BookmarkFormat::Safari,
            path: home.join(SAFARI_DIR).join("Bookmarks.plist"),
        });
    }
    let (browser, rest) = id.split_once(':').ok_or_else(bad)?;
    if browser == "html" {
        let path = PathBuf::from(rest);
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !path.is_absolute() || !(ext == "html" || ext == "htm") {
            return Err(Error::Invalid(
                "Pick a bookmarks file exported as HTML (.html or .htm).".into(),
            ));
        }
        return Ok(SourceRef {
            id: id.into(),
            browser: "html".into(),
            name: "HTML export".into(),
            profile: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            format: BookmarkFormat::Netscape,
            path,
        });
    }
    if !plain_component(rest) {
        return Err(bad());
    }
    if browser == "firefox" {
        return Ok(SourceRef {
            id: id.into(),
            browser: "firefox".into(),
            name: "Firefox".into(),
            profile: firefox_profile_name(rest),
            format: BookmarkFormat::Firefox,
            path: home.join(FIREFOX_PROFILES).join(rest).join("places.sqlite"),
        });
    }
    let c = CHROMIUM
        .iter()
        .find(|c| c.browser == browser)
        .ok_or_else(bad)?;
    Ok(SourceRef {
        id: id.into(),
        browser: c.browser.into(),
        name: c.name.into(),
        profile: rest.into(),
        format: BookmarkFormat::Chromium,
        path: home
            .join("Library/Application Support")
            .join(c.dir)
            .join(rest)
            .join("Bookmarks"),
    })
}

/// `abcd1234.default-release` → `default-release`: the part a person recognises.
fn firefox_profile_name(folder: &str) -> String {
    folder
        .split_once('.')
        .map(|(_, n)| n.to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| folder.to_string())
}

fn subdirs(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    out.sort();
    out
}

/// Every source that exists, by its file's presence alone.
pub fn installed(home: &Path) -> Vec<SourceRef> {
    let mut out = Vec::new();
    for c in CHROMIUM {
        let root = home.join("Library/Application Support").join(c.dir);
        for profile in subdirs(&root) {
            if SKIP_PROFILES.contains(&profile.as_str())
                || !root.join(&profile).join("Bookmarks").is_file()
            {
                continue;
            }
            if let Ok(r) = resolve(home, &format!("{}:{profile}", c.browser)) {
                out.push(r);
            }
        }
    }
    let ff = home.join(FIREFOX_PROFILES);
    for profile in subdirs(&ff) {
        if ff.join(&profile).join("places.sqlite").is_file() {
            if let Ok(r) = resolve(home, &format!("firefox:{profile}")) {
                out.push(r);
            }
        }
    }
    // Listing ~/Library/Safari needs Full Disk Access, so only the folder's presence is checked.
    if home.join(SAFARI_DIR).is_dir() {
        if let Ok(r) = resolve(home, "safari") {
            out.push(r);
        }
    }
    out
}

/// Read every bookmark in one source. The caller has checked the user allowed it.
pub fn read(home: &Path, id: &str) -> Result<Vec<RawBookmark>> {
    let r = resolve(home, id)?;
    read_ref(&r)
}

pub fn read_ref(r: &SourceRef) -> Result<Vec<RawBookmark>> {
    let missing = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(format!(
            "{} has no bookmarks file at {} any more.",
            r.label(),
            r.path.display()
        )),
        _ => e.into(),
    };
    match r.format {
        BookmarkFormat::Chromium => {
            let text = std::fs::read_to_string(&r.path).map_err(missing)?;
            chromium::parse(&text)
        }
        BookmarkFormat::Firefox => {
            if !r.path.is_file() {
                return Err(missing(std::io::ErrorKind::NotFound.into()));
            }
            firefox::read(&r.path)
        }
        BookmarkFormat::Safari => safari::read(&r.path),
        BookmarkFormat::Netscape => {
            let bytes = std::fs::read(&r.path).map_err(missing)?;
            Ok(netscape::parse(&String::from_utf8_lossy(&bytes)))
        }
    }
}

/// Detected sources with the user's choice, and counts for the allowed ones.
pub fn detect(home: &Path, settings: &Settings) -> Vec<BrowserSource> {
    installed(home)
        .into_iter()
        .map(|r| {
            let allowed = settings.browsers.get(&r.id).copied().unwrap_or(false);
            let mut s = BrowserSource {
                id: r.id.clone(),
                browser: r.browser.clone(),
                label: r.label(),
                profile: r.profile.clone(),
                format: r.format,
                path: r.path.display().to_string(),
                allowed,
                count: None,
                needs_access: false,
                guidance: None,
            };
            if allowed {
                match read_ref(&r) {
                    Ok(items) => s.count = Some(super::http_only(&items).count() as u32),
                    Err(Error::Denied(msg)) => {
                        s.needs_access = true;
                        s.guidance = Some(msg);
                    }
                    Err(e) => s.guidance = Some(e.to_string()),
                }
            } else if r.format == BookmarkFormat::Safari {
                s.guidance = Some(SAFARI_GUIDANCE.into());
            }
            s
        })
        .collect()
}

/// `YYYY-MM-DD` in local time for seconds since the Unix epoch; None for nonsense values.
pub fn date_from_unix(secs: i64) -> Option<String> {
    // Before 1995 or far in the future means the field was empty or in another unit.
    if !(788_918_400..=32_503_680_000).contains(&secs) {
        return None;
    }
    let dt = chrono::DateTime::from_timestamp(secs, 0)?;
    Some(
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn finds_profiles_by_their_files() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let chrome = h.join("Library/Application Support/Google/Chrome");
        touch(&chrome.join("Default/Bookmarks"), "{}");
        touch(&chrome.join("Profile 2/Bookmarks"), "{}");
        touch(&chrome.join("System Profile/Bookmarks"), "{}");
        std::fs::create_dir_all(chrome.join("Profile 3")).unwrap(); // no Bookmarks file
        touch(
            &h.join("Library/Application Support/Arc/User Data/Default/Bookmarks"),
            "{}",
        );
        touch(
            &h.join(FIREFOX_PROFILES)
                .join("ab12.default-release/places.sqlite"),
            "",
        );
        std::fs::create_dir_all(h.join(SAFARI_DIR)).unwrap();

        let ids: Vec<String> = installed(h).into_iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            vec![
                "chrome:Default",
                "chrome:Profile 2",
                "arc:Default",
                "firefox:ab12.default-release",
                "safari"
            ]
        );
        let ff = resolve(h, "firefox:ab12.default-release").unwrap();
        assert_eq!(ff.label(), "Firefox · default-release");
    }

    #[test]
    fn nothing_installed_is_an_empty_list() {
        let home = tempfile::tempdir().unwrap();
        assert!(installed(home.path()).is_empty());
        assert!(detect(home.path(), &Settings::default()).is_empty());
    }

    #[test]
    fn ids_cannot_escape() {
        let h = Path::new("/home/someone");
        for bad in [
            "chrome:../../etc",
            "chrome:a/b",
            "chrome:",
            "firefox:..",
            "nope:Default",
            "html:relative.html",
            "html:/tmp/notes.txt",
            "plain",
        ] {
            assert!(resolve(h, bad).is_err(), "{bad} must be refused");
        }
        assert!(resolve(h, "html:/tmp/export.HTML").is_ok());
    }

    #[test]
    fn counts_only_when_allowed() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
            {"type":"url","name":"A","url":"https://a.example/"},
            {"type":"url","name":"js","url":"javascript:void(0)"}]}}}"#;
        touch(
            &h.join("Library/Application Support/Google/Chrome/Default/Bookmarks"),
            json,
        );
        let mut s = Settings::default();
        let d = detect(h, &s);
        assert_eq!((d[0].allowed, d[0].count), (false, None));
        s.browsers.insert("chrome:Default".into(), true);
        let d = detect(h, &s);
        assert_eq!((d[0].allowed, d[0].count), (true, Some(1)));
    }

    #[test]
    fn dates() {
        assert!(date_from_unix(0).is_none());
        let d = date_from_unix(1_700_000_000).unwrap();
        assert!(d.starts_with("2023-11-1"), "{d}");
    }
}
