//! Every write to an existing vault file goes through here (SPEC section 1, "Safe writes").
//!
//! The rules, shared with any other tool that writes the vault:
//! - take an exclusive `flock` on the file itself;
//! - compare the SHA-256 of what is on disk with the hash the writer read;
//! - write in place (truncate + write), never rename over: another writer may be holding
//!   the lock on this very inode, and a rename would swap the file away from it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Current, Error, Result};

pub fn sha256(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Exclusive advisory lock, released on drop (closing the file releases it too).
struct Locked(File);

impl Locked {
    fn open(path: &Path) -> Result<Self> {
        let f = OpenOptions::new().read(true).write(true).open(path)?;
        // SAFETY: flock on a valid, owned descriptor; blocks until the lock is free.
        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(Error::Internal(format!(
                "could not lock {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            )));
        }
        Ok(Locked(f))
    }

    fn read(&mut self) -> Result<String> {
        let mut s = String::new();
        self.0.seek(SeekFrom::Start(0))?;
        self.0.read_to_string(&mut s)?;
        Ok(s)
    }

    fn replace(&mut self, text: &str) -> Result<()> {
        self.0.seek(SeekFrom::Start(0))?;
        self.0.set_len(0)?;
        self.0.write_all(text.as_bytes())?;
        self.0.sync_data()?;
        Ok(())
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: unlocking our own descriptor.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// The text and its hash, read under the lock so it is never half-written.
pub fn read(path: &Path) -> Result<(String, String)> {
    let mut l = Locked::open(path)?;
    let text = l.read()?;
    let hash = sha256(&text);
    Ok((text, hash))
}

/// Replace the whole file with `new_text` if it still hashes to `base`.
/// On a mismatch nothing is written and the current text comes back in the error.
pub fn guarded_write(path: &Path, base: &str, new_text: &str) -> Result<String> {
    guarded_edit(path, Some(base), |_| Ok(new_text.to_string()))
}

/// Read under the lock, check `base` (if given), let `edit` compute the new text, write it.
/// `edit` returning the text unchanged writes nothing. Returns the new hash.
pub fn guarded_edit(
    path: &Path,
    base: Option<&str>,
    edit: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let mut l = Locked::open(path)?;
    let current = l.read()?;
    let hash = sha256(&current);
    if let Some(base) = base {
        if base != hash {
            return Err(Error::Conflict {
                current: Current {
                    text: current,
                    hash,
                },
            });
        }
    }
    let next = edit(&current)?;
    if next != current {
        l.replace(&next)?;
    }
    Ok(sha256(&next))
}

/// Create a new file; fails if it already exists (never overwrites).
pub fn create_new(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                Error::Invalid(format!("{} already exists", path.display()))
            }
            _ => e.into(),
        })?;
    f.write_all(text.as_bytes())?;
    f.sync_data()?;
    Ok(())
}

/// Edit `path` under its lock, creating it empty first when missing; `edit` sees `""` for
/// a new file. Never writes outside the lock: a new file is created empty (no truncate,
/// so a file another writer just made is opened as it is) and filled under `flock`.
pub fn edit_or_create(path: &Path, edit: impl FnOnce(&str) -> Result<String>) -> Result<String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    guarded_edit(path, None, edit)
}

/// Append under the lock, starting the file with `header` when it is new or empty.
///
/// The header is written in the same locked edit as the line. (Writing it with
/// `create_new` before taking the lock let a second writer append in between, and the
/// header, written at offset 0, then overwrote that writer's line.)
pub fn append(path: &Path, header: impl FnOnce() -> String, text: &str) -> Result<()> {
    edit_or_create(path, |cur| {
        let mut s = if cur.is_empty() {
            header()
        } else {
            cur.to_string()
        };
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(text);
        if !text.ends_with('\n') {
            s.push('\n');
        }
        Ok(s)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_and_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "one\r\ntwo\n").unwrap();
        let (text, base) = read(&p).unwrap();
        assert!(text.contains("\r\n"), "line endings are kept");
        let h2 = guarded_write(&p, &base, "changed\n").unwrap();
        assert_eq!(h2, sha256("changed\n"));
        match guarded_write(&p, &base, "stale\n") {
            Err(Error::Conflict { current }) => assert_eq!(current.text, "changed\n"),
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "changed\n");
    }

    #[test]
    fn in_place_keeps_inode() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "a\n").unwrap();
        let ino = std::fs::metadata(&p).unwrap().ino();
        guarded_edit(&p, None, |t| Ok(format!("{t}b\n"))).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().ino(), ino);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nb\n");
    }

    #[test]
    fn racing_first_appends_lose_no_line() {
        // regression: the header of a new file must never land on another writer's line
        for round in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join(format!("inbox/{round}.md"));
            let threads: Vec<_> = (0..6)
                .map(|i| {
                    let p = p.clone();
                    std::thread::spawn(move || {
                        append(&p, || "# Head\n".into(), &format!("- line {i}")).unwrap()
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            let text = std::fs::read_to_string(&p).unwrap();
            assert!(text.starts_with("# Head\n"), "{text}");
            assert_eq!(text.matches("# Head").count(), 1, "{text}");
            assert_eq!(
                text.lines().filter(|l| l.starts_with("- line")).count(),
                6,
                "{text}"
            );
        }
    }

    #[test]
    fn create_and_append() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/inbox.md");
        append(&p, || "# Inbox\n".into(), "- one").unwrap();
        append(&p, || unreachable!(), "- two").unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# Inbox\n- one\n- two\n"
        );
        assert!(matches!(create_new(&p, "x"), Err(Error::Invalid(_))));
    }
}
