//! Small helpers every module uses.

use std::io::Read;

/// Today's date as `YYYY-MM-DD`, local time.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// `HH:MM`, local time.
pub fn now_hhmm() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

/// Random lowercase hex, from the OS random source.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// A task id: 6 characters of `[a-z0-9]` (SPEC 4.1).
pub fn task_id() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut buf = [0u8; 6];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

/// A file-name-safe slug: lowercase letters, digits and dashes, at most `max` chars.
pub fn slug(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= max {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "untitled".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(
            slug("Lay the main line — along the fence!", 60),
            "lay-the-main-line-along-the-fence"
        );
        assert_eq!(slug("???", 10), "untitled");
        assert!(slug(&"a".repeat(200), 40).len() <= 40);
    }

    #[test]
    fn ids() {
        let id = task_id();
        assert_eq!(id.len(), 6);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(random_hex(16).len(), 32);
    }
}
