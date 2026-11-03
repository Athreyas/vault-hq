//! Frontmatter: the `key: value` block between `---` lines at the top of a note (SPEC 3).
//! Edits are textual and minimal, so a note's other lines and their order never change.

use std::collections::BTreeMap;

pub struct Parsed<'a> {
    pub props: BTreeMap<String, String>,
    pub body: &'a str,
    /// Byte length of the frontmatter block including both `---` lines (0 if none).
    pub len: usize,
}

pub fn parse(text: &str) -> Parsed<'_> {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Parsed {
            props: BTreeMap::new(),
            body: text,
            len: 0,
        };
    };
    let start = text.len() - rest.len();
    let mut offset = start;
    let mut props = BTreeMap::new();
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        offset += line.len();
        if trimmed == "---" {
            return Parsed {
                props,
                body: &text[offset..],
                len: offset,
            };
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let k = k.trim();
            if !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let v = v.trim();
                let v = v
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(v);
                props.insert(k.to_string(), v.to_string());
            }
        }
    }
    // an opening line with no closing one is not frontmatter
    Parsed {
        props: BTreeMap::new(),
        body: text,
        len: 0,
    }
}

/// `[a, b]` → `["a", "b"]`; a bare value → one item; empty → none.
pub fn list(value: &str) -> Vec<String> {
    let v = value.trim();
    let inner = v
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(v);
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn format_list(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

/// Set (or add) one property, keeping every other line as it was. A note without
/// frontmatter is returned unchanged unless `create` is true.
pub fn set(text: &str, key: &str, value: &str, create: bool) -> String {
    let p = parse(text);
    if p.len == 0 {
        return if create {
            format!("---\n{key}: {value}\n---\n\n{text}")
        } else {
            text.to_string()
        };
    }
    let block = &text[..p.len];
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    let mut done = false;
    let lines: Vec<&str> = block.split_inclusive('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let is_close = i > 0 && trimmed == "---";
        if !done && !is_close {
            if let Some((k, _)) = trimmed.split_once(':') {
                if k.trim() == key {
                    out.push_str(&format!("{key}: {value}\n"));
                    done = true;
                    continue;
                }
            }
        }
        if is_close && !done {
            out.push_str(&format!("{key}: {value}\n"));
            done = true;
        }
        out.push_str(line);
    }
    out.push_str(p.body);
    out
}

/// Bump `updated:` to `date`, only when the note already has frontmatter.
pub fn bump_updated(text: &str, date: &str) -> String {
    if parse(text).len == 0 {
        text.to_string()
    } else {
        set(text, "updated", date, false)
    }
}

/// A complete frontmatter block for a new note, keys in the given order.
pub fn render(props: &[(&str, String)]) -> String {
    let mut s = String::from("---\n");
    for (k, v) in props {
        s.push_str(&format!("{k}: {v}\n"));
    }
    s.push_str("---\n\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str =
        "---\ntitle: Garden\ntags: [garden, outdoor]\nupdated: 2026-01-01\n---\n\n# Garden\n";

    #[test]
    fn parses() {
        let p = parse(NOTE);
        assert_eq!(p.props["title"], "Garden");
        assert_eq!(list(&p.props["tags"]), vec!["garden", "outdoor"]);
        assert_eq!(p.body, "\n# Garden\n");
    }

    #[test]
    fn no_frontmatter() {
        assert_eq!(parse("# Hi\n").len, 0);
        assert_eq!(parse("---\nnot closed\n").len, 0);
    }

    #[test]
    fn crlf() {
        let p = parse("---\r\ntitle: X\r\n---\r\nbody\r\n");
        assert_eq!(p.props["title"], "X");
        assert_eq!(p.body, "body\r\n");
    }

    #[test]
    fn set_replaces_and_adds() {
        let t = bump_updated(NOTE, "2026-11-02");
        assert!(t.contains("updated: 2026-11-02\n"));
        assert!(t.ends_with("---\n\n# Garden\n"));
        let t = set(NOTE, "status", "active", false);
        assert!(t.contains("updated: 2026-01-01\nstatus: active\n---\n"));
        assert_eq!(bump_updated("# plain\n", "2026-11-02"), "# plain\n");
    }
}
