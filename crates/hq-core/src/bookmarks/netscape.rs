//! Netscape bookmark HTML: what every browser's "Export bookmarks" writes, including
//! Safari's, which is the way in when the user won't grant Full Disk Access.
//!
//! The format is loose, unclosed `<DT>`s and `<p>`s everywhere, so this is a small tag
//! scanner, not an HTML parser: `<H3>` names the folder the next `<DL>` opens, `</DL>`
//! closes it, and `<A HREF>` is a bookmark.

use super::browsers::{date_from_unix, RawBookmark};

pub fn parse(html: &str) -> Vec<RawBookmark> {
    // ASCII lowercasing keeps byte offsets, so `lower` indexes `html` directly.
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let start = i + off;
        let Some(len) = lower[start..].find('>') else {
            break;
        };
        let end = start + len;
        let tag = &html[start + 1..end];
        let ltag = &lower[start + 1..end];
        i = end + 1;
        let name = ltag.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        match name {
            "h3" => {
                let close = lower[i..].find("</h3").map_or(i, |c| i + c);
                pending = Some(decode(&strip_tags(&html[i..close])).trim().to_string());
                i = close;
            }
            "dl" => stack.push(pending.take().unwrap_or_default()),
            "/dl" => {
                stack.pop();
            }
            "a" => {
                let attrs = attributes(tag);
                let close = lower[i..].find("</a").map_or(i, |c| i + c);
                let text = decode(&strip_tags(&html[i..close])).trim().to_string();
                i = close;
                let Some(url) = attrs
                    .iter()
                    .find(|(k, _)| k == "href")
                    .map(|(_, v)| decode(v))
                else {
                    continue;
                };
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                out.push(RawBookmark {
                    title: if text.is_empty() { url.clone() } else { text },
                    url,
                    folder: stack
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("/"),
                    added: get("add_date")
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .and_then(date_from_unix),
                    tags: get("tags")
                        .map(|t| {
                            decode(t)
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
            _ => {}
        }
    }
    out
}

/// `name=value` pairs of one tag, names lowercased; values quoted with `"`, `'`, or bare.
pub(crate) fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.split_once(char::is_whitespace).map_or("", |(_, r)| r);
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim().to_ascii_lowercase();
        // a valueless attribute before this one ("<A PRIVATE HREF=...>"): keep the last word
        let name = name
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or("")
            .to_string();
        let after = rest[eq + 1..].trim_start();
        let (value, next) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &after[1..];
                match body.find(q) {
                    Some(e) => (&body[..e], &body[e + 1..]),
                    None => (body, ""),
                }
            }
            _ => {
                let e = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..e], &after[e..])
            }
        };
        out.push((name, value.to_string()));
        rest = next;
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// The entities exports actually use, plus numeric ones.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let semi = rest
            .char_indices()
            .take(12)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i);
        let Some(semi) = semi else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FIXTURE: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<!-- This is an automatically generated file. -->
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1600000000" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks Bar</H3>
    <DL><p>
        <DT><A HREF="https://www.youtube.com/watch?v=abc&amp;t=10" ADD_DATE="1700000000" TAGS="video,talks">A talk &amp; Q&amp;A</A>
        <DT><H3>Dev &lt;tools&gt;</H3>
        <DL><p>
            <DT><A href='https://github.com/x/y' add_date=1700000000>x/y</A>
        </DL><p>
    </DL><p>
    <DT><A HREF="https://example.com/top"></A>
    <DT><A NAME="no-href">Anchor</A>
</DL><p>
"#;

    #[test]
    fn parses_nested_folders_and_attributes() {
        let b = parse(FIXTURE);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].url, "https://www.youtube.com/watch?v=abc&t=10");
        assert_eq!(b[0].title, "A talk & Q&A");
        assert_eq!(b[0].folder, "Bookmarks Bar");
        assert_eq!(b[0].tags, vec!["video", "talks"]);
        assert!(b[0].added.is_some());
        assert_eq!(b[1].folder, "Bookmarks Bar/Dev <tools>");
        assert_eq!(b[1].url, "https://github.com/x/y");
        assert_eq!(b[2].folder, "");
        assert_eq!(b[2].title, "https://example.com/top");
    }

    #[test]
    fn survives_broken_input() {
        assert!(parse("").is_empty());
        assert!(parse("<DL><DT><A HREF=\"https://a.example").is_empty());
        let b = parse("<A HREF=https://a.example/x>x");
        assert_eq!(b[0].url, "https://a.example/x");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode("a&#39;b&#x41;&bogus;&"), "a'bA&bogus;&");
        assert_eq!(decode("caf&#233;"), "café");
    }
}
