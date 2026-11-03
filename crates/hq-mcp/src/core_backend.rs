//! `Backend` over `hq_core::hq::Hq`: the same registry, notes, capture, days, tasks,
//! search and bookmark code the app runs, so an agent's write and a click in the app make
//! the same file change.
//!
//! The app watches its vaults; this process does not, so every call starts by dropping
//! the cached scans (`Hq::invalidate`). A read never sees a stale note, and a hash an
//! agent gets is the hash on disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hq_core::bookmarks::{enrich, store};
use hq_core::hq::Hq;
use hq_core::ollama::{Chat, Models};
use hq_core::search::{self, ask, Index, Semantic};
use hq_core::settings::{Settings, Store, VaultEntry};
use hq_core::tasks::{mutate, query, resolve_today, TaskIndex};
use hq_core::types::{
    split_note_id, AskEvent, NoteFilter, NoteInput, NoteView, ScopeKinds, SearchResult,
    SearchScope, Task, TaskGroups, TaskInput, TaskPatch, TaskStatus,
};
use hq_core::vault::{layout, registry, Vault};
use hq_core::{capture, days, frontmatter, locks, notes, util};
use hq_core::{Error, Result};

use crate::backend::*;

pub struct CoreBackend {
    hq: Hq,
    /// The search index, brought up to date per call (unchanged vaults are not re-cut).
    index: Mutex<Arc<Index>>,
    /// Fetch a new bookmark's page title (one capped request, as the app does).
    fetch_pages: bool,
    /// The private settings folder `with_vaults` made; removed with the backend.
    _temp: Option<TempDir>,
}

/// A folder removed on drop.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl CoreBackend {
    pub fn from_hq(hq: Hq) -> Self {
        CoreBackend {
            hq,
            index: Mutex::new(Arc::new(Index::new())),
            fetch_pages: true,
            _temp: None,
        }
    }

    /// The app's own state: settings in `support`, caches in `cache`. Settings that don't
    /// parse leave the server running with no vaults, as the app does, never rewriting them.
    pub fn open(support: &Path, cache: &Path) -> Self {
        let hq = Hq::open(support, cache).unwrap_or_else(|e| {
            eprintln!("vault-hq mcp: {e}");
            Hq::degraded(support, cache, &e)
        });
        Self::from_hq(hq)
    }

    /// Serve exactly these folders (the first is primary) with `base`'s other settings:
    /// its blocklist (checked on every folder), model server and models. Used by
    /// `--vault` and tests; the app's settings file is not touched.
    pub fn with_vaults(paths: &[PathBuf], mut base: Settings, cache: &Path) -> Result<Self> {
        let mut entries = Vec::new();
        for p in paths {
            let abs = std::path::absolute(p)?;
            let v = registry::open_checked(&base, &abs)?;
            entries.push(VaultEntry {
                id: v.id,
                path: abs.to_string_lossy().into_owned(),
                name: v.name,
            });
        }
        base.primary_vault = entries.first().map(|e| e.id.clone());
        base.vaults = entries;
        let dir = std::env::temp_dir().join(format!("vault-hq-mcp-{}", util::random_hex(8)));
        std::fs::create_dir_all(&dir)?;
        let temp = TempDir(dir);
        Store::new(&temp.0).save(&base)?;
        let hq = Hq::open(&temp.0, cache)?;
        Ok(CoreBackend {
            _temp: Some(temp),
            ..Self::from_hq(hq)
        })
    }

    /// No page fetch for new bookmarks (tests, offline use).
    pub fn without_page_fetch(mut self) -> Self {
        self.fetch_pages = false;
        self
    }

    pub fn hq(&self) -> &Hq {
        &self.hq
    }

    fn none_yet() -> Error {
        Error::Unavailable(
            "No vault is set up yet. Open Vault HQ and add or create one, then reconnect.".into(),
        )
    }

    /// Start every call from the disk, not a cached scan.
    fn fresh(&self) {
        self.hq.invalidate(None);
    }

    /// The open vaults, primary first.
    fn vaults(&self) -> Result<Vec<Vault>> {
        let reg = self.hq.registry();
        let mut all = reg.vaults().to_vec();
        if all.is_empty() {
            return Err(Self::none_yet());
        }
        if let Some(i) = reg
            .primary_id()
            .and_then(|p| all.iter().position(|v| v.id == p))
        {
            let p = all.remove(i);
            all.insert(0, p);
        }
        Ok(all)
    }

    /// By id or (case-insensitive) name; `None` = the primary vault.
    fn vault(&self, which: Option<&str>) -> Result<Vault> {
        let all = self.vaults()?;
        match which.map(str::trim).filter(|w| !w.is_empty()) {
            None => Ok(all[0].clone()),
            Some(w) => all
                .iter()
                .find(|v| v.id == w || v.name.eq_ignore_ascii_case(w))
                .cloned()
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "No vault \"{w}\". Known: {}",
                        all.iter()
                            .map(|v| format!("{} ({})", v.name, v.id))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }),
        }
    }

    fn vault_id(&self, which: Option<&str>) -> Result<Option<String>> {
        match which.map(str::trim).filter(|w| !w.is_empty()) {
            None => Ok(None),
            Some(w) => Ok(Some(self.vault(Some(w))?.id)),
        }
    }

    fn scope(&self, s: &Scope) -> Result<SearchScope> {
        Ok(SearchScope {
            vaults: s
                .vaults
                .iter()
                .map(|w| self.vault(Some(w)).map(|v| v.id))
                .collect::<Result<_>>()?,
            days: s.days,
            until: None,
            kinds: match s.kind {
                ScopeKind::All => ScopeKinds::Both,
                ScopeKind::Notes => ScopeKinds::Notes,
                ScopeKind::Bookmarks => ScopeKinds::Bookmarks,
            },
        })
    }

    /// The search index over the current scans.
    fn search_index(&self) -> Result<Arc<Index>> {
        let snaps = self.hq.snapshots();
        if snaps.is_empty() {
            return Err(Self::none_yet());
        }
        let ids: Vec<String> = snaps.iter().map(|s| s.vault.id.clone()).collect();
        let mut guard = lock(&self.index);
        let idx = Arc::make_mut(&mut guard);
        idx.retain_vaults(&ids);
        for s in &snaps {
            idx.update_vault(&s.vault.id, &s.scan.notes);
        }
        Ok(guard.clone())
    }

    /// A task index built from disk, and B's mutation context over it.
    fn with_tasks<T>(&self, f: impl FnOnce(&mut mutate::Ctx) -> Result<T>) -> Result<T> {
        let vaults = self.vaults()?;
        let mut index = TaskIndex::build(&vaults);
        let mut ctx = mutate::Ctx {
            vaults: &vaults,
            index: &mut index,
            today: resolve_today(None)?,
        };
        let out = f(&mut ctx);
        self.hq.invalidate(None);
        out
    }
}

/// Notes live in the vault's regular folders; HQ's own and editors' folders are off limits.
fn check_folder(folder: &str) -> Result<()> {
    if folder
        .split('/')
        .any(|c| c.starts_with('.') || c.eq_ignore_ascii_case("_templates"))
    {
        return Err(Error::Denied(format!(
            "{folder} is not a notes folder (hidden folders and _templates are managed by the app)."
        )));
    }
    Ok(())
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
    let body = &text[body_start..end];
    let kept = body.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    let at = body_start + kept;
    let (before, after) = text.split_at(at);
    if kept == 0 {
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

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Backend for CoreBackend {
    fn search(&self, ctx: &Ctx, input: SearchInput) -> Result<SearchResult> {
        self.fresh();
        let scope = self.scope(&input.scope)?;
        let index = self.search_index()?;
        ctx.check()?;
        let settings = self.hq.settings();
        // the client refuses any model server that isn't this Mac or the home network
        let models = Models::new(&settings.ollama).ok();
        let embedder = models.as_ref().map(Models::embedder);
        let semantic = embedder
            .as_ref()
            .filter(|_| !settings.ollama.embed_model.is_empty())
            .map(|e| Semantic {
                embedder: e,
                cache_dir: self.hq.cache_dir(),
            });
        search::search(
            &index,
            &input.query,
            &scope,
            input.limit.unwrap_or(10) as usize,
            semantic,
            &util::today(),
        )
    }

    fn ask(&self, ctx: &Ctx, input: AskInput) -> Result<AskOutput> {
        self.fresh();
        let scope = self.scope(&input.scope)?;
        let settings = self.hq.settings();
        let models = Models::new(&settings.ollama)?;
        let index = self.search_index()?;
        let vaults = self.vaults()?;
        let embedder = models.embedder();
        let chat = models.chat();
        let semantic = (!settings.ollama.embed_model.is_empty()).then(|| Semantic {
            embedder: &embedder,
            cache_dir: self.hq.cache_dir(),
        });
        let chat: Option<&dyn Chat> = (!settings.ollama.chat_model.is_empty()).then_some(&chat);
        let today = util::today();
        let mut sources = Vec::new();
        let mut semantic_used = false;
        // over MCP the answer is not saved as a conversation note: ask stays read-only
        let answered = ask::ask(
            &index,
            &ask::AskInput {
                question: &input.question,
                scope: &scope,
                conversation: None,
                today: &today,
            },
            &ask::AskVaults {
                all: &vaults,
                save_to: None,
            },
            semantic,
            chat,
            ctx.cancel_token(),
            &mut |e| {
                if let AskEvent::Sources {
                    sources: s,
                    semantic,
                } = e
                {
                    sources = s;
                    semantic_used = semantic;
                }
            },
        )?;
        ctx.check()?;
        Ok(AskOutput {
            answer: answered.answer,
            model: answered.model,
            sources,
            semantic: semantic_used,
        })
    }

    fn read_note(&self, _ctx: &Ctx, input: ReadNoteInput) -> Result<NoteView> {
        self.fresh();
        if self.hq.registry().vaults().is_empty() {
            return Err(Self::none_yet());
        }
        match (input.id, input.reference) {
            (Some(id), _) => {
                let (v, _) = self.hq.locate(&id)?;
                notes::get(&[self.hq.snapshot(&v.id)?], None, &id)
            }
            (None, Some(r)) => match input.vault.as_deref() {
                Some(w) => {
                    let v = self.vault(Some(w))?;
                    notes::get(&[self.hq.snapshot(&v.id)?], None, &r)
                }
                None => self.hq.note_get(&r),
            },
            (None, None) => Err(Error::Invalid("give `id` or `ref`".into())),
        }
    }

    fn list_notes(&self, _ctx: &Ctx, input: ListNotesInput) -> Result<ListNotesOutput> {
        self.fresh();
        let filter = NoteFilter {
            vault: self.vault_id(input.vault.as_deref())?,
            folder: input.folder,
            note_type: input.note_type,
            text: input.text,
            updated_since: input.updated_since,
            limit: None,
        };
        let all = self.hq.notes_list(&filter);
        let total = all.len() as u32;
        let offset = input.offset.unwrap_or(0) as usize;
        let limit = input.limit.unwrap_or(100) as usize;
        let notes: Vec<_> = all.into_iter().skip(offset).take(limit).collect();
        let next = offset + notes.len();
        Ok(ListNotesOutput {
            notes,
            total,
            next_offset: (next < total as usize).then_some(next as u32),
        })
    }

    fn create_note(&self, ctx: &Ctx, input: CreateNoteInput) -> Result<NoteView> {
        self.fresh();
        let vault = self.vault(input.vault.as_deref())?;
        let folder = input
            .folder
            .as_deref()
            .map(|f| f.trim().trim_matches('/'))
            .filter(|f| !f.is_empty())
            .unwrap_or(layout::INBOX)
            .to_string();
        check_folder(&folder)?;
        self.hq.note_create(&NoteInput {
            title: input.title,
            vault: Some(vault.id),
            folder: Some(folder),
            template: input.template,
            note_type: input.note_type,
            tags: (!input.tags.is_empty()).then_some(input.tags),
            body: input.body,
            author: Some(ctx.author.clone()),
        })
    }

    fn append_to_note(&self, _ctx: &Ctx, input: AppendInput) -> Result<WriteOutput> {
        let (vault, rel) = self.hq.locate(&input.id)?;
        check_folder(notes::folder_of(&rel))?;
        let path = notes::note_path(&vault, &rel)?;
        let today = util::today();
        let hash = locks::guarded_edit(&path, Some(&input.base_hash), |cur| {
            let next = insert_text(cur, input.heading.as_deref(), &input.text);
            Ok(frontmatter::bump_updated(&next, &today))
        })?;
        self.hq.invalidate(Some(&vault.id));
        Ok(WriteOutput { id: input.id, hash })
    }

    fn capture(&self, ctx: &Ctx, input: CaptureInput) -> Result<LineOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let text = one_line(&input.text);
        if text.is_empty() {
            return Err(Error::Invalid("Nothing to capture.".into()));
        }
        // A's capture note and header; the line is signed like the daily Agent log
        let today = util::today();
        let rel = layout::capture_rel(&today);
        let path = vault.resolve_rel(&rel)?;
        let line = format!("- {} {} · {text}", util::now_hhmm(), ctx.author);
        capture::append_line(&path, || capture::header(&today), &line, &today)?;
        self.hq.invalidate(Some(&vault.id));
        Ok(LineOutput {
            id: hq_core::types::note_id(&vault.id, &rel),
            line,
        })
    }

    fn log(&self, ctx: &Ctx, input: LogInput) -> Result<LineOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let today = util::today();
        let hhmm = util::now_hhmm();
        let text = one_line(&input.text);
        let id = days::log(&vault, &ctx.author, &text, &today, &hhmm, &today)?;
        self.hq.invalidate(Some(&vault.id));
        Ok(LineOutput {
            id,
            line: format!("- {hhmm} {} · {text}", ctx.author),
        })
    }

    fn add_task(&self, _ctx: &Ctx, input: AddTaskInput) -> Result<Task> {
        let vault = self.vault_id(input.vault.as_deref())?;
        let task = TaskInput {
            description: input.description,
            tags: input.tags,
            project: input.project,
            note: input.note,
            vault,
            priority: input.priority,
            start: input.start,
            scheduled: input.scheduled,
            due: input.due,
            recur: input.recur,
            depends: input.depends,
            waiting: input.waiting,
        };
        self.with_tasks(|c| mutate::add(c, &task))
    }

    fn update_task(&self, _ctx: &Ctx, input: UpdateTaskInput) -> Result<Task> {
        let mut patch = input.patch;
        let close = match input.status {
            Some(s @ (TaskStatus::Done | TaskStatus::Cancelled)) => Some(s),
            other => {
                if other.is_some() {
                    patch.status = other;
                }
                None
            }
        };
        self.with_tasks(|c| {
            let mut key = input.id.clone();
            let mut last = None;
            if patch != TaskPatch::default() {
                let t = mutate::update(c, &key, &patch)?;
                key = t.key.clone();
                last = Some(t);
            }
            if let Some(text) = &input.annotate {
                let t = mutate::annotate(c, &key, text)?;
                key = t.key.clone();
                last = Some(t);
            }
            match close {
                Some(TaskStatus::Done) => last = Some(mutate::done(c, &key)?),
                Some(_) => last = Some(mutate::cancel(c, &key)?),
                None => {}
            }
            last.ok_or_else(|| Error::Invalid("`patch` must change at least one field".into()))
        })
    }

    fn list_tasks(&self, _ctx: &Ctx, input: ListTasksInput) -> Result<TaskGroups> {
        let vaults = self.vaults()?;
        let mut q = input.query;
        q.vaults = q
            .vaults
            .iter()
            .map(|w| self.vault(Some(w)).map(|v| v.id))
            .collect::<Result<_>>()?;
        if q.limit.is_none() {
            q.limit = Some(50);
        }
        query::run(&TaskIndex::build(&vaults), &q)
    }

    fn add_bookmark(&self, ctx: &Ctx, input: AddBookmarkInput) -> Result<BookmarkOutput> {
        let vault = self.vault(input.vault.as_deref())?;
        let (mut bookmark, created) = store::add(&vault, &input.url, input.note.as_deref())?;
        if created {
            // the store writes `author: hq`; this note was the agent's doing
            let author = ctx.author.clone();
            bookmark = store::update(&vault, &bookmark.id, |t| {
                frontmatter::set(t, "author", &author, false)
            })?;
            if self.fetch_pages && !ctx.is_cancelled() {
                // the app's one request with its timeout; offline just keeps the URL as title
                if let Ok(meta) = enrich::fetch_page(&bookmark.url) {
                    if let Ok(b) = enrich::apply_page(&vault, &bookmark.id, &meta) {
                        bookmark = b;
                    }
                }
            }
        }
        self.hq.invalidate(Some(&vault.id));
        Ok(BookmarkOutput { bookmark, created })
    }

    fn vault_info(&self, ctx: &Ctx) -> Result<VaultInfoOutput> {
        self.fresh();
        let mut vaults = Vec::new();
        for v in self.hq.vaults() {
            ctx.check()?;
            let root = Path::new(&v.path);
            let mut folders: Vec<String> = std::fs::read_dir(root)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect();
            folders.sort();
            vaults.push(VaultSummary {
                has_agents_md: root.join("AGENTS.md").is_file(),
                id: v.id,
                name: v.name,
                path: v.path,
                primary: v.primary,
                notes: v.notes,
                unreadable: v.unreadable,
                folders,
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
        let (text, _) = locks::read(&path)?;
        Ok(ResourceText {
            mime_type: "text/markdown".into(),
            text,
        })
    }

    fn note_resource(&self, _ctx: &Ctx, id: &str) -> Result<ResourceText> {
        if split_note_id(id).is_none() {
            return Err(Error::NotFound(format!("\"{id}\" is not a note id")));
        }
        let (_, rel) = self.hq.locate(id)?;
        check_folder(notes::folder_of(&rel))?;
        Ok(ResourceText {
            mime_type: "text/markdown".into(),
            text: self.hq.note_source(id)?.text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            insert_text(t, Some("Agent log"), "- x"),
            "## Work\n\n## Agent log\n- x\n"
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
    fn folders() {
        assert!(check_folder("01-Projects/sub").is_ok());
        assert!(check_folder(".vault-hq").is_err());
        assert!(check_folder("a/.git").is_err());
        assert!(check_folder("_Templates").is_err());
    }
}
