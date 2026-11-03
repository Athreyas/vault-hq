//! Quick capture (SPEC 5): one line of text into `00-Inbox/YYYY-MM-DD-capture.md`.
//!
//! Only the note kind is handled here. A task (`⌘Enter`) goes to stream B's engine and a
//! bare URL to stream F's bookmarks; `classify` says which, and `Hq::capture` returns
//! `Error::Unavailable` for those kinds until the integrator routes them.

use std::path::Path;

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{note_id, CaptureKind, CaptureResult};
use crate::vault::{layout, Vault};

/// What a capture is: the kind asked for, else a bookmark when the text is one URL alone.
pub fn classify(text: &str, kind: Option<CaptureKind>) -> CaptureKind {
    if let Some(k) = kind {
        return k;
    }
    let t = text.trim();
    let one_word = !t.is_empty() && !t.contains(char::is_whitespace);
    if one_word && (t.starts_with("https://") || t.starts_with("http://")) {
        CaptureKind::Bookmark
    } else {
        CaptureKind::Note
    }
}

/// The capture note's opening text.
pub fn header(date: &str) -> String {
    format!(
        "{}# {date} capture\n\nQuick thoughts captured in Vault HQ. Sort them into a project or note on the weekly pass.\n\n",
        frontmatter::render(&[
            ("title", format!("{date} capture")),
            ("type", "capture".into()),
            ("created", date.into()),
            ("updated", date.into()),
            ("author", "human".into()),
            ("tags", "[inbox]".into()),
        ])
    )
}

/// Append a line with `locks::append` (which starts a new note with `header` under the
/// lock), then bump `updated:`.
pub fn append_line(
    path: &Path,
    header: impl FnOnce() -> String,
    line: &str,
    today: &str,
) -> Result<()> {
    locks::append(path, header, line)?;
    locks::guarded_edit(path, None, |cur| Ok(frontmatter::bump_updated(cur, today)))?;
    Ok(())
}

/// Append `- HH:MM text` to today's capture note in `vault`.
pub fn capture_note(vault: &Vault, text: &str, today: &str, hhmm: &str) -> Result<CaptureResult> {
    // one capture is one line: newlines would break the list
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(Error::Invalid("Nothing to capture.".into()));
    }
    let rel = layout::capture_rel(today);
    let path = vault.resolve_rel(&rel)?;
    let line = format!("- {hhmm} {text}");
    append_line(&path, || header(today), &line, today)?;
    Ok(CaptureResult {
        kind: CaptureKind::Note,
        id: note_id(&vault.id, &rel),
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(
            classify("https://example.com/a", None),
            CaptureKind::Bookmark
        );
        assert_eq!(
            classify("read https://example.com", None),
            CaptureKind::Note
        );
        assert_eq!(
            classify("buy seeds", Some(CaptureKind::Task)),
            CaptureKind::Task
        );
        assert_eq!(classify("ftp://x", None), CaptureKind::Note);
    }

    #[test]
    fn captures_create_then_append() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let r = capture_note(&v, "first\nthought", "2026-11-02", "09:00").unwrap();
        assert_eq!(r.line, "- 09:00 first thought");
        capture_note(&v, "second", "2026-11-02", "09:05").unwrap();
        let text =
            std::fs::read_to_string(dir.path().join("00-Inbox/2026-11-02-capture.md")).unwrap();
        assert!(text.starts_with("---\ntitle: 2026-11-02 capture\ntype: capture\n"));
        assert!(
            text.ends_with("weekly pass.\n\n- 09:00 first thought\n- 09:05 second\n"),
            "{text}"
        );
        assert!(capture_note(&v, "  \n ", "2026-11-02", "09:00").is_err());
    }
}
