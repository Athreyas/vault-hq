//! Filling bookmarks in: a pasted link's page title and description (one fetch, for that
//! URL only, on the user's action), and tags plus a one-line summary from the user's own
//! chat model (opt-in). The model is injected as a plain function, so the core doesn't
//! depend on which client talks to it and tests need no server.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::netscape;
use super::store::{self, fm_value, split_body, Stored};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::types::{Bookmark, EnrichReport};
use crate::util;
use crate::vault::Vault;

const TIMEOUT: Duration = Duration::from_secs(8);
/// Enough for any page's <head>; a big page is cut off, never read whole.
const MAX_BYTES: u64 = 512 * 1024;
const MAX_TEXT: usize = 300;
const MAX_NEW_TAGS: usize = 5;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageMeta {
    pub title: Option<String>,
    pub description: Option<String>,
}

/// GET one page and read its title and description. http and https only.
pub fn fetch_page(url: &str) -> Result<PageMeta> {
    let u = url::Url::parse(url).map_err(|_| Error::Invalid(format!("{url} is not a link.")))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(Error::Invalid(
            "Only http and https pages are fetched.".into(),
        ));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(5)
        .user_agent("Vault HQ (bookmark title)")
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into();
    let mut resp = agent
        .get(u.as_str())
        .header("Accept", "text/html,application/xhtml+xml")
        .call()
        .map_err(|e| Error::Unavailable(format!("Could not fetch {url}: {e}")))?;
    let html = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|ct| ct.to_ascii_lowercase().contains("html"));
    if !html {
        return Ok(PageMeta::default());
    }
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| Error::Unavailable(format!("Could not read {url}: {e}")))?;
    Ok(parse_page(&String::from_utf8_lossy(&buf)))
}

fn clean(s: &str) -> Option<String> {
    let s = netscape::decode(s);
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return None;
    }
    Some(match s.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s,
    })
}

/// The best title (`og:title`, then `<title>`) and description (`og:description`, then
/// `description`) in a page's markup.
pub fn parse_page(html: &str) -> PageMeta {
    let lower = html.to_ascii_lowercase();
    let mut og_title = None;
    let mut og_desc = None;
    let mut desc = None;
    let mut at = 0;
    while let Some(off) = lower[at..].find("<meta") {
        let start = at + off;
        let Some(len) = lower[start..].find('>') else {
            break;
        };
        let attrs = netscape::attributes(&html[start + 1..start + len]);
        at = start + len;
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let key = get("property")
            .or(get("name"))
            .unwrap_or("")
            .to_ascii_lowercase();
        let Some(content) = get("content").and_then(clean) else {
            continue;
        };
        match key.as_str() {
            "og:title" | "twitter:title" if og_title.is_none() => og_title = Some(content),
            "og:description" | "twitter:description" if og_desc.is_none() => {
                og_desc = Some(content)
            }
            "description" if desc.is_none() => desc = Some(content),
            _ => {}
        }
    }
    let title = lower.find("<title").and_then(|s| {
        let open = s + lower[s..].find('>')? + 1;
        let close = open + lower[open..].find("</title")?;
        clean(&html[open..close])
    });
    PageMeta {
        title: og_title.or(title),
        description: og_desc.or(desc),
    }
}

/// Put a fetched page's title and description on its note, without overwriting what the
/// user (or an earlier enrichment) wrote: the title only while it is still the bare URL.
pub fn apply_page(vault: &Vault, id: &str, meta: &PageMeta) -> Result<Bookmark> {
    store::update(vault, id, |text| {
        let p = frontmatter::parse(text);
        let mut t = text.to_string();
        if let Some(title) = &meta.title {
            if p.props.get("title") == p.props.get("url") {
                t = frontmatter::set(&t, "title", &fm_value(title), false);
            }
        }
        if let Some(d) = &meta.description {
            if split_body(p.body).0.is_none() {
                t = store::set_summary(&t, d);
            }
        }
        if t != text {
            t = frontmatter::bump_updated(&t, &util::today());
        }
        t
    })
}

/// What the model is asked for one bookmark.
pub fn prompt(b: &Bookmark) -> String {
    let mut s = String::from("You label saved bookmarks for a personal knowledge base.\n\n");
    s.push_str(&format!("Title: {}\nURL: {}\n", b.title, b.url));
    if !b.folder.is_empty() {
        s.push_str(&format!("Browser folder: {}\n", b.folder));
    }
    if let Some(d) = &b.summary {
        s.push_str(&format!("Page description: {d}\n"));
    }
    s.push_str(
        "\nReply with exactly two lines and nothing else:\n\
         TAGS: up to 5 short lowercase topic tags, comma-separated\n\
         SUMMARY: one sentence of at most 25 words saying what the page is\n",
    );
    s
}

/// Read the model's reply: the two-line form asked for, or a JSON object with `tags` and
/// `summary` (some models answer in JSON whatever they're told).
pub fn parse_reply(reply: &str) -> Result<(Vec<String>, String)> {
    let trimmed = reply.trim();
    let json_start = trimmed.find('{');
    if let Some(v) = json_start
        .and_then(|s| trimmed.rfind('}').map(|e| &trimmed[s..=e]))
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
    {
        let tags = match v.get("tags") {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::String(s)) => s.split(',').map(String::from).collect(),
            _ => Vec::new(),
        };
        let summary = v.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        return finish(tags, summary);
    }
    let mut tags = Vec::new();
    let mut summary = String::new();
    for line in trimmed.lines() {
        let l = line.trim().trim_start_matches(['*', '-', '#', ' ']);
        let (k, v) = match l.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        match k.trim().trim_matches('*').to_ascii_lowercase().as_str() {
            "tags" => tags = v.split(',').map(String::from).collect(),
            "summary" => summary = v.trim().trim_matches('*').trim().to_string(),
            _ => {}
        }
    }
    finish(tags, &summary)
}

fn finish(tags: Vec<String>, summary: &str) -> Result<(Vec<String>, String)> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = store::tag(t.trim().trim_start_matches('#'));
        if !t.is_empty() && !out.contains(&t) && out.len() < MAX_NEW_TAGS {
            out.push(t);
        }
    }
    let summary = clean(summary.trim().trim_matches('"'))
        .ok_or_else(|| Error::Invalid("The model's reply had no summary line.".into()))?;
    Ok((out, summary))
}

fn apply_enrichment(text: &str, tags: &[String], summary: &str) -> String {
    let p = frontmatter::parse(text);
    let mut all = frontmatter::list(p.props.get("tags").map_or("", String::as_str));
    for t in tags {
        if !all.contains(t) {
            all.push(t.clone());
        }
    }
    let t = frontmatter::set(text, "tags", &frontmatter::format_list(&all), false);
    let t = frontmatter::set(&t, "enriched", &util::today(), false);
    store::set_summary(&t, summary)
}

/// Tag and summarise bookmarks with `model` (prompt in, reply out). With no `ids`, every
/// bookmark not enriched yet. Stops at `cancel`, and at the first sign the model server is
/// down rather than failing every item one by one.
pub fn enrich(
    vault: &Vault,
    ids: Option<&[String]>,
    model: &dyn Fn(&str) -> Result<String>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u32, u32, &str),
) -> Result<EnrichReport> {
    let mut report = EnrichReport::default();
    let targets: Vec<Stored> = match ids {
        Some(ids) => ids
            .iter()
            .map(|id| store::get(vault, id))
            .collect::<Result<_>>()?,
        None => {
            let all = store::load(vault);
            let total = all.len();
            let todo: Vec<Stored> = all
                .into_iter()
                .filter(|s| s.prop("enriched").is_empty())
                .collect();
            report.skipped = (total - todo.len()) as u32;
            todo
        }
    };
    let total = targets.len() as u32;
    for (i, s) in targets.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let b = s.view(&vault.id);
        match model(&prompt(&b)).and_then(|r| parse_reply(&r)) {
            Ok((tags, summary)) => {
                match store::update(vault, &b.id, |t| apply_enrichment(t, &tags, &summary)) {
                    Ok(_) => report.enriched += 1,
                    Err(e) => {
                        report.failed += 1;
                        report.errors.push(format!("{}: {e}", b.title));
                    }
                }
            }
            Err(e @ Error::Unavailable(_)) => {
                report.failed += 1;
                report.errors.push(e.to_string());
                break;
            }
            Err(e) => {
                report.failed += 1;
                report.errors.push(format!("{}: {e}", b.title));
            }
        }
        progress(i as u32 + 1, total, &b.title);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        (d, v)
    }

    #[test]
    fn reads_page_head() {
        let html = r#"<html><head><TITLE> Plain &amp; simple
            title </TITLE>
            <meta name="description" content="A page about things.">
            <meta property='og:title' content="The OG title">
            </head><body>…</body></html>"#;
        let m = parse_page(html);
        assert_eq!(m.title.as_deref(), Some("The OG title"));
        assert_eq!(m.description.as_deref(), Some("A page about things."));
        let m = parse_page("<title>Only &lt;this&gt;</title>");
        assert_eq!(m.title.as_deref(), Some("Only <this>"));
        assert_eq!(m.description, None);
        assert_eq!(parse_page("no markup"), PageMeta::default());
        let long = format!("<title>{}</title>", "word ".repeat(200));
        assert!(parse_page(&long).title.unwrap().chars().count() <= MAX_TEXT + 1);
    }

    #[test]
    fn refuses_other_schemes_without_a_request() {
        assert!(matches!(
            fetch_page("file:///etc/hosts"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            fetch_page("ftp://example.com/"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(fetch_page("not a url"), Err(Error::Invalid(_))));
    }

    #[test]
    fn fetches_from_a_local_server() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 2048];
            let _ = s.read(&mut req);
            let body = format!(
                "<title>Local page</title>{}",
                "x".repeat(2 * MAX_BYTES as usize)
            );
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        let m = fetch_page(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(m.title.as_deref(), Some("Local page"));
        let _ = server.join();
    }

    /// Goes to the internet, so it only runs when asked: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_over_https_with_the_system_roots() {
        let m = fetch_page("https://example.com/").unwrap();
        assert!(m.title.is_some());
    }

    #[test]
    fn page_meta_fills_only_blanks() {
        let (_d, v) = vault();
        let (b, _) = store::add(&v, "https://example.com/p", None).unwrap();
        let meta = PageMeta {
            title: Some("Real: title".into()),
            description: Some("What it is.".into()),
        };
        let b = apply_page(&v, &b.id, &meta).unwrap();
        assert_eq!(b.title, "Real: title");
        assert_eq!(b.summary.as_deref(), Some("What it is."));
        let again = PageMeta {
            title: Some("Other".into()),
            description: Some("Other.".into()),
        };
        let b = apply_page(&v, &b.id, &again).unwrap();
        assert_eq!(
            (b.title.as_str(), b.summary.as_deref()),
            ("Real: title", Some("What it is."))
        );
    }

    #[test]
    fn replies() {
        let (t, s) =
            parse_reply("TAGS: Rust, Systems Programming, #rust\nSUMMARY: The Rust language site.")
                .unwrap();
        assert_eq!(t, vec!["rust", "systems-programming"]);
        assert_eq!(s, "The Rust language site.");
        let (t, s) =
            parse_reply("Sure!\n```json\n{\"tags\": [\"a\", \"b\"], \"summary\": \"S.\"}\n```")
                .unwrap();
        assert_eq!(
            (t, s.as_str()),
            (vec!["a".to_string(), "b".to_string()], "S.")
        );
        let (_, s) = parse_reply("**Tags:** x\n**Summary:** Bold answer.").unwrap();
        assert_eq!(s, "Bold answer.");
        assert!(parse_reply("I can't help with that").is_err());
    }

    #[test]
    fn enrich_batch_cancel_and_model_down() {
        let (_d, v) = vault();
        let (a, _) = store::add(&v, "https://a.example/", Some("note")).unwrap();
        store::add(&v, "https://b.example/", None).unwrap();
        let calls = Cell::new(0);
        let model = |p: &str| -> Result<String> {
            calls.set(calls.get() + 1);
            assert!(p.contains("URL: https://"));
            Ok("TAGS: web\nSUMMARY: A site.".into())
        };
        let no = AtomicBool::new(false);
        let r = enrich(&v, None, &model, &no, &mut |_, _, _| {}).unwrap();
        assert_eq!((r.enriched, r.failed), (2, 0));
        let b = store::get(&v, &a.id).unwrap().view(&v.id);
        assert_eq!(b.tags, vec!["web"]);
        assert_eq!(b.summary.as_deref(), Some("A site."));
        assert!(b.has_notes, "the user's note survives");

        // already enriched: skipped
        let r = enrich(&v, None, &model, &no, &mut |_, _, _| {}).unwrap();
        assert_eq!((r.enriched, r.skipped), (0, 2));

        let yes = AtomicBool::new(true);
        let r = enrich(
            &v,
            Some(std::slice::from_ref(&a.id)),
            &model,
            &yes,
            &mut |_, _, _| {},
        )
        .unwrap();
        assert!(r.cancelled);

        let down =
            |_: &str| -> Result<String> { Err(Error::Unavailable("model server is down".into())) };
        let ids = vec![a.id.clone(), a.id.clone()];
        let r = enrich(&v, Some(&ids), &down, &no, &mut |_, _, _| {}).unwrap();
        assert_eq!(r.failed, 1, "stops at the first unavailable");
    }
}
