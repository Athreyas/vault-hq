//! Chromium-family `Bookmarks` files (Chrome, Brave, Edge, Arc, Dia, Vivaldi, Chromium):
//! JSON with `roots.{bookmark_bar, other, synced}`, each a folder tree of
//! `{type: "url"|"folder", name, url, date_added, children}`.

use serde_json::Value;

use super::browsers::{date_from_unix, RawBookmark};
use crate::error::{Error, Result};

/// Seconds between 1601-01-01 (Chromium's epoch, in microseconds) and 1970-01-01.
const EPOCH_GAP: i64 = 11_644_473_600;

const ROOTS: &[(&str, &str)] = &[
    ("bookmark_bar", "Bookmarks bar"),
    ("other", "Other bookmarks"),
    ("synced", "Mobile bookmarks"),
];

pub fn parse(text: &str) -> Result<Vec<RawBookmark>> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        Error::Invalid(format!(
            "This browser's Bookmarks file isn't valid JSON: {e}"
        ))
    })?;
    let roots = v
        .get("roots")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Invalid("This browser's Bookmarks file has no \"roots\".".into()))?;
    let mut out = Vec::new();
    for (key, fallback) in ROOTS {
        if let Some(root) = roots.get(*key) {
            let name = root
                .get("name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
                .unwrap_or(fallback);
            walk(root, name, &mut out);
        }
    }
    Ok(out)
}

fn walk(node: &Value, folder: &str, out: &mut Vec<RawBookmark>) {
    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        let name = child.get("name").and_then(Value::as_str).unwrap_or("");
        match child.get("type").and_then(Value::as_str) {
            Some("url") => {
                let Some(url) = child.get("url").and_then(Value::as_str) else {
                    continue;
                };
                out.push(RawBookmark {
                    url: url.to_string(),
                    title: if name.trim().is_empty() { url } else { name }
                        .trim()
                        .to_string(),
                    folder: folder.to_string(),
                    added: child
                        .get("date_added")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<i64>().ok())
                        .and_then(|us| date_from_unix(us / 1_000_000 - EPOCH_GAP)),
                    tags: Vec::new(),
                });
            }
            Some("folder") => walk(child, &format!("{folder}/{name}"), out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "checksum": "0",
      "roots": {
        "bookmark_bar": {"type": "folder", "name": "Bookmarks bar", "children": [
          {"type": "url", "name": "Rust", "url": "https://www.rust-lang.org/",
           "date_added": "13340000000000000"},
          {"type": "folder", "name": "Reading", "children": [
            {"type": "url", "name": "", "url": "https://example.com/post?utm_source=x#top"}
          ]}
        ]},
        "other": {"type": "folder", "name": "", "children": [
          {"type": "url", "name": "Clip", "url": "https://youtu.be/abc"}
        ]},
        "synced": {"type": "folder", "name": "Mobile bookmarks", "children": []}
      },
      "version": 1
    }"#;

    #[test]
    fn parses_folders_names_and_dates() {
        let b = parse(FIXTURE).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].title, "Rust");
        assert_eq!(b[0].folder, "Bookmarks bar");
        // 13340000000000000 µs after 1601 is 2023-09-24 UTC
        assert!(b[0].added.as_deref().unwrap().starts_with("2023-09-2"));
        assert_eq!(b[1].folder, "Bookmarks bar/Reading");
        assert_eq!(
            b[1].title, b[1].url,
            "an unnamed bookmark is titled by its URL"
        );
        assert_eq!(b[2].folder, "Other bookmarks");
        assert_eq!(b[2].added, None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(parse("not json"), Err(Error::Invalid(_))));
        assert!(matches!(parse("{}"), Err(Error::Invalid(_))));
    }
}
