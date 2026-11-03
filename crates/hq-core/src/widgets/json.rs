//! The JSON status widget: GET a URL the user chose, pick fields out of the answer and
//! say whether each one is ok. This is one of the few things HQ sends off the Mac (SPEC 1),
//! so it is tight: http/https only, a timeout, a 256 KB cap, at most one fetch a minute
//! per URL however many widgets or windows ask.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::types::{JsonField, JsonStatusData, WidgetBody, WidgetData, WidgetKind, WidgetStatus};

pub const MIN_INTERVAL_SECS: u64 = 60;
pub const MAX_BYTES: u64 = 256 * 1024;
const MAX_TIMEOUT_SECS: u64 = 30;
const MAX_CACHED_URLS: usize = 64;
/// How a value is cut when it is shown (an object or a long string).
const MAX_SHOWN_CHARS: usize = 80;

/// `s` trimmed, if it is an http(s) URL with a host; empty stays empty (not set yet).
pub fn check_url(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    let lower = s.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .ok_or_else(|| Error::Invalid("The URL must start with http:// or https://.".into()))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) || s.contains(char::is_whitespace) {
        return Err(Error::Invalid(format!("\"{s}\" isn't a URL HQ can fetch.")));
    }
    Ok(s.to_string())
}

/// A field's "ok when" rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    Eq(String),
    Ne(String),
    Lt(f64),
    Le(f64),
    Gt(f64),
    Ge(f64),
    Truthy,
    Falsy,
}

/// `= up`, `!= down`, `< 500`, `>= 0.9`, `true`, `false`; blank = no rule.
pub fn parse_rule(s: &str) -> Result<Option<Rule>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    match s.to_ascii_lowercase().as_str() {
        "true" | "truthy" | "yes" => return Ok(Some(Rule::Truthy)),
        "false" | "falsy" | "no" => return Ok(Some(Rule::Falsy)),
        _ => {}
    }
    // longest operators first, so "<=" isn't read as "<" then "=…"
    for op in ["!=", "<=", ">=", "==", "=", "<", ">"] {
        if let Some(rest) = s.strip_prefix(op) {
            let rest = rest.trim().trim_matches('"').to_string();
            let n = || {
                rest.parse::<f64>()
                    .map_err(|_| Error::Invalid(format!("\"{s}\": {op} needs a number after it.")))
            };
            return Ok(Some(match op {
                "=" | "==" => Rule::Eq(rest),
                "!=" => Rule::Ne(rest),
                "<" => Rule::Lt(n()?),
                "<=" => Rule::Le(n()?),
                ">" => Rule::Gt(n()?),
                _ => Rule::Ge(n()?),
            }));
        }
    }
    Err(Error::Invalid(format!(
        "\"{s}\" isn't a rule; try = up, != down, < 500, >= 0.9, true or false."
    )))
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => {
            let s = s.trim().to_ascii_lowercase();
            !matches!(s.as_str(), "" | "0" | "false" | "no" | "off" | "down")
        }
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub fn check(rule: &Rule, v: &Value) -> bool {
    let eq = |want: &str| match (as_number(v), want.parse::<f64>()) {
        (Some(a), Ok(b)) if !matches!(v, Value::Bool(_)) => a == b,
        _ => display(v).eq_ignore_ascii_case(want),
    };
    let cmp = |f: fn(f64, f64) -> bool, b: f64| as_number(v).is_some_and(|a| f(a, b));
    match rule {
        Rule::Eq(w) => eq(w),
        Rule::Ne(w) => !eq(w),
        Rule::Lt(b) => cmp(|a, b| a < b, *b),
        Rule::Le(b) => cmp(|a, b| a <= b, *b),
        Rule::Gt(b) => cmp(|a, b| a > b, *b),
        Rule::Ge(b) => cmp(|a, b| a >= b, *b),
        Rule::Truthy => truthy(v),
        Rule::Falsy => !truthy(v),
    }
}

/// A value as one short line of text.
pub fn display(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            // 0.1 + 0.2 style noise stays out of a glanceable number
            Some(f) if n.is_f64() => {
                let r = (f * 1000.0).round() / 1000.0;
                r.to_string()
            }
            _ => n.to_string(),
        },
        other => other.to_string(),
    };
    if s.chars().count() > MAX_SHOWN_CHARS {
        let cut: String = s.chars().take(MAX_SHOWN_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        s
    }
}

/// A JSON pointer from what the user typed: `/a/0/b` as is, `a.0.b` converted, blank =
/// the whole document.
pub fn pointer(path: &str) -> String {
    let p = path.trim();
    if p.is_empty() || p.starts_with('/') {
        return p.to_string();
    }
    p.split('.')
        .map(|seg| format!("/{}", seg.replace('~', "~0").replace('/', "~1")))
        .collect()
}

/// One configured field, as stored in the widget's config.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldMap {
    pub label: String,
    pub path: String,
    pub ok: String,
    pub unit: String,
}

pub fn field_maps(cfg: &Map<String, Value>) -> Vec<FieldMap> {
    let get = |m: &Map<String, Value>, k: &str| {
        m.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    cfg.get("fields")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_object)
                .map(|m| FieldMap {
                    label: get(m, "label"),
                    path: get(m, "path"),
                    ok: get(m, "ok"),
                    unit: get(m, "unit"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The configured fields read out of `doc`. A field with a rule that doesn't parse, or
/// whose path finds nothing, is reported as failing rather than hidden.
pub fn map_fields(doc: &Value, maps: &[FieldMap]) -> Vec<JsonField> {
    maps.iter()
        .map(|m| {
            let found = doc.pointer(&pointer(&m.path));
            let label = if m.label.is_empty() {
                m.path.clone()
            } else {
                m.label.clone()
            };
            let ok = match parse_rule(&m.ok) {
                Ok(None) => None,
                Ok(Some(rule)) => Some(found.is_some_and(|v| check(&rule, v))),
                Err(_) => Some(false),
            };
            JsonField {
                label,
                value: found.map(display),
                unit: m.unit.clone(),
                ok,
            }
        })
        .collect()
}

/// Bad if the fetch failed or any rule fails; ok if every rule passes; unknown if there are
/// no rules to judge by.
pub fn overall(fields: &[JsonField], error: Option<&str>) -> WidgetStatus {
    if error.is_some() || fields.iter().any(|f| f.ok == Some(false)) {
        WidgetStatus::Bad
    } else if fields.iter().any(|f| f.ok == Some(true)) {
        WidgetStatus::Ok
    } else {
        WidgetStatus::Unknown
    }
}

// ---- fetching and the per-URL cache -----------------------------------------------------

/// GET `url` and parse the answer as JSON. Returns the HTTP status with the document;
/// a non-2xx answer is an error naming the status.
pub fn fetch(url: &str, timeout: Duration) -> Result<(u16, Value)> {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(3)
        .user_agent("vault-hq")
        .tls_config(tls)
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| Error::Unavailable(format!("Couldn't reach {url}: {e}")))?;
    let status = resp.status().as_u16();
    let mut bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Unavailable(format!("The answer from {url} broke off: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(Error::Unavailable(format!("The server answered {status}.")));
    }
    parse_body(&bytes).map(|doc| (status, doc))
}

/// The response body as JSON, refusing anything over the cap.
pub fn parse_body(bytes: &[u8]) -> Result<Value> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Invalid(format!(
            "The answer is over {} KB; point the widget at something smaller.",
            MAX_BYTES / 1024
        )));
    }
    serde_json::from_slice(bytes)
        .map_err(|e| Error::Invalid(format!("The answer isn't JSON ({e}).")))
}

#[derive(Debug, Clone)]
struct Entry {
    fetched_at: Option<i64>,
    next_at: i64,
    last_used: i64,
    status: Option<u16>,
    doc: Option<Value>,
    error: Option<String>,
}

/// Last answer per URL, so polling faster than the interval (or from two widgets) never
/// reaches the network. Field mapping is re-run on every ask, so editing a widget's fields
/// shows at once without a fetch.
#[derive(Default)]
pub struct JsonCache {
    entries: Mutex<HashMap<String, Entry>>,
}

fn interval_ms(cfg: &Map<String, Value>) -> i64 {
    let secs = cfg
        .get("intervalSecs")
        .and_then(Value::as_u64)
        .unwrap_or(300)
        .max(MIN_INTERVAL_SECS);
    secs as i64 * 1000
}

fn timeout(cfg: &Map<String, Value>) -> Duration {
    let secs = cfg
        .get("timeoutSecs")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, MAX_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// The widget's data, fetching through `fetcher` only when this URL's copy is due.
/// The cache lock is not held during the fetch; two simultaneous first asks may both
/// fetch, which is harmless and keeps a slow server from blocking other widgets.
pub fn widget<F>(cache: &JsonCache, cfg: &Map<String, Value>, now: i64, fetcher: F) -> WidgetData
where
    F: Fn(&str, Duration) -> Result<(u16, Value)>,
{
    let url = cfg
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        let message = "Add a URL in this widget's settings.".to_string();
        return WidgetData {
            kind: WidgetKind::JsonStatus,
            status: WidgetStatus::Unknown,
            summary: message.clone(),
            at: now,
            body: WidgetBody::Unavailable { message },
        };
    }

    let cached = {
        let map = cache.entries.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&url).cloned()
    };
    let entry = match cached {
        Some(e) if now < e.next_at => e,
        previous => {
            let result = check_url(&url).and_then(|u| fetcher(&u, timeout(cfg)));
            let mut e = previous.unwrap_or(Entry {
                fetched_at: None,
                next_at: 0,
                last_used: now,
                status: None,
                doc: None,
                error: None,
            });
            e.next_at = now + interval_ms(cfg);
            match result {
                Ok((status, doc)) => {
                    e.fetched_at = Some(now);
                    e.status = Some(status);
                    e.doc = Some(doc);
                    e.error = None;
                }
                Err(err) => e.error = Some(err.to_string()),
            }
            e
        }
    };

    {
        let mut map = cache.entries.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = entry.clone();
        e.last_used = now;
        map.insert(url.clone(), e);
        if map.len() > MAX_CACHED_URLS {
            // forget the URL nobody has asked about for longest (an edited-away widget)
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
    }

    let fields = entry
        .doc
        .as_ref()
        .map(|doc| map_fields(doc, &field_maps(cfg)))
        .unwrap_or_default();
    let status = overall(&fields, entry.error.as_deref());
    let summary = match (&entry.error, fields.len()) {
        (Some(err), _) => err.clone(),
        (None, 0) => "fetched; add fields to show".to_string(),
        (None, n) => {
            let passing = fields.iter().filter(|f| f.ok == Some(true)).count();
            let judged = fields.iter().filter(|f| f.ok.is_some()).count();
            if judged == 0 {
                format!("{n} fields")
            } else {
                format!("{passing}/{judged} ok")
            }
        }
    };
    WidgetData {
        kind: WidgetKind::JsonStatus,
        status,
        summary,
        at: now,
        body: WidgetBody::JsonStatus(JsonStatusData {
            url,
            http_status: entry.status,
            fields,
            error: entry.error,
            fetched_at: entry.fetched_at,
            next_at: entry.next_at,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    #[test]
    fn urls() {
        assert_eq!(
            check_url("  https://a.example/x ").unwrap(),
            "https://a.example/x"
        );
        assert_eq!(
            check_url("HTTP://a.local:8080").unwrap(),
            "HTTP://a.local:8080"
        );
        assert_eq!(check_url("").unwrap(), "");
        for bad in [
            "ftp://a",
            "file:///etc/hosts",
            "http://",
            "https:///x",
            "a.example",
            "http://a b",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rules_parse() {
        assert_eq!(parse_rule("").unwrap(), None);
        assert_eq!(parse_rule("= up").unwrap(), Some(Rule::Eq("up".into())));
        assert_eq!(parse_rule("==\"ok\"").unwrap(), Some(Rule::Eq("ok".into())));
        assert_eq!(
            parse_rule("!= down").unwrap(),
            Some(Rule::Ne("down".into()))
        );
        assert_eq!(parse_rule("<= 5").unwrap(), Some(Rule::Le(5.0)));
        assert_eq!(parse_rule(">=0.9").unwrap(), Some(Rule::Ge(0.9)));
        assert_eq!(parse_rule("<500").unwrap(), Some(Rule::Lt(500.0)));
        assert_eq!(parse_rule("TRUE").unwrap(), Some(Rule::Truthy));
        assert!(parse_rule("< fast").is_err());
        assert!(parse_rule("up").is_err());
    }

    #[test]
    fn status_thresholds() {
        let r = |s: &str| parse_rule(s).unwrap().unwrap();
        assert!(check(&r("< 500"), &json!(120)));
        assert!(!check(&r("< 500"), &json!(500)));
        assert!(check(&r("<= 500"), &json!("500")));
        assert!(check(&r(">= 0.9"), &json!(0.95)));
        assert!(
            !check(&r("> 1"), &json!("n/a")),
            "a non-number fails a comparison"
        );
        assert!(check(&r("= up"), &json!("UP")));
        assert!(check(&r("= 1"), &json!(1.0)));
        assert!(check(&r("!= down"), &json!("degraded")));
        assert!(check(&r("= true"), &json!(true)));
        assert!(check(&r("true"), &json!("yes")));
        assert!(!check(&r("true"), &json!("down")));
        assert!(check(&r("false"), &json!(0)));
        assert!(!check(&r("true"), &Value::Null));
    }

    #[test]
    fn pointers() {
        assert_eq!(pointer("/a/0"), "/a/0");
        assert_eq!(pointer("a.0.b"), "/a/0/b");
        assert_eq!(pointer("a/b.c"), "/a~1b/c");
        assert_eq!(pointer(" "), "");
    }

    #[test]
    fn maps_fields() {
        let doc = json!({
            "status": "up",
            "latency": {"p95": 812.3456},
            "checks": [{"name": "db", "ok": true}],
            "meta": {"long": "x".repeat(200)}
        });
        let maps = vec![
            FieldMap {
                label: "status".into(),
                path: "status".into(),
                ok: "= up".into(),
                unit: "".into(),
            },
            FieldMap {
                label: "p95".into(),
                path: "/latency/p95".into(),
                ok: "< 500".into(),
                unit: "ms".into(),
            },
            FieldMap {
                label: "".into(),
                path: "checks.0.ok".into(),
                ok: "true".into(),
                unit: "".into(),
            },
            FieldMap {
                label: "gone".into(),
                path: "/nope".into(),
                ok: "".into(),
                unit: "".into(),
            },
            FieldMap {
                label: "missing+rule".into(),
                path: "/nope".into(),
                ok: "= 1".into(),
                unit: "".into(),
            },
            FieldMap {
                label: "bad rule".into(),
                path: "status".into(),
                ok: "?".into(),
                unit: "".into(),
            },
            FieldMap {
                label: "long".into(),
                path: "meta.long".into(),
                ok: "".into(),
                unit: "".into(),
            },
        ];
        let f = map_fields(&doc, &maps);
        assert_eq!(f[0].value.as_deref(), Some("up"));
        assert_eq!(f[0].ok, Some(true));
        assert_eq!(f[1].value.as_deref(), Some("812.346"));
        assert_eq!((f[1].ok, f[1].unit.as_str()), (Some(false), "ms"));
        assert_eq!(f[2].label, "checks.0.ok");
        assert_eq!(f[2].ok, Some(true));
        assert_eq!((f[3].value.clone(), f[3].ok), (None, None));
        assert_eq!(f[4].ok, Some(false));
        assert_eq!(f[5].ok, Some(false));
        assert_eq!(
            f[6].value.as_ref().unwrap().chars().count(),
            MAX_SHOWN_CHARS
        );
        assert_eq!(overall(&f, None), WidgetStatus::Bad);
        assert_eq!(overall(&f[..1], None), WidgetStatus::Ok);
        assert_eq!(overall(&f[3..4], None), WidgetStatus::Unknown);
        assert_eq!(overall(&f[..1], Some("down")), WidgetStatus::Bad);
    }

    #[test]
    fn body_cap_and_parse() {
        assert!(parse_body(br#"{"a":1}"#).is_ok());
        assert!(parse_body(b"<html>").is_err());
        let big = vec![b' '; MAX_BYTES as usize + 1];
        let err = parse_body(&big).unwrap_err().to_string();
        assert!(err.contains("256 KB"), "{err}");
    }

    fn cfg(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn polls_at_most_once_per_interval_and_keeps_the_last_good_copy() {
        let cache = JsonCache::default();
        let calls = Cell::new(0);
        let up = |_: &str, _: Duration| {
            calls.set(calls.get() + 1);
            Ok((200, json!({"status": "up"})))
        };
        let c = cfg(json!({
            "url": "http://status.example/x",
            "intervalSecs": 10,
            "fields": [{"label": "s", "path": "status", "ok": "= up"}]
        }));
        let d = widget(&cache, &c, 0, up);
        assert_eq!(d.status, WidgetStatus::Ok);
        assert_eq!(d.summary, "1/1 ok");
        // asked again 30 s later: the interval is raised to the 60 s floor, so no fetch
        widget(&cache, &c, 30_000, up);
        assert_eq!(calls.get(), 1);
        widget(&cache, &c, 60_000, up);
        assert_eq!(calls.get(), 2);

        // the server goes away: the widget says so but still shows the last fields
        let down = |_: &str, _: Duration| Err(Error::Unavailable("Couldn't reach it.".into()));
        let d = widget(&cache, &c, 120_000, down);
        assert_eq!(d.status, WidgetStatus::Bad);
        let WidgetBody::JsonStatus(j) = d.body else {
            panic!("json body")
        };
        assert_eq!(j.error.as_deref(), Some("Couldn't reach it."));
        assert_eq!(j.fields[0].value.as_deref(), Some("up"));
        assert_eq!(j.fetched_at, Some(60_000));
        assert_eq!(j.next_at, 180_000);
    }

    #[test]
    fn no_url_asks_for_one() {
        let cache = JsonCache::default();
        let d = widget(&cache, &Map::new(), 0, |_: &str, _: Duration| {
            panic!("must not fetch")
        });
        assert!(matches!(d.body, WidgetBody::Unavailable { .. }));
    }
}
