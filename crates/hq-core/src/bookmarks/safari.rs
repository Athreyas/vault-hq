//! Safari `~/Library/Safari/Bookmarks.plist` (binary or XML plist). macOS lets an app read
//! it only with Full Disk Access; without it the read fails with EPERM, which HQ turns into
//! guidance instead of an error message nobody can act on.

use std::path::Path;

use plist::{Dictionary, Value};

use super::browsers::{date_from_unix, RawBookmark, SAFARI_GUIDANCE};
use crate::error::{Error, Result};

pub fn read(path: &Path) -> Result<Vec<RawBookmark>> {
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => Error::Denied(SAFARI_GUIDANCE.into()),
        std::io::ErrorKind::NotFound => Error::NotFound(
            "Safari has no bookmarks file yet. Save a bookmark in Safari, or import an exported HTML file."
                .into(),
        ),
        _ => e.into(),
    })?;
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> Result<Vec<RawBookmark>> {
    let v = Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Invalid(format!("Safari's Bookmarks.plist could not be read: {e}")))?;
    let root = v.as_dictionary().ok_or_else(|| {
        Error::Invalid("Safari's Bookmarks.plist has an unexpected shape.".into())
    })?;
    let mut out = Vec::new();
    walk(root, "", &mut out);
    Ok(out)
}

/// Safari's internal names for its fixed folders.
fn folder_name(title: &str) -> &str {
    match title {
        "BookmarksBar" => "Favorites",
        "BookmarksMenu" => "Bookmarks Menu",
        "com.apple.ReadingList" => "Reading List",
        t => t,
    }
}

fn walk(node: &Dictionary, folder: &str, out: &mut Vec<RawBookmark>) {
    let kind = node.get("WebBookmarkType").and_then(Value::as_string);
    match kind {
        Some("WebBookmarkTypeLeaf") => {
            let Some(url) = node.get("URLString").and_then(Value::as_string) else {
                return;
            };
            let title = node
                .get("URIDictionary")
                .and_then(Value::as_dictionary)
                .and_then(|d| d.get("title"))
                .and_then(Value::as_string)
                .filter(|t| !t.trim().is_empty())
                .unwrap_or(url);
            let added = node
                .get("ReadingList")
                .and_then(Value::as_dictionary)
                .and_then(|d| d.get("DateAdded"))
                .and_then(Value::as_date)
                .and_then(|d| {
                    let t: std::time::SystemTime = d.into();
                    t.duration_since(std::time::UNIX_EPOCH).ok()
                })
                .and_then(|d| date_from_unix(d.as_secs() as i64));
            out.push(RawBookmark {
                url: url.to_string(),
                title: title.trim().to_string(),
                folder: folder.to_string(),
                added,
                tags: Vec::new(),
            });
        }
        Some("WebBookmarkTypeList") | None => {
            let title = node.get("Title").and_then(Value::as_string).unwrap_or("");
            // History and other proxies show up as lists with no children; skipping is enough.
            let here = match (folder.is_empty(), folder_name(title)) {
                (_, "") => folder.to_string(),
                (true, name) => name.to_string(),
                (false, name) => format!("{folder}/{name}"),
            };
            if let Some(children) = node.get("Children").and_then(Value::as_array) {
                for c in children.iter().filter_map(Value::as_dictionary) {
                    walk(c, &here, out);
                }
            }
        }
        _ => {} // WebBookmarkTypeProxy (History) and anything newer
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Title</key><string></string>
  <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
  <key>Children</key>
  <array>
    <dict>
      <key>Title</key><string>History</string>
      <key>WebBookmarkType</key><string>WebBookmarkTypeProxy</string>
    </dict>
    <dict>
      <key>Title</key><string>BookmarksBar</string>
      <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
      <key>Children</key>
      <array>
        <dict>
          <key>URLString</key><string>https://news.ycombinator.com/item?id=1</string>
          <key>URIDictionary</key><dict><key>title</key><string>A thread</string></dict>
          <key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
        </dict>
        <dict>
          <key>Title</key><string>Tools</string>
          <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
          <key>Children</key>
          <array>
            <dict>
              <key>URLString</key><string>https://example.org/tool</string>
              <key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
            </dict>
          </array>
        </dict>
      </array>
    </dict>
    <dict>
      <key>Title</key><string>com.apple.ReadingList</string>
      <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
      <key>Children</key>
      <array>
        <dict>
          <key>URLString</key><string>https://example.org/paper.pdf</string>
          <key>URIDictionary</key><dict><key>title</key><string>Paper</string></dict>
          <key>ReadingList</key><dict><key>DateAdded</key><date>2024-02-03T10:00:00Z</date></dict>
          <key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>"#;

    #[test]
    fn parses_folders_and_reading_list() {
        let b = parse(FIXTURE.as_bytes()).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(
            (b[0].title.as_str(), b[0].folder.as_str()),
            ("A thread", "Favorites")
        );
        assert_eq!(b[1].folder, "Favorites/Tools");
        assert_eq!(b[1].title, "https://example.org/tool");
        assert_eq!(b[2].folder, "Reading List");
        assert!(b[2].added.as_deref().unwrap().starts_with("2024-02-0"));
    }

    #[test]
    fn eperm_becomes_guidance() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Bookmarks.plist");
        std::fs::write(&p, FIXTURE).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = read(&p);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        match r {
            Err(Error::Denied(msg)) => assert!(msg.contains("Full Disk Access")),
            // running as root ignores file modes; nothing to check then
            Ok(_) => {}
            Err(e) => panic!("expected guidance, got {e:?}"),
        }
    }

    #[test]
    fn garbage_is_invalid() {
        assert!(matches!(parse(b"nope"), Err(Error::Invalid(_))));
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read(&dir.path().join("Bookmarks.plist")),
            Err(Error::NotFound(_))
        ));
    }
}
