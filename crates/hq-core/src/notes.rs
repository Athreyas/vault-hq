//! Notes (SPEC 3, 5): list, read with links and backlinks, source, guarded save, create
//! from a template or blank, move/rename with link updates, archive.
//!
//! Link resolution never leaves the vault: a reference is only ever matched against the
//! notes the scan found, and every path a write builds is checked to stay under the
//! vault's resolved root (no `..`, no hidden folders, no symlink out).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::locks;
use crate::types::{
    note_id, split_note_id, LinkView, MoveResult, NoteFilter, NoteId, NoteInput, NoteSummary,
    NoteView, SaveResult, Source, TemplateInfo,
};
use crate::util;
use crate::vault::registry::Snapshot;
use crate::vault::{layout, Note, Scan, Vault};

// ---- summaries --------------------------------------------------------------------------

pub fn folder_of(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(f, _)| f).unwrap_or("")
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn stem_of(rel: &str) -> &str {
    let name = file_name(rel);
    name.strip_suffix(".md").unwrap_or(name)
}

/// `updated:`, or the file's modification date when a note has none.
pub fn updated(note: &Note) -> String {
    if let Some(u) = note.props.get("updated").filter(|u| !u.is_empty()) {
        return u.clone();
    }
    chrono::DateTime::from_timestamp(note.mtime, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn summary(vault: &Vault, note: &Note) -> NoteSummary {
    NoteSummary {
        id: note_id(&vault.id, &note.rel),
        vault: vault.id.clone(),
        vault_name: vault.name.clone(),
        rel: note.rel.clone(),
        title: note.title().to_string(),
        folder: folder_of(&note.rel).to_string(),
        note_type: note.props.get("type").filter(|t| !t.is_empty()).cloned(),
        updated: updated(note),
        tags: note
            .props
            .get("tags")
            .map(|t| frontmatter::list(t))
            .unwrap_or_default(),
    }
}

/// Notes the vault keeps for its agents rather than for reading: their rules
/// (`AGENTS.md`, `CLAUDE.md` at any depth, `07-Agents/`) and their memory (`06-Memory/`:
/// sessions, facts, saved Ask conversations).
pub fn is_system(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.eq_ignore_ascii_case("AGENTS.md")
        || name.eq_ignore_ascii_case("CLAUDE.md")
        || rel.starts_with("06-Memory/")
        || rel.starts_with("07-Agents/")
}

/// An agent's raw session dump waiting in the inbox to be curated into memory
/// (tagged `session-capture` or `compact` by the session hooks).
fn is_session_capture(props: &std::collections::BTreeMap<String, String>) -> bool {
    props.get("tags").is_some_and(|t| {
        frontmatter::list(t)
            .iter()
            .any(|x| x == "session-capture" || x == "compact")
    })
}

pub fn list(snaps: &[Snapshot], filter: &NoteFilter) -> Vec<NoteSummary> {
    let text = filter
        .text
        .as_deref()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty());
    let folder = filter
        .folder
        .as_deref()
        .map(|f| f.trim_matches('/'))
        .filter(|f| !f.is_empty());
    let mut out: Vec<NoteSummary> = Vec::new();
    for s in snaps {
        if filter.vault.as_deref().is_some_and(|v| v != s.vault.id) {
            continue;
        }
        for n in &s.scan.notes {
            if filter.hide_system == Some(true)
                && (is_system(&n.rel) || is_session_capture(&n.props))
            {
                continue;
            }
            if folder.is_some_and(|f| !(folder_of(&n.rel) == f || layout::is_in(&n.rel, f))) {
                continue;
            }
            if let Some(t) = &filter.note_type {
                if n.props.get("type") != Some(t) {
                    continue;
                }
            }
            let sum = summary(&s.vault, n);
            if let Some(since) = &filter.updated_since {
                if sum.updated.as_str() < since.as_str() {
                    continue;
                }
            }
            if let Some(t) = &text {
                if !(sum.title.to_lowercase().contains(t)
                    || n.rel.to_lowercase().contains(t)
                    || n.body.to_lowercase().contains(t))
                {
                    continue;
                }
            }
            out.push(sum);
        }
    }
    out.sort_by(|a, b| {
        a.folder
            .cmp(&b.folder)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then_with(|| a.vault_name.cmp(&b.vault_name))
    });
    if let Some(limit) = filter.limit {
        out.truncate(limit as usize);
    }
    out
}

// ---- links --------------------------------------------------------------------------------

/// A link found in a note's text. `range` is the byte range of the target path as written
/// (without `#heading`, `|alias` or a markdown link's title), which is what a move rewrites.
#[derive(Debug, Clone, PartialEq)]
pub struct RawLink {
    pub target: String,
    pub range: Range<usize>,
    pub embed: bool,
    pub wiki: bool,
}

/// Every `[[wiki]]`, `![[embed]]` and relative markdown link to a note, outside code fences.
pub fn links(text: &str) -> Vec<RawLink> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut fence = false;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        wiki_links(line, start, &mut out);
        markdown_links(line, start, &mut out);
    }
    out
}

fn wiki_links(line: &str, base: usize, out: &mut Vec<RawLink>) {
    let mut from = 0;
    while let Some(i) = line[from..].find("[[") {
        let open = from + i;
        let inner_start = open + 2;
        let Some(j) = line[inner_start..].find("]]") else {
            break;
        };
        let inner = &line[inner_start..inner_start + j];
        from = inner_start + j + 2;
        if inner.contains("[[") {
            from = inner_start;
            continue;
        }
        let path_part = inner.split(['|', '#']).next().unwrap_or("");
        let lead = path_part.len() - path_part.trim_start().len();
        let target = path_part.trim();
        if target.is_empty() {
            continue; // [[#heading]] points into the same note
        }
        let s = base + inner_start + lead;
        out.push(RawLink {
            target: target.to_string(),
            range: s..s + target.len(),
            embed: open > 0 && line.as_bytes()[open - 1] == b'!',
            wiki: true,
        });
    }
}

fn markdown_links(line: &str, base: usize, out: &mut Vec<RawLink>) {
    let mut from = 0;
    while let Some(i) = line[from..].find("](") {
        let close = from + i;
        let url_start = close + 2;
        from = url_start;
        let Some(open) = line[..close].rfind('[') else {
            continue;
        };
        if open > 0 && line.as_bytes()[open - 1] == b'[' {
            continue; // part of a wiki link
        }
        let rest = &line[url_start..];
        let (raw_start, raw) = if let Some(r) = rest.strip_prefix('<') {
            match r.find('>') {
                Some(k) => (url_start + 1, &r[..k]),
                None => continue,
            }
        } else {
            let end = rest
                .find(|c: char| c == ')' || c.is_whitespace())
                .unwrap_or(rest.len());
            (url_start, &rest[..end])
        };
        let path = raw.split('#').next().unwrap_or("");
        if path.is_empty() || path.contains("://") || path.starts_with("mailto:") {
            continue;
        }
        let last = file_name(path);
        if !(path.ends_with(".md") || !last.contains('.')) {
            continue; // an image or attachment, not a note
        }
        let s = base + raw_start;
        out.push(RawLink {
            target: percent_decode(path),
            range: s..s + path.len(),
            embed: open > 0 && line.as_bytes()[open - 1] == b'!',
            wiki: false,
        });
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Resolves link targets among one vault's notes (SPEC 3): exact vault-relative path,
/// path + `.md`, case-insensitive path, path suffix, stem. Ties go to the shortest path.
pub struct Resolver<'a> {
    notes: &'a [Note],
    by_rel: HashMap<&'a str, usize>,
    by_lower: HashMap<String, usize>,
    by_stem: HashMap<String, Vec<usize>>,
}

impl<'a> Resolver<'a> {
    pub fn new(scan: &'a Scan) -> Self {
        let notes = scan.notes.as_slice();
        let mut by_rel = HashMap::new();
        let mut by_lower = HashMap::new();
        let mut by_stem: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in notes.iter().enumerate() {
            by_rel.insert(n.rel.as_str(), i);
            by_lower.entry(n.rel.to_lowercase()).or_insert(i);
            by_stem.entry(n.stem.to_lowercase()).or_default().push(i);
        }
        Resolver {
            notes,
            by_rel,
            by_lower,
            by_stem,
        }
    }

    fn shortest(&self, idx: impl Iterator<Item = usize>) -> Option<&'a Note> {
        idx.map(|i| &self.notes[i])
            .min_by(|a, b| a.rel.len().cmp(&b.rel.len()).then(a.rel.cmp(&b.rel)))
    }

    /// A target written as in a wiki link (vault-relative or a bare stem).
    pub fn resolve(&self, target: &str) -> Option<&'a Note> {
        let clean = normalize(target.trim().trim_start_matches('/'), "")?;
        if clean.is_empty() {
            return None;
        }
        if let Some(&i) = self.by_rel.get(clean.as_str()) {
            return Some(&self.notes[i]);
        }
        let with_md = if clean.ends_with(".md") {
            clean.clone()
        } else {
            format!("{clean}.md")
        };
        if let Some(&i) = self.by_rel.get(with_md.as_str()) {
            return Some(&self.notes[i]);
        }
        let lower = with_md.to_lowercase();
        if let Some(&i) = self.by_lower.get(&lower) {
            return Some(&self.notes[i]);
        }
        let suffix = format!("/{lower}");
        if let Some(n) = self.shortest(
            self.notes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.rel.to_lowercase().ends_with(&suffix))
                .map(|(i, _)| i),
        ) {
            return Some(n);
        }
        let stem = stem_of(&lower).to_string();
        self.by_stem
            .get(&stem)
            .and_then(|v| self.shortest(v.iter().copied()))
    }

    /// A link as it appears in the note at `from_rel`: markdown links are relative to that
    /// note's folder (or to the vault root when they start with `/`).
    pub fn resolve_link(&self, from_rel: &str, link: &RawLink) -> Option<&'a Note> {
        if link.wiki || link.target.starts_with('/') {
            return self.resolve(&link.target);
        }
        let joined = normalize(&link.target, folder_of(from_rel))?;
        self.resolve(&joined)
    }

    /// True when `stem` names exactly one note other than `except`.
    fn stem_taken(&self, stem: &str, except: &str) -> bool {
        self.by_stem
            .get(&stem.to_lowercase())
            .is_some_and(|v| v.iter().any(|&i| self.notes[i].rel != except))
    }
}

/// Join `path` onto `dir` and fold `.`/`..`; None when it would climb out of the vault.
fn normalize(path: &str, dir: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

/// The path from folder `from_dir` to vault-relative `to`, as a markdown link writes it.
fn relative_path(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|p| !p.is_empty()).collect();
    let target: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&target[common..]);
    parts.join("/")
}

// ---- finding and reading --------------------------------------------------------------------

/// Find a note by id (`<vault>:<rel>`), or by a reference resolved like a wiki link in
/// `primary` first and then the other vaults. Returns the note with its snapshot.
pub fn find<'a>(
    snaps: &'a [Snapshot],
    primary: Option<&str>,
    reference: &str,
) -> Result<(&'a Snapshot, &'a Note)> {
    let reference = reference.trim();
    let reference = reference
        .strip_prefix("[[")
        .and_then(|r| r.strip_suffix("]]"))
        .map(|r| r.split(['|', '#']).next().unwrap_or(r))
        .unwrap_or(reference);
    if let Some((vid, rel)) = split_note_id(reference) {
        if let Some(s) = snaps.iter().find(|s| s.vault.id == vid) {
            // an id names one exact file; no fuzzy matching
            return s
                .scan
                .notes
                .iter()
                .find(|n| n.rel == rel)
                .map(|n| (s, n))
                .ok_or_else(|| Error::NotFound(format!("No note {rel} in {}.", s.vault.name)));
        }
    }
    let mut order: Vec<&Snapshot> = snaps.iter().collect();
    order.sort_by_key(|s| Some(s.vault.id.as_str()) != primary);
    for s in order {
        if let Some(n) = Resolver::new(&s.scan).resolve(reference) {
            return Ok((s, n));
        }
    }
    Err(Error::NotFound(format!("No note matches \"{reference}\".")))
}

/// Frontmatter lines in file order (the scan keeps them sorted by key).
fn ordered_props(text: &str) -> Vec<(String, String)> {
    let p = frontmatter::parse(text);
    if p.len == 0 {
        return Vec::new();
    }
    text[..p.len]
        .lines()
        .filter_map(|l| {
            let (k, _) = l.split_once(':')?;
            let k = k.trim();
            p.props.get(k).map(|v| (k.to_string(), v.clone()))
        })
        .collect()
}

pub fn get(snaps: &[Snapshot], primary: Option<&str>, reference: &str) -> Result<NoteView> {
    let (snap, note) = find(snaps, primary, reference)?;
    let path = note_path(&snap.vault, &note.rel)?;
    // read fresh: the scan may be a moment old, the reader should not be
    let (text, hash) = locks::read(&path)?;
    let parsed = frontmatter::parse(&text);
    let resolver = Resolver::new(&snap.scan);
    let title = parsed
        .props
        .get("title")
        .filter(|t| !t.is_empty())
        .cloned()
        .unwrap_or_else(|| note.stem.clone());
    let mut sum = summary(&snap.vault, note);
    sum.title = title.clone();
    let links = links(&text)
        .into_iter()
        .map(|l| LinkView {
            resolved: resolver
                .resolve_link(&note.rel, &l)
                .map(|n| note_id(&snap.vault.id, &n.rel)),
            target: l.target,
            embed: l.embed,
        })
        .collect();
    let mut backlinks: Vec<NoteSummary> = snap
        .scan
        .notes
        .iter()
        .filter(|o| o.rel != note.rel)
        .filter(|o| {
            links_in(o)
                .iter()
                .any(|l| resolver.resolve_link(&o.rel, l).map(|n| &n.rel) == Some(&note.rel))
        })
        .map(|o| summary(&snap.vault, o))
        .collect();
    backlinks.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.rel.cmp(&b.rel)));
    let body = strip_lead_heading(parsed.body, &title);
    Ok(NoteView {
        note: sum,
        props: ordered_props(&text)
            .into_iter()
            .filter(|(k, _)| k != "title")
            .collect(),
        body: body.to_string(),
        hash,
        links,
        backlinks,
    })
}

/// Links in a scanned note: the body plus frontmatter values (Obsidian links properties).
fn links_in(note: &Note) -> Vec<RawLink> {
    let mut out = links(&note.body);
    for v in note.props.values() {
        if v.contains("[[") {
            out.extend(links(v));
        }
    }
    out
}

/// Most notes open with `# <title>`, which the reader already shows as its heading.
fn strip_lead_heading<'t>(body: &'t str, title: &str) -> &'t str {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        if line.trim().eq_ignore_ascii_case(title.trim()) {
            return after;
        }
    }
    body
}

/// A plain-text taste of a note for Needs and cards: no title line, no task syntax.
pub fn excerpt(body: &str, max: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut skipped_title = false;
    for line in body.lines() {
        let t = line.trim_end();
        if !skipped_title && lines.is_empty() {
            if t.trim().is_empty() {
                continue;
            }
            if t.starts_with("# ") {
                skipped_title = true;
                continue;
            }
        }
        let indent = &t[..t.len() - t.trim_start().len()];
        let s = t.trim_start();
        let s = if let Some(h) = s.strip_prefix('#') {
            h.trim_start_matches('#').trim_start().to_string()
        } else if let Some(r) = s.strip_prefix("- [ ] ") {
            format!("{indent}○ {r}")
        } else if let Some(r) = s
            .strip_prefix("- [x] ")
            .or_else(|| s.strip_prefix("- [X] "))
        {
            format!("{indent}✓ {r}")
        } else if let Some(r) = s.strip_prefix("- [-] ") {
            format!("{indent}✕ {r}")
        } else {
            format!("{indent}{s}")
        };
        lines.push(clean_task_fields(&s).replace("**", ""));
    }
    let text = lines.join("\n").trim().to_string();
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(i) if i > max / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end())
}

fn clean_task_fields(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut tokens = line.split(' ').peekable();
    while let Some(tok) = tokens.next() {
        match tok {
            "🆔" | "➕" => {
                tokens.next();
            }
            "🔺" | "⏫" | "🔼" | "🔽" | "⏬" => {}
            "📅" => {
                if let Some(d) = tokens.next() {
                    out.push(format!("(due {d})"));
                }
            }
            "✅" => {
                if let Some(d) = tokens.next() {
                    out.push(format!("(done {d})"));
                }
            }
            t => out.push(t.to_string()),
        }
    }
    out.join(" ")
}

// ---- paths a write may touch ------------------------------------------------------------------

fn check_rel_shape(rel: &str) -> Result<()> {
    if rel.split('/').any(|c| c.starts_with('.')) {
        return Err(Error::Denied(format!(
            "{rel} is inside a hidden folder; HQ doesn't write there."
        )));
    }
    Ok(())
}

/// The nearest existing ancestor of `path` resolves to somewhere under the vault root.
fn ensure_inside(vault: &Vault, path: &Path) -> Result<()> {
    let root = std::fs::canonicalize(&vault.root)?;
    let mut cur = Some(path);
    while let Some(p) = cur {
        if p.exists() {
            let real = std::fs::canonicalize(p)?;
            if real.starts_with(&root) {
                return Ok(());
            }
            break;
        }
        cur = p.parent();
    }
    Err(Error::Denied(format!(
        "{} is outside the vault.",
        path.display()
    )))
}

/// The path of an existing note, refusing anything that is not a plain `.md` file in the vault.
pub fn note_path(vault: &Vault, rel: &str) -> Result<PathBuf> {
    if !rel.ends_with(".md") {
        return Err(Error::Invalid(format!("{rel} is not a markdown note.")));
    }
    let path = vault.resolve_rel(rel)?;
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Err(Error::Denied(format!(
                "{rel} is not a regular file; HQ only edits plain notes."
            )))
        }
        Err(_) => return Err(Error::NotFound(format!("No note at {rel}."))),
    }
    ensure_inside(vault, &path)?;
    Ok(path)
}

/// A path for a new note: inside the vault, not hidden.
fn new_note_path(vault: &Vault, rel: &str) -> Result<PathBuf> {
    check_rel_shape(rel)?;
    let path = vault.resolve_rel(rel)?;
    ensure_inside(vault, &path)?;
    Ok(path)
}

// ---- source and save ----------------------------------------------------------------------------

pub fn source(vault: &Vault, rel: &str) -> Result<Source> {
    let (text, hash) = locks::read(&note_path(vault, rel)?)?;
    Ok(Source {
        id: note_id(&vault.id, rel),
        text,
        hash,
    })
}

/// Replace a note's text if it still hashes to `base`; otherwise nothing is written and
/// the `Conflict` error carries what is on disk to compare with. `updated:` is bumped when
/// the text changed.
pub fn save(vault: &Vault, rel: &str, text: &str, base: &str, today: &str) -> Result<SaveResult> {
    let path = note_path(vault, rel)?;
    let mut written = String::new();
    let hash = locks::guarded_edit(&path, Some(base), |cur| {
        written = if text == cur {
            cur.to_string()
        } else {
            frontmatter::bump_updated(text, today)
        };
        Ok(written.clone())
    })?;
    Ok(SaveResult {
        id: note_id(&vault.id, rel),
        text: written,
        hash,
    })
}

// ---- create -----------------------------------------------------------------------------------

/// A frontmatter value on one line, quoted when a YAML reader would misread it.
fn fm_value(v: &str) -> String {
    let v = v.replace(['\n', '\r'], " ").trim().to_string();
    let risky = v.contains(": ")
        || v.starts_with([
            '[', '{', '"', '\'', '#', '&', '*', '!', '|', '>', '%', '@', '`',
        ]);
    if risky && !v.contains('"') {
        format!("\"{v}\"")
    } else {
        v
    }
}

fn clean_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|t| {
            t.trim()
                .trim_start_matches('#')
                .chars()
                .filter(|c| !matches!(c, ',' | '[' | ']' | '\n' | '\r' | '"'))
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

pub fn templates(snaps: &[Snapshot]) -> Vec<TemplateInfo> {
    let mut out = Vec::new();
    for s in snaps {
        let dir = s.vault.root.join(layout::TEMPLATES);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<TemplateInfo> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let stem = name.strip_suffix(".md")?.to_string();
                (!name.starts_with('.') && e.file_type().ok()?.is_file()).then(|| TemplateInfo {
                    vault: s.vault.id.clone(),
                    vault_name: s.vault.name.clone(),
                    rel: format!("{}/{name}", layout::TEMPLATES),
                    name: stem,
                })
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(found);
    }
    out
}

fn template_text(vault: &Vault, template: &str) -> Result<String> {
    let rel = if template.contains('/') {
        template.to_string()
    } else {
        let name = template.strip_suffix(".md").unwrap_or(template);
        format!("{}/{name}.md", layout::TEMPLATES)
    };
    if !layout::is_in(&rel, layout::TEMPLATES) {
        return Err(Error::Invalid(format!(
            "Templates live in {}/; {rel} is not one.",
            layout::TEMPLATES
        )));
    }
    let path = note_path(vault, &rel)
        .map_err(|_| Error::NotFound(format!("No template {template} in {}.", vault.name)))?;
    Ok(std::fs::read_to_string(path)?)
}

/// The text of a new note from `input`, before it has a file name.
pub fn render_new(vault: &Vault, input: &NoteInput, today: &str) -> Result<String> {
    let title = input.title.replace(['\n', '\r'], " ").trim().to_string();
    if title.is_empty() {
        return Err(Error::Invalid("A note needs a title.".into()));
    }
    let author = input
        .author
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .unwrap_or("human");
    let tags = clean_tags(input.tags.as_deref().unwrap_or_default());
    let body = input.body.as_deref().unwrap_or("").trim_end();

    if let Some(t) = input.template.as_deref().filter(|t| !t.trim().is_empty()) {
        let raw = template_text(vault, t.trim())?
            .replace("{{title}}", &title)
            .replace("{{date}}", today);
        let mut text = if frontmatter::parse(&raw).len == 0 {
            let kind = input.note_type.as_deref().unwrap_or("note");
            format!(
                "{}{}",
                frontmatter::render(&[
                    ("title", fm_value(&title)),
                    ("type", fm_value(kind)),
                    ("created", today.into()),
                    ("updated", today.into()),
                    ("author", fm_value(author)),
                ]),
                raw.trim_start()
            )
        } else {
            raw
        };
        let props = frontmatter::parse(&text).props;
        let missing = |k: &str| props.get(k).is_none_or(|v| v.is_empty());
        if missing("title") {
            text = frontmatter::set(&text, "title", &fm_value(&title), false);
        }
        if let Some(kind) = &input.note_type {
            text = frontmatter::set(&text, "type", &fm_value(kind), false);
        } else if missing("type") {
            text = frontmatter::set(&text, "type", "note", false);
        }
        text = frontmatter::set(&text, "created", today, false);
        text = frontmatter::set(&text, "updated", today, false);
        if missing("author") {
            text = frontmatter::set(&text, "author", &fm_value(author), false);
        }
        if !tags.is_empty() {
            let mut all = props
                .get("tags")
                .map(|t| frontmatter::list(t))
                .unwrap_or_default();
            for t in tags {
                if !all.contains(&t) {
                    all.push(t);
                }
            }
            text = frontmatter::set(&text, "tags", &frontmatter::format_list(&all), false);
        }
        if !body.is_empty() {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push('\n');
            text.push_str(body);
            text.push('\n');
        }
        return Ok(text);
    }

    let kind = input.note_type.as_deref().unwrap_or("note");
    let mut props: Vec<(&str, String)> = vec![
        ("title", fm_value(&title)),
        ("type", fm_value(kind)),
        ("created", today.into()),
        ("updated", today.into()),
        ("author", fm_value(author)),
    ];
    if !tags.is_empty() {
        props.push(("tags", frontmatter::format_list(&tags)));
    }
    if kind == "project" {
        props.push(("status", "active".into()));
    }
    let mut text = frontmatter::render(&props);
    text.push_str(&format!("# {title}\n"));
    if !body.is_empty() {
        text.push('\n');
        text.push_str(body);
        text.push('\n');
    }
    Ok(text)
}

/// Create a note; returns its vault-relative path. The file name is a slug of the title,
/// with `-2`, `-3`… when taken: an existing file is never overwritten.
pub fn create(vault: &Vault, input: &NoteInput, today: &str) -> Result<String> {
    let text = render_new(vault, input, today)?;
    let kind = input.note_type.as_deref().unwrap_or("note");
    let folder = match input.folder.as_deref().map(|f| f.trim().trim_matches('/')) {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => layout::folder_for_type(kind).to_string(),
    };
    let base = util::slug(&input.title, 80);
    for n in 1..1000 {
        let name = if n == 1 {
            format!("{base}.md")
        } else {
            format!("{base}-{n}.md")
        };
        let rel = format!("{folder}/{name}");
        let path = new_note_path(vault, &rel)?;
        match locks::create_new(&path, &text) {
            Ok(()) => return Ok(rel),
            Err(Error::Invalid(_)) if path.exists() => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Error::Invalid(format!(
        "Too many notes named {base} in {folder}."
    )))
}

// ---- move, rename, archive ---------------------------------------------------------------------

/// Rename without ever replacing an existing file.
fn rename_no_overwrite(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let a = CString::new(from.as_os_str().as_bytes())
            .map_err(|_| Error::Invalid("path contains a NUL byte".into()))?;
        let b = CString::new(to.as_os_str().as_bytes())
            .map_err(|_| Error::Invalid("path contains a NUL byte".into()))?;
        // SAFETY: two valid NUL-terminated paths; RENAME_EXCL fails instead of replacing.
        let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) };
        if rc == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(Error::Invalid(format!("{} already exists.", to.display())));
        }
        Err(e.into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        if to.exists() {
            return Err(Error::Invalid(format!("{} already exists.", to.display())));
        }
        std::fs::rename(from, to)?;
        Ok(())
    }
}

/// `to` as a note path: a folder (ending in `/` or an existing folder) keeps the file
/// name; `.md` is added when missing.
fn target_rel(vault: &Vault, rel: &str, to: &str) -> Result<String> {
    let t = to.trim().trim_start_matches('/');
    let is_dir = t.ends_with('/') || t.is_empty() || vault.root.join(t).is_dir();
    let t = t.trim_end_matches('/');
    let out = if is_dir {
        if t.is_empty() {
            file_name(rel).to_string()
        } else {
            format!("{t}/{}", file_name(rel))
        }
    } else if t.ends_with(".md") {
        t.to_string()
    } else {
        format!("{t}.md")
    };
    normalize(&out, "").ok_or_else(|| Error::Denied(format!("{to} is outside the vault.")))
}

/// Rewrite the links in `text` (the note at `from_rel`) that resolve to `old_rel` so they
/// point at `new_rel`. `relocated` is the note's own new path when it is the moved note,
/// so its relative markdown links are rewritten from its new folder.
fn relink(
    text: &str,
    from_rel: &str,
    relocated: Option<&str>,
    resolver: &Resolver<'_>,
    old_rel: &str,
    new_rel: &str,
) -> String {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for l in links(text) {
        let Some(hit) = resolver.resolve_link(from_rel, &l) else {
            continue;
        };
        let points_here = hit.rel == old_rel;
        if !points_here && (l.wiki || relocated.is_none()) {
            continue;
        }
        let dest = if points_here {
            new_rel
        } else {
            hit.rel.as_str()
        };
        let written = &text[l.range.clone()];
        let replacement = if l.wiki {
            let new_stem = stem_of(new_rel);
            if !written.contains('/') && !resolver.stem_taken(new_stem, old_rel) {
                new_stem.to_string()
            } else if written.ends_with(".md") {
                new_rel.to_string()
            } else {
                new_rel.trim_end_matches(".md").to_string()
            }
        } else {
            let p = if written.starts_with('/') {
                format!("/{dest}")
            } else {
                let dir = folder_of(relocated.unwrap_or(from_rel));
                relative_path(dir, dest)
            };
            let bracketed = l.range.start > 0 && text.as_bytes()[l.range.start - 1] == b'<';
            if written.contains("%20") || (!bracketed && p.contains(' ')) {
                p.replace(' ', "%20")
            } else {
                p
            }
        };
        if replacement != written {
            edits.push((l.range, replacement));
        }
    }
    let mut out = text.to_string();
    for (range, rep) in edits.into_iter().rev() {
        out.replace_range(range, &rep);
    }
    out
}

/// Move or rename a note and update every link in the vault that points at it. With
/// `dry_run` nothing is written; the result lists the notes that would change so the UI
/// can ask first. Every edit runs under the note's lock on its current text.
pub fn move_note(
    snap: &Snapshot,
    rel: &str,
    to: &str,
    dry_run: bool,
    today: &str,
) -> Result<MoveResult> {
    let vault = &snap.vault;
    let from = note_path(vault, rel)?;
    let new_rel = target_rel(vault, rel, to)?;
    if new_rel == rel {
        return Err(Error::Invalid("The note is already there.".into()));
    }
    let dest = new_note_path(vault, &new_rel)?;
    let case_only = new_rel.to_lowercase() == rel.to_lowercase();
    if !case_only && dest.exists() {
        return Err(Error::Invalid(format!("{new_rel} already exists.")));
    }
    let resolver = Resolver::new(&snap.scan);
    let referrers: Vec<&Note> = snap
        .scan
        .notes
        .iter()
        .filter(|n| n.rel != rel)
        .filter(|n| {
            links_in(n)
                .iter()
                .any(|l| resolver.resolve_link(&n.rel, l).map(|x| x.rel.as_str()) == Some(rel))
        })
        .collect();
    let relinked: Vec<NoteId> = referrers
        .iter()
        .map(|n| note_id(&vault.id, &n.rel))
        .collect();
    if dry_run {
        return Ok(MoveResult {
            id: note_id(&vault.id, &new_rel),
            relinked,
            dry_run: true,
        });
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if case_only {
        std::fs::rename(&from, &dest)?;
    } else {
        rename_no_overwrite(&from, &dest)?;
    }
    locks::guarded_edit(&dest, None, |cur| {
        let t = relink(cur, rel, Some(&new_rel), &resolver, rel, &new_rel);
        Ok(frontmatter::bump_updated(&t, today))
    })?;
    for n in &referrers {
        let path = vault.resolve_rel(&n.rel)?;
        // a referrer that vanished since the scan has nothing left to fix
        if !path.is_file() {
            continue;
        }
        locks::guarded_edit(&path, None, |cur| {
            let t = relink(cur, &n.rel, None, &resolver, rel, &new_rel);
            Ok(if t == cur {
                t
            } else {
                frontmatter::bump_updated(&t, today)
            })
        })?;
    }
    Ok(MoveResult {
        id: note_id(&vault.id, &new_rel),
        relinked,
        dry_run: false,
    })
}

/// Where `archive` puts a note: inbox items in `08-Archive/inbox/`, anything else under
/// `08-Archive/` with its path kept, with `-2`, `-3`… when that name is taken.
pub fn archive_target(vault: &Vault, rel: &str) -> Result<String> {
    if layout::is_in(rel, layout::ARCHIVE) {
        return Err(Error::Invalid(format!("{rel} is already archived.")));
    }
    let base = if layout::is_in(rel, layout::INBOX) {
        format!("{}/{}", layout::ARCHIVE_INBOX, file_name(rel))
    } else {
        format!("{}/{rel}", layout::ARCHIVE)
    };
    let stem = base.trim_end_matches(".md");
    for n in 1..1000 {
        let candidate = if n == 1 {
            base.clone()
        } else {
            format!("{stem}-{n}.md")
        };
        if !vault.root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(Error::Invalid(format!(
        "Too many archived notes named {base}."
    )))
}

pub fn archive(snap: &Snapshot, rel: &str, today: &str) -> Result<MoveResult> {
    let to = archive_target(&snap.vault, rel)?;
    move_note(snap, rel, &to, false, today)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn snap(files: &[(&str, &str)]) -> (tempfile::TempDir, Snapshot) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let vault = Vault::open(dir.path()).unwrap();
        let scan = Arc::new(crate::vault::scan(&vault));
        (dir, Snapshot { vault, scan })
    }

    fn rescan(s: &Snapshot) -> Snapshot {
        Snapshot {
            vault: s.vault.clone(),
            scan: Arc::new(crate::vault::scan(&s.vault)),
        }
    }

    #[test]
    fn parses_links() {
        let text = "See [[Garden Plan|plan]] and ![[pic]] and [[a/b#h]].\n\
                    ```\n[[not a link]]\n```\n\
                    [rel](../x/y.md) [web](https://e.com) ![img](p.png) [sp](my%20note.md \"t\")\n";
        let l = links(text);
        let targets: Vec<_> = l.iter().map(|x| x.target.as_str()).collect();
        assert_eq!(
            targets,
            vec!["Garden Plan", "pic", "a/b", "../x/y.md", "my note.md"]
        );
        assert!(l[1].embed && l[1].wiki);
        assert!(!l[3].wiki);
        assert_eq!(&text[l[0].range.clone()], "Garden Plan");
        assert_eq!(&text[l[4].range.clone()], "my%20note.md");
    }

    #[test]
    fn resolution_order() {
        let (_d, s) = snap(&[
            ("01-Projects/garden.md", ""),
            ("05-References/garden.md", ""),
            ("a/deeper/nest/Garden.md", ""),
            ("x/Plan.md", ""),
            ("Plan.md", ""),
        ]);
        let r = Resolver::new(&s.scan);
        let at = |t: &str| r.resolve(t).map(|n| n.rel.clone());
        // exact path, path + .md, case-insensitive, suffix, stem (shortest wins)
        assert_eq!(at("x/Plan.md").as_deref(), Some("x/Plan.md"));
        assert_eq!(at("x/Plan").as_deref(), Some("x/Plan.md"));
        assert_eq!(at("X/PLAN").as_deref(), Some("x/Plan.md"));
        assert_eq!(
            at("nest/garden").as_deref(),
            Some("a/deeper/nest/Garden.md")
        );
        assert_eq!(at("garden").as_deref(), Some("01-Projects/garden.md"));
        assert_eq!(at("plan").as_deref(), Some("Plan.md"));
        // never leaves the vault
        assert_eq!(at("../outside"), None);
        assert_eq!(at("/etc/hosts"), None);
        assert_eq!(at("nothing"), None);
        // markdown links are relative to the note's folder
        let l = &links("[p](../Plan.md)")[0];
        assert_eq!(
            r.resolve_link("x/note.md", l).map(|n| n.rel.as_str()),
            Some("Plan.md")
        );
        let up = &links("[p](../../../Plan.md)")[0];
        assert!(r.resolve_link("x/note.md", up).is_none());
    }

    #[test]
    fn get_with_backlinks() {
        let (_d, s) = snap(&[
            (
                "01-Projects/garden.md",
                "---\ntitle: Garden\ntype: project\n---\n\n# Garden\n\nSee [[zone-map]].\n",
            ),
            (
                "05-References/zone-map.md",
                "---\ntitle: Zones\n---\nback to [[garden]]\n",
            ),
            ("other.md", "[g](01-Projects/garden.md)\n"),
        ]);
        let snaps = vec![s];
        let v = get(&snaps, None, "garden").unwrap();
        assert_eq!(v.note.title, "Garden");
        assert_eq!(v.body.trim(), "See [[zone-map]].");
        assert_eq!(v.props[0], ("type".into(), "project".into()));
        assert_eq!(v.links.len(), 1);
        assert!(v.links[0]
            .resolved
            .as_deref()
            .unwrap()
            .ends_with(":05-References/zone-map.md"));
        let back: Vec<_> = v.backlinks.iter().map(|b| b.rel.as_str()).collect();
        assert_eq!(back, vec!["05-References/zone-map.md", "other.md"]);
        // by id
        let id = v.note.id.clone();
        assert_eq!(
            get(&snaps, None, &id).unwrap().note.rel,
            "01-Projects/garden.md"
        );
        assert!(matches!(get(&snaps, None, "nope"), Err(Error::NotFound(_))));
    }

    #[test]
    fn save_guards_and_bumps() {
        let (_d, s) = snap(&[("n.md", "---\nupdated: 2026-01-01\n---\nbody\n")]);
        let src = source(&s.vault, "n.md").unwrap();
        let r = save(
            &s.vault,
            "n.md",
            "---\nupdated: 2026-01-01\n---\nnew\n",
            &src.hash,
            "2026-11-02",
        )
        .unwrap();
        assert_eq!(r.text, "---\nupdated: 2026-11-02\n---\nnew\n");
        match save(&s.vault, "n.md", "lost\n", &src.hash, "2026-11-02") {
            Err(Error::Conflict { current }) => assert_eq!(current.text, r.text),
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert!(matches!(
            save(&s.vault, "missing.md", "x", "h", "d"),
            Err(Error::NotFound(_))
        ));
        assert!(save(&s.vault, "../escape.md", "x", "h", "d").is_err());
    }

    #[test]
    fn create_blank_and_template() {
        let (_d, s) = snap(&[(
            "_templates/meeting.md",
            "---\ntitle: {{title}}\ntype: meeting\ntags: [work]\n---\n\n# {{title}} on {{date}}\n",
        )]);
        let input = NoteInput {
            title: "Plan: the garden".into(),
            note_type: Some("project".into()),
            tags: Some(vec!["#garden".into()]),
            ..Default::default()
        };
        let rel = create(&s.vault, &input, "2026-11-02").unwrap();
        assert_eq!(rel, "01-Projects/plan-the-garden.md");
        let text = std::fs::read_to_string(s.vault.root.join(&rel)).unwrap();
        assert!(text.starts_with("---\ntitle: \"Plan: the garden\"\ntype: project\ncreated: 2026-11-02\nupdated: 2026-11-02\nauthor: human\ntags: [garden]\nstatus: active\n---\n\n# Plan: the garden\n"), "{text}");
        // same title again: a new file, the first one untouched
        let rel2 = create(&s.vault, &input, "2026-11-02").unwrap();
        assert_eq!(rel2, "01-Projects/plan-the-garden-2.md");
        assert_eq!(
            std::fs::read_to_string(s.vault.root.join(&rel)).unwrap(),
            text
        );

        let t = NoteInput {
            title: "Standup".into(),
            template: Some("meeting".into()),
            folder: Some("02-Areas/work".into()),
            author: Some("claude".into()),
            ..Default::default()
        };
        let rel = create(&s.vault, &t, "2026-11-02").unwrap();
        let text = std::fs::read_to_string(s.vault.root.join(&rel)).unwrap();
        assert_eq!(rel, "02-Areas/work/standup.md");
        assert!(text.contains("title: Standup\ntype: meeting\ntags: [work]\ncreated: 2026-11-02\nupdated: 2026-11-02\nauthor: claude\n---"), "{text}");
        assert!(text.contains("# Standup on 2026-11-02"));
        // no writing into hidden folders or out of the vault
        let bad = NoteInput {
            title: "x".into(),
            folder: Some(".vault-hq".into()),
            ..Default::default()
        };
        assert!(matches!(create(&s.vault, &bad, "d"), Err(Error::Denied(_))));
        let bad = NoteInput {
            title: "x".into(),
            folder: Some("../out".into()),
            ..Default::default()
        };
        assert!(create(&s.vault, &bad, "d").is_err());
        assert!(create(&s.vault, &NoteInput::default(), "d").is_err());
        assert!(templates(std::slice::from_ref(&s))[0].name == "meeting");
    }

    #[test]
    fn move_updates_links() {
        let (_d, s) = snap(&[
            (
                "01-Projects/garden.md",
                "---\nupdated: 2026-01-01\n---\nself [[garden]], up [r](../ref.md)\n",
            ),
            (
                "ref.md",
                "[[garden|the garden]] and [[01-Projects/garden#Tasks]] and ![[garden]]\n",
            ),
            (
                "x/other.md",
                "---\nupdated: 2026-01-01\n---\n[g](../01-Projects/garden.md) [[unrelated]]\n",
            ),
            ("unrelated.md", "nothing here\n"),
        ]);
        let dry = move_note(
            &s,
            "01-Projects/garden.md",
            "02-Areas/yard",
            true,
            "2026-11-02",
        )
        .unwrap();
        assert!(dry.dry_run);
        assert_eq!(dry.relinked.len(), 2);
        assert!(
            s.vault.root.join("01-Projects/garden.md").exists(),
            "dry run writes nothing"
        );

        let r = move_note(
            &s,
            "01-Projects/garden.md",
            "02-Areas/yard",
            false,
            "2026-11-02",
        )
        .unwrap();
        assert!(r.id.ends_with(":02-Areas/yard.md"));
        let read = |p: &str| std::fs::read_to_string(s.vault.root.join(p)).unwrap();
        assert_eq!(
            read("ref.md"),
            "[[yard|the garden]] and [[02-Areas/yard#Tasks]] and ![[yard]]\n"
        );
        assert!(read("x/other.md").contains("[g](../02-Areas/yard.md) [[unrelated]]"));
        assert!(read("x/other.md").contains("updated: 2026-11-02"));
        let moved = read("02-Areas/yard.md");
        assert!(
            moved.contains("self [[yard]], up [r](../ref.md)"),
            "{moved}"
        );
        assert!(moved.contains("updated: 2026-11-02"));
        assert_eq!(read("unrelated.md"), "nothing here\n");

        // never onto an existing note
        let s = rescan(&s);
        assert!(matches!(
            move_note(&s, "ref.md", "unrelated", false, "d"),
            Err(Error::Invalid(_))
        ));
        assert!(move_note(&s, "ref.md", "../out.md", false, "d").is_err());
    }

    #[test]
    fn archives() {
        let (_d, s) = snap(&[
            ("00-Inbox/2026-10-01-capture.md", "x\n"),
            ("08-Archive/inbox/2026-10-01-capture.md", "old\n"),
            ("01-Projects/p.md", "p\n"),
        ]);
        let r = archive(&s, "00-Inbox/2026-10-01-capture.md", "d").unwrap();
        assert!(r.id.ends_with(":08-Archive/inbox/2026-10-01-capture-2.md"));
        let s = rescan(&s);
        let r = archive(&s, "01-Projects/p.md", "d").unwrap();
        assert!(r.id.ends_with(":08-Archive/01-Projects/p.md"));
        let s = rescan(&s);
        assert!(archive(&s, "08-Archive/01-Projects/p.md", "d").is_err());
    }

    #[test]
    fn excerpts_and_lists() {
        let e = excerpt("\n# Title\n\n## Tasks\n- [ ] Do it ⏫ 📅 2026-11-01 🆔 abc123\n- [x] **Done** ✅ 2026-10-01\n", 200);
        assert_eq!(
            e,
            "Tasks\n○ Do it (due 2026-11-01)\n✓ Done (done 2026-10-01)"
        );
        assert!(excerpt(&"word ".repeat(100), 20).ends_with('…'));
        let (_d, s) = snap(&[
            (
                "01-Projects/a.md",
                "---\ntitle: Alpha\ntype: project\nupdated: 2026-10-01\n---\nbody\n",
            ),
            ("02-Areas/b.md", "---\ntitle: Beta\n---\nsecret word\n"),
        ]);
        let snaps = vec![s];
        let all = list(&snaps, &NoteFilter::default());
        assert_eq!(all.len(), 2);
        let f = |f: NoteFilter| {
            list(&snaps, &f)
                .into_iter()
                .map(|n| n.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            f(NoteFilter {
                note_type: Some("project".into()),
                ..Default::default()
            }),
            vec!["Alpha"]
        );
        assert_eq!(
            f(NoteFilter {
                text: Some("SECRET".into()),
                ..Default::default()
            }),
            vec!["Beta"]
        );
        assert_eq!(
            f(NoteFilter {
                folder: Some("01-Projects/".into()),
                ..Default::default()
            }),
            vec!["Alpha"]
        );
        assert!(is_system("AGENTS.md") && is_system("01-Projects/x/CLAUDE.md"));
        assert!(is_system("06-Memory/sessions/garden.md") && is_system("07-Agents/cursor.md"));
        let mut p = std::collections::BTreeMap::new();
        p.insert("tags".to_string(), "[inbox, compact, garden]".to_string());
        assert!(is_session_capture(&p));
        p.insert("tags".to_string(), "[inbox, idea]".to_string());
        assert!(!is_session_capture(&p));
        assert!(!is_system("01-Projects/memory-palace.md"));
        assert_eq!(
            f(NoteFilter {
                vault: Some("other".into()),
                ..Default::default()
            }),
            Vec::<String>::new()
        );
    }
}
