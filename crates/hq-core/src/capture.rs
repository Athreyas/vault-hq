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

/// Edit `path` under its lock, creating it empty first when missing; `edit` sees `""` for
/// a new file and writes its first text under the same lock.
///
/// Not `locks::append`: it writes a new file's header with `create_new` outside the lock,
/// so a second writer can append between the create and the header write, and the header
/// (written at offset 0) then overwrites that line. Creating the file empty and filling it
/// under `flock` leaves no such gap.
pub fn edit_or_create(path: &Path, edit: impl FnOnce(&str) -> Result<String>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // create(true) without truncate: an existing file, or one just made by another writer,
    // is opened as it is
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    locks::guarded_edit(path, None, edit)?;
    Ok(())
}

/// Append a line, starting the note with `header` when it is new or empty, and bump
/// `updated:`, all in one locked edit.
pub fn append_line(
    path: &Path,
    header: impl FnOnce() -> String,
    line: &str,
    today: &str,
) -> Result<()> {
    edit_or_create(path, |cur| {
        let mut s = if cur.is_empty() {
            header()
        } else {
            cur.to_string()
        };
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(line);
        if !line.ends_with('\n') {
            s.push('\n');
        }
        Ok(frontmatter::bump_updated(&s, today))
    })
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
    fn racing_first_writers_lose_no_line() {
        // the header of a brand-new note must never land on top of another writer's line
        for round in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("inbox/{round}.md"));
            let threads: Vec<_> = (0..6)
                .map(|i| {
                    let p = path.clone();
                    std::thread::spawn(move || {
                        append_line(&p, || "# Head\n".into(), &format!("- line {i}"), "d").unwrap()
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.starts_with("# Head\n"), "{text}");
            assert_eq!(
                text.lines().filter(|l| l.starts_with("- line")).count(),
                6,
                "{text}"
            );
        }
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
