//! `Backend` over hq-core, for the vaults the app knows.
//!
//! Real today (Phase 0 foundation): vault_info, read_note, list_notes, create_note,
//! append_to_note, capture, log, both resources, and a plain keyword `search`.
//!
//! TODO(integrator): wire these to the streams that own them, then delete the stubs:
//! - `search`      → stream C `hq_core::search` (BM25 + vectors, SPEC 6); the keyword scan
//!   below is a stand-in so agents get something useful meanwhile.
//! - `ask`         → stream C, non-streaming ask (collect the tokens, return the sources).
//! - `add_task`, `update_task`, `list_tasks` → stream B `hq_core::tasks` (add / update /
//!   query); map `TaskPatch` and `ListTasksInput` onto B's types.
//! - `add_bookmark` → stream F `hq_core::bookmarks` (`bookmark_add(url, note)`).
//! - `read_note` by `ref` → stream A's link resolver once `notes.rs` has one; `resolve_ref`
//!   here follows SPEC 3's order meanwhile.
//! - `create_note` templates (`_templates/*.md`) → stream A's `note_create`.
//! - `capture` / `log` → stream A's `capture.rs` / `days.rs` once they exist, so the
//!   inbox and daily-note formats live in one place.

use std::path::{Component, Path, PathBuf};

use hq_core::frontmatter;
use hq_core::locks;
use hq_core::settings::{self, Settings};
use hq_core::types::{note_id, split_note_id};
use hq_core::util;
use hq_core::vault::{scan, Note, Vault};
use hq_core::{Error, Result};

use crate::backend::*;

pub struct CoreBackend {
    vaults: Vec<Vault>,
    primary: usize,
}

const INBOX: &str = "00-Inbox";
const DAILY: &str = "09-Planning/Daily";
const AGENT_LOG: &str = "Agent log";

impl CoreBackend {
    /// `primary` is a vault id; the first vault is primary when it is missing or unknown.
    pub fn new(vaults: Vec<Vault>, primary: Option<&str>) -> Self {
        let primary = primary
            .and_then(|p| vaults.iter().position(|v| v.id == p))
            .unwrap_or(0);
        CoreBackend { vaults, primary }
    }

    /// The vaults in the app's settings, minus blocked or missing ones (logged to stderr).
    pub fn from_settings(s: &Settings) -> Self {
        let mut vaults = Vec::new();
        for entry in &s.vaults {
            let dir = Path::new(&entry.path);
            if let Some(why) = settings::blocked(s, dir) {
                eprintln!("vault-hq mcp: skipping a vault: {why}");
                continue;
            }
            match Vault::open(dir) {
                Ok(v) => vaults.push(v),
                Err(e) => eprintln!("vault-hq mcp: skipping vault \"{}\": {e}", entry.name),
            }
        }
        CoreBackend::new(vaults, s.primary_vault.as_deref())
    }

    pub fn vaults(&self) -> &[Vault] {
        &self.vaults
    }

    fn none_yet() -> Error {
        Error::Unavailable(
            "No vault is set up yet. Open Vault HQ and add or create one, then reconnect.".into(),
        )
    }

    /// By id or (case-insensitive) name; `None` = the primary vault.
    fn vault(&self, which: Option<&str>) -> Result<&Vault> {
        if self.vaults.is_empty() {
            return Err(Self::none_yet());
        }
        match which.map(str::trim).filter(|w| !w.is_empty()) {
            None => Ok(&self.vaults[self.primary]),
            Some(w) => self
                .vaults
                .iter()
                .find(|v| v.id == w || v.name.eq_ignore_ascii_case(w))
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "No vault \"{w}\". Known: {}",
                        self.vaults
                            .iter()
                            .map(|v| format!("{} ({})", v.name, v.id))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }),
        }
    }

    /// The vaults a call covers: one if named, else all of them, primary first.
    fn vaults_for(&self, which: Option<&str>) -> Result<Vec<&Vault>> {
        if which.is_some_and(|w| !w.trim().is_empty()) {
            return Ok(vec![self.vault(which)?]);
        }
        if self.vaults.is_empty() {
            return Err(Self::none_yet());
        }
        let mut all: Vec<&Vault> = vec![&self.vaults[self.primary]];
        all.extend(
            self.vaults
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != self.primary)
                .map(|(_, v)| v),
        );
        Ok(all)
    }

    /// A note id to its vault and file. Only existing `.md` files outside hidden folders.
    fn locate(&self, id: &str) -> Result<(&Vault, String, PathBuf)> {
        let (vid, rel) = split_note_id(id)
            .ok_or_else(|| Error::Invalid(format!("\"{id}\" is not a note id")))?;
        let vault = self
            .vaults
            .iter()
            .find(|v| v.id == vid)
            .ok_or_else(|| Error::NotFound(format!("No vault with id {vid}")))?;
        let path = safe_path(vault, rel)?;
        if !rel.ends_with(".md") || !path.is_file() {
            return Err(Error::NotFound(format!(
                "No note {rel} in {}. Use search or list_notes for ids.",
                vault.name
            )));
        }
        Ok((vault, rel.to_string(), path))
    }

    /// SPEC 3's link resolution: exact path, path + .md, case-insensitive path, path
    /// suffix, then stem. Vaults in `vaults_for` order; first hit wins.
    fn resolve_ref(&self, reference: &str, vault: Option<&str>) -> Result<String> {
        let target = clean_ref(reference);
        if target.is_empty() {
            return Err(Error::Invalid(format!("\"{reference}\" is not a link")));
        }
        let lower = target.to_lowercase();
        let with_md = if lower.ends_with(".md") {
            target.clone()
        } else {
            format!("{target}.md")
        };
        let lower_md = with_md.to_lowercase();
        for v in self.vaults_for(vault)? {
            let notes = scan(v).notes;
            let rels: Vec<&str> = notes.iter().map(|n| n.rel.as_str()).collect();
            let hit = rels
                .iter()
                .find(|r| **r == target)
                .or_else(|| rels.iter().find(|r| **r == with_md))
                .or_else(|| rels.iter().find(|r| r.to_lowercase() == lower_md))
                .or_else(|| {
                    rels.iter()
                        .find(|r| r.to_lowercase().ends_with(&format!("/{lower_md}")))
                })
                .or_else(|| {
                    notes
                        .iter()
                        .find(|n| n.stem.to_lowercase() == lower.trim_end_matches(".md"))
                        .map(|n| &n.rel)
                        .and_then(|r| rels.iter().find(|x| **x == r.as_str()))
                });
            if let Some(rel) = hit {
                return Ok(note_id(&v.id, rel));
            }
        }
        Err(Error::NotFound(format!(
            "No note matches \"{reference}\". Try search."
        )))
    }
}

/// `[[garden#Plan|the garden]]` → `garden`.
fn clean_ref(r: &str) -> String {
    let r = r.trim();
    let r = r.strip_prefix('!').unwrap_or(r);
    let r = r
        .strip_prefix("[[")
        .and_then(|x| x.strip_suffix("]]"))
        .unwrap_or(r);
    let r = r.split('|').next().unwrap_or("");
    let r = r.split('#').next().unwrap_or("");
    r.trim().trim_start_matches("./").to_string()
}

/// A vault-relative path for reading or writing: inside the vault, and never in a hidden
/// folder (`.vault-hq/`, `.obsidian/`, `.git/`) where notes don't live.
fn safe_path(vault: &Vault, rel: &str) -> Result<PathBuf> {
    let p = vault.resolve_rel(rel)?;
    let hidden = Path::new(rel)
        .components()
        .any(|c| matches!(c, Component::Normal(s) if s.to_string_lossy().starts_with('.')));
    if hidden {
        return Err(Error::Denied(format!(
            "{rel} is in a hidden folder; notes live in the vault's regular folders"
        )));
    }
    Ok(p)
}

/// The full file a `NoteOut` was read from is not kept; resources read the file again.
fn read_text(path: &Path) -> Result<(String, String)> {
    locks::read(path)
}

fn local_date(mtime: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(mtime, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// The date a note counts as last touched: `updated:`, else `created:`, else its mtime.
fn note_date(n: &Note) -> String {
    n.props
        .get("updated")
        .or_else(|| n.props.get("created"))
        .filter(|d| crate::tools::is_date(d))
        .cloned()
        .unwrap_or_else(|| local_date(n.mtime))
}

fn days_ago(days: u32) -> String {
    (chrono::Local::now() - chrono::Duration::days(days as i64))
        .format("%Y-%m-%d")
        .to_string()
}

fn is_bookmark(n: &Note) -> bool {
    n.props.get("type").is_some_and(|t| t == "bookmark") || n.rel.starts_with("10-Bookmarks/")
}

/// Indent continuation lines so a multi-line capture stays one list item.
fn list_item(prefix: &str, text: &str) -> String {
    let mut lines = text.trim().lines();
    let mut s = format!("{prefix}{}", lines.next().unwrap_or(""));
    for l in lines {
        s.push_str("\n  ");
        s.push_str(l);
    }
    s
}

fn heading_of(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end_matches(['\n', '\r']);
    let level = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) && t[level..].starts_with(' ') {
        Some((level, t[level..].trim()))
    } else {
        None
    }
}

/// Put `addition` at the end of the section under `heading` (any level; the section ends
/// at the next heading of the same or a higher level), before its trailing blank lines.
/// No heading: at the end of the note. A missing heading is added as `## heading`.
/// Headings inside fenced code blocks don't count.
pub(crate) fn insert_text(text: &str, heading: Option<&str>, addition: &str) -> String {
    let addition = addition.trim_end_matches(['\n', '\r']);
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let addition = addition.replace("\r\n", "\n").replace('\n', nl);
    let end_of_note = |t: &str| {
        let mut s = t.to_string();
        if !s.is_empty() && !s.ends_with('\n') {
            s.push_str(nl);
        }
        s
    };
    let Some(heading) = heading else {
        return format!("{}{addition}{nl}", end_of_note(text));
    };
    let want = heading.trim().trim_start_matches('#').trim().to_lowercase();
    let mut offset = 0;
    let mut fenced = false;
    let mut section: Option<(usize, usize)> = None; // (level, start of section body)
    let mut end = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced {
            if let Some((level, title)) = heading_of(line) {
                match section {
                    None if title.to_lowercase() == want => {
                        section = Some((level, offset + line.len()))
                    }
                    Some((l, _)) if level <= l => {
                        end = Some(offset);
                        break;
                    }
                    _ => {}
                }
            }
        }
        offset += line.len();
    }
    let Some((_, body_start)) = section else {
        let base = end_of_note(text);
        let gap = if base.is_empty() || base.ends_with(&format!("{nl}{nl}")) {
            ""
        } else {
            nl
        };
        return format!("{base}{gap}## {}{nl}{nl}{addition}{nl}", heading.trim());
    };
    let end = end.unwrap_or(text.len());
    // insert after the last non-blank line of the section
    let body = &text[body_start..end];
    let kept = body.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    let at = body_start + kept;
    let (before, after) = text.split_at(at);
    if kept == 0 {
        // empty section: right under the heading, keeping the blank line before the next one
        if after.trim().is_empty() {
            return format!("{before}{addition}{nl}");
        }
        let next = after.trim_start_matches(['\n', '\r']);
        return format!("{before}{addition}{nl}{nl}{next}");
    }
    let tail = if after.is_empty() {
        nl.to_string()
    } else {
        after.to_string()
    };
    format!("{before}{nl}{addition}{tail}")
}

fn snippet(body: &str, term: &str) -> String {
    let lower = body.to_lowercase();
    let at = lower.find(term).unwrap_or(0);
    // `lower` can differ in byte length from `body` for some scripts; clamp to a boundary
    let mut start = at.saturating_sub(80).min(body.len());
    while !body.is_char_boundary(start) {
        start -= 1;
    }
    let s: String = body[start..].chars().take(200).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if start > 0 {
        format!("…{s}")
    } else {
        s
    }
}

impl Backend for CoreBackend {
    fn search(&self, ctx: &Ctx, input: SearchInput) -> Result<SearchOutput> {
        let terms: Vec<String> = input
            .query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let since = input.scope.days.map(days_ago);
        let mut hits = Vec::new();
        let vaults: Vec<&Vault> = if input.scope.vaults.is_empty() {
            self.vaults_for(None)?
        } else {
            input
                .scope
                .vaults
                .iter()
                .map(|w| self.vault(Some(w)))
                .collect::<Result<_>>()?
        };
        for v in vaults {
            for n in scan(v).notes {
                ctx.check()?;
                let bookmark = is_bookmark(&n);
                match input.scope.kind {
                    ScopeKind::Notes if bookmark => continue,
                    ScopeKind::Bookmarks if !bookmark => continue,
                    _ => {}
                }
                if since.as_ref().is_some_and(|s| note_date(&n) < *s) {
                    continue;
                }
                let title = n.title().to_lowercase();
                let body = n.body.to_lowercase();
                if !terms
                    .iter()
                    .all(|t| title.contains(t) || body.contains(t) || n.rel.contains(t))
                {
                    continue;
                }
                let score: usize = terms
                    .iter()
                    .map(|t| title.matches(t).count() * 5 + body.matches(t).count())
                    .sum();
                hits.push(SearchHit {
                    id: note_id(&v.id, &n.rel),
                    title: n.title().to_string(),
                    snippet: snippet(&n.body, &terms[0]),
                    score: score as f32,
                });
            }
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(input.limit.unwrap_or(10) as usize);
        Ok(SearchOutput { hits })
    }

    fn ask(&self, _ctx: &Ctx, _input: AskInput) -> Result<AskOutput> {
        Err(Error::Unavailable(
            "Ask is not connected in this build yet; use search and read_note.".into(),
        ))
    }

    fn read_note(&self, _ctx: &Ctx, input: ReadNoteInput) -> Result<NoteOut> {
        let id = match (input.id, input.reference) {
            (Some(id), _) => id,
            (None, Some(r)) => self.resolve_ref(&r, input.vault.as_deref())?,
            (None, None) => return Err(Error::Invalid("give `id` or `ref`".into())),
        };
        let (_, rel, path) = self.locate(&id)?;
        let (text, hash) = read_text(&path)?;
        let p = frontmatter::parse(&text);
        let stem = Path::new(&rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = p
            .props
            .get("title")
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or(stem);
        Ok(NoteOut {
            id,
            title,
            body: p.body.to_string(),
            props: p.props,
            hash,
        })
    }

    fn list_notes(&self, ctx: &Ctx, input: ListNotesInput) -> Result<ListNotesOutput> {
        let folder = input
            .folder
            .as_deref()
            .map(|f| f.trim().trim_matches('/').to_string())
            .filter(|f| !f.is_empty());
        let mut rows: Vec<(String, NoteSummary)> = Vec::new();
        for v in self.vaults_for(input.vault.as_deref())? {
            ctx.check()?;
            for n in scan(v).notes {
                if let Some(f) = &folder {
                    if !n.rel.starts_with(&format!("{f}/")) {
                        continue;
                    }
                }
                if let Some(t) = &input.note_type {
                    if n.props.get("type").is_none_or(|x| x != t) {
                        continue;
                    }
                }
                let date = note_date(&n);
                if input.updated_since.as_ref().is_some_and(|s| date < *s) {
                    continue;
                }
                rows.push((
                    date.clone(),
                    NoteSummary {
                        id: note_id(&v.id, &n.rel),
                        title: n.title().to_string(),
                        note_type: n.props.get("type").cloned(),
                        updated: Some(date).filter(|d| !d.is_empty()),
                        tags: n
                            .props
                            .get("tags")
                            .map(|t| frontmatter::list(t))
                            .unwrap_or_default(),
                    },
                ));
            }
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
        let total = rows.len() as u32;
        let offset = input.offset.unwrap_or(0) as usize;
        let limit = input.limit.unwrap_or(100) as usize;
        let notes: Vec<NoteSummary> = rows
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|r| r.1)
            .collect();
        let next = offset + notes.len();
        Ok(ListNotesOutput {
            notes,
            total,
            next_offset: (next < total as usize).then_some(next as u32),
        })
    }

    fn create_note(&self, ctx: &Ctx, input: CreateNoteInput) -> Result<WriteOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let folder = input
            .folder
            .as_deref()
            .map(|f| f.trim().trim_matches('/'))
            .filter(|f| !f.is_empty())
            .unwrap_or(INBOX);
        if folder
            .split('/')
            .any(|c| c.eq_ignore_ascii_case("_templates"))
        {
            return Err(Error::Denied(
                "Templates are managed in the app, not over MCP".into(),
            ));
        }
        let rel = format!("{folder}/{}.md", util::slug(&input.title, 60));
        let path = safe_path(vault, &rel)?;
        let today = util::today();
        let mut props = vec![
            ("title", input.title.trim().to_string()),
            (
                "type",
                input
                    .note_type
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("note")
                    .to_string(),
            ),
            ("created", today.clone()),
            ("updated", today),
            ("author", ctx.author.clone()),
        ];
        if !input.tags.is_empty() {
            let tags: Vec<String> = input
                .tags
                .iter()
                .map(|t| t.trim_start_matches('#').to_string())
                .collect();
            props.push(("tags", frontmatter::format_list(&tags)));
        }
        let mut text = frontmatter::render(&props);
        let body = input.body.unwrap_or_default();
        let body = body.trim_matches('\n');
        if !body.is_empty() {
            text.push_str(body);
            text.push('\n');
        }
        locks::create_new(&path, &text).map_err(|e| match e {
            Error::Invalid(_) => Error::Invalid(format!(
                "{rel} already exists. Read it and use append_to_note, or pick another title."
            )),
            e => e,
        })?;
        Ok(WriteOutput {
            id: note_id(&vault.id, &rel),
            hash: locks::sha256(&text),
        })
    }

    fn append_to_note(&self, _ctx: &Ctx, input: AppendInput) -> Result<WriteOutput> {
        let (vault, rel, path) = self.locate(&input.id)?;
        let today = util::today();
        let hash = locks::guarded_edit(&path, Some(&input.base_hash), |cur| {
            let next = insert_text(cur, input.heading.as_deref(), &input.text);
            Ok(frontmatter::bump_updated(&next, &today))
        })?;
        Ok(WriteOutput {
            id: note_id(&vault.id, &rel),
            hash,
        })
    }

    fn capture(&self, ctx: &Ctx, input: CaptureInput) -> Result<LineOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let today = util::today();
        let rel = format!("{INBOX}/{today}-capture.md");
        let path = safe_path(vault, &rel)?;
        // agents sign their line the way the daily Agent log does: `- HH:MM who · text`
        let line = list_item(
            &format!("- {} {} · ", util::now_hhmm(), ctx.author),
            &input.text,
        );
        let author = ctx.author.clone();
        locks::append(
            &path,
            || {
                frontmatter::render(&[
                    ("title", format!("Capture {today}")),
                    ("type", "capture".into()),
                    ("created", today.clone()),
                    ("updated", today.clone()),
                    ("author", author),
                ])
            },
            &line,
        )?;
        Ok(LineOutput {
            id: note_id(&vault.id, &rel),
            line,
        })
    }

    fn log(&self, ctx: &Ctx, input: LogInput) -> Result<LineOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let today = util::today();
        let rel = format!("{DAILY}/{today}.md");
        let path = safe_path(vault, &rel)?;
        if !path.exists() {
            let mut text = frontmatter::render(&[
                ("title", today.clone()),
                ("type", "daily".into()),
                ("created", today.clone()),
                ("updated", today.clone()),
                ("author", ctx.author.clone()),
            ]);
            for h in ["Quick Wins", "Work", "Learning", "Tomorrow", AGENT_LOG] {
                text.push_str(&format!("## {h}\n\n"));
            }
            match locks::create_new(&path, &text) {
                Ok(()) | Err(Error::Invalid(_)) => {} // created meanwhile by someone else
                Err(e) => return Err(e),
            }
        }
        let line = format!(
            "- {} {} · {}",
            util::now_hhmm(),
            ctx.author,
            input.text.trim()
        );
        locks::guarded_edit(&path, None, |cur| {
            let next = insert_text(cur, Some(AGENT_LOG), &line);
            Ok(frontmatter::bump_updated(&next, &today))
        })?;
        Ok(LineOutput {
            id: note_id(&vault.id, &rel),
            line,
        })
    }

    fn add_task(&self, _ctx: &Ctx, _input: AddTaskInput) -> Result<TaskOut> {
        Err(tasks_pending())
    }

    fn update_task(&self, _ctx: &Ctx, _input: UpdateTaskInput) -> Result<TaskOut> {
        Err(tasks_pending())
    }

    fn list_tasks(&self, _ctx: &Ctx, _input: ListTasksInput) -> Result<ListTasksOutput> {
        Err(tasks_pending())
    }

    fn add_bookmark(&self, _ctx: &Ctx, _input: AddBookmarkInput) -> Result<BookmarkOut> {
        Err(Error::Unavailable(
            "Bookmarks are not connected in this build yet; capture the URL instead.".into(),
        ))
    }

    fn vault_info(&self, ctx: &Ctx) -> Result<VaultInfoOutput> {
        let mut vaults = Vec::new();
        for (i, v) in self.vaults.iter().enumerate() {
            ctx.check()?;
            let s = scan(v);
            let mut folders: Vec<String> = std::fs::read_dir(&v.root)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect();
            folders.sort();
            vaults.push(VaultSummary {
                id: v.id.clone(),
                name: v.name.clone(),
                primary: i == self.primary,
                notes: s.notes.len() as u32,
                unreadable: s.unreadable.len() as u32,
                folders,
                has_agents_md: v.root.join("AGENTS.md").is_file(),
            });
        }
        Ok(VaultInfoOutput {
            vaults,
            author: ctx.author.clone(),
            today: util::today(),
            server_version: env!("CARGO_PKG_VERSION").into(),
        })
    }

    fn agents_md(&self, _ctx: &Ctx) -> Result<ResourceText> {
        let vault = self.vault(None)?;
        let path = vault.root.join("AGENTS.md");
        if !path.is_file() {
            return Err(Error::NotFound(format!(
                "{} has no AGENTS.md yet (Vault HQ onboarding writes one). Until then: new \
                 notes go to 00-Inbox, never delete, and log what you change.",
                vault.name
            )));
        }
        let (text, _) = read_text(&path)?;
        Ok(ResourceText {
            mime_type: "text/markdown".into(),
            text,
        })
    }

    fn note_resource(&self, _ctx: &Ctx, id: &str) -> Result<ResourceText> {
        let (_, _, path) = self.locate(id)?;
        let (text, _) = read_text(&path)?;
        Ok(ResourceText {
            mime_type: "text/markdown".into(),
            text,
        })
    }
}

fn tasks_pending() -> Error {
    Error::Unavailable(
        "Tasks are not connected in this build yet; capture the task as text instead.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs() {
        assert_eq!(clean_ref("[[garden#Plan|the garden]]"), "garden");
        assert_eq!(clean_ref("![[img.png]]"), "img.png");
        assert_eq!(
            clean_ref("./01-Projects/garden.md"),
            "01-Projects/garden.md"
        );
    }

    #[test]
    fn insert_at_end() {
        assert_eq!(insert_text("a", None, "b"), "a\nb\n");
        assert_eq!(insert_text("a\n", None, "b\n"), "a\nb\n");
        assert_eq!(insert_text("", None, "b"), "b\n");
    }

    #[test]
    fn insert_in_section() {
        let t = "# N\n\n## Notes\n\n- one\n\n## Tasks\n\n- [ ] x\n";
        assert_eq!(
            insert_text(t, Some("Notes"), "- two"),
            "# N\n\n## Notes\n\n- one\n- two\n\n## Tasks\n\n- [ ] x\n"
        );
        // the last section, and a deeper heading inside it stays in it
        let t = "## Log\n- a\n### sub\n- b\n";
        assert_eq!(
            insert_text(t, Some("log"), "- c"),
            "## Log\n- a\n### sub\n- b\n- c\n"
        );
    }

    #[test]
    fn insert_in_empty_section() {
        let t = "## Work\n\n## Agent log\n\n";
        assert_eq!(
            insert_text(t, Some("Agent log"), "- 10:00 claude · hi"),
            "## Work\n\n## Agent log\n- 10:00 claude · hi\n"
        );
        let t = "## Agent log\n\n## Tomorrow\n";
        assert_eq!(
            insert_text(t, Some("Agent log"), "- x"),
            "## Agent log\n- x\n\n## Tomorrow\n"
        );
    }

    #[test]
    fn insert_adds_missing_heading() {
        assert_eq!(
            insert_text("body\n", Some("Notes"), "- n"),
            "body\n\n## Notes\n\n- n\n"
        );
    }

    #[test]
    fn insert_ignores_fenced_headings() {
        let t = "## A\n```\n## B\n```\n- a\n## B\n- b\n";
        assert_eq!(
            insert_text(t, Some("A"), "- new"),
            "## A\n```\n## B\n```\n- a\n- new\n## B\n- b\n"
        );
    }

    #[test]
    fn insert_keeps_crlf() {
        assert_eq!(
            insert_text("## A\r\n- a\r\n", Some("A"), "- b"),
            "## A\r\n- a\r\n- b\r\n"
        );
    }

    #[test]
    fn multi_line_items() {
        assert_eq!(list_item("- ", "one\ntwo\n"), "- one\n  two");
    }

    #[test]
    fn snippets_are_safe() {
        let body = "ünïcödé ".repeat(40) + "needle here";
        let s = snippet(&body, "needle");
        assert!(s.contains("needle"));
    }
}
