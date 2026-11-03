//! The import assistant's analysis of an existing vault (usually Obsidian's): what doesn't
//! fit the HQ format yet, and a plan of changes the user ticks. Reads only; writes nothing
//! (not even the marker), so analysing a vault the user then walks away from leaves no trace.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use walkdir::WalkDir;

use super::agents::FOLDERS;
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::types::{BrokenLink, ImportAnalysis, ImportPlan, KindCount, PlanAction, PlanItem, Prop};
use crate::util;
use crate::vault::{scan, Note, Vault};

/// Suggests tags for a note's text. Stream C's model client plugs in here; without one,
/// `heuristic_tags` is used.
pub type TagSuggester<'a> = &'a dyn Fn(&str) -> Result<Vec<String>>;

const REQUIRED: [&str; 5] = ["title", "type", "created", "updated", "author"];
const MAX_BROKEN: usize = 300;
/// Untagged notes sent to the suggester, at most: a model call per note adds up.
const MAX_SUGGEST: usize = 200;

/// Root files that belong at the root.
const ROOT_OK: &[&str] = &["AGENTS.md", "CLAUDE.md", "README.md"];
/// Folders that hold attachments, left where they are.
const ATTACHMENT_DIRS: &[&str] = &[
    "attachments",
    "assets",
    "files",
    "media",
    "images",
    "resources/attachments",
];

/// Where a familiar folder name belongs in the HQ layout.
fn known_target(name: &str) -> Option<&'static str> {
    // "1. Projects", "01 - Projects", "Projects" all mean the same folder
    let bare: String = name
        .trim_start_matches(|c: char| c.is_ascii_digit() || " .-_".contains(c))
        .to_lowercase();
    Some(match bare.as_str() {
        "inbox" => "00-Inbox",
        "projects" => "01-Projects",
        "areas" => "02-Areas",
        "learning" => "03-Learning",
        "personal" => "04-Personal",
        "references" | "reference" | "resources" => "05-References",
        "memory" => "06-Memory",
        "agents" => "07-Agents",
        "archive" | "archives" => "08-Archive",
        "planning" => "09-Planning",
        "daily" | "daily notes" | "dailies" | "journal" | "journals" => "09-Planning/Daily",
        "bookmarks" => "10-Bookmarks",
        "templates" => "_templates",
        _ => return None,
    })
}

fn is_standard(name: &str) -> bool {
    FOLDERS.iter().any(|(f, _)| *f == name) || name == "_templates"
}

fn skipped_dir(name: &str) -> bool {
    name.starts_with('.') || ["_tools", "_templates", "node_modules"].contains(&name)
}

pub fn analyze(root: &Path, suggest: Option<TagSuggester>) -> Result<ImportAnalysis> {
    if !root.is_dir() {
        return Err(Error::NotFound(format!("No folder at {}", root.display())));
    }
    // A read-only view: Vault::open would write the marker.
    let vault = Vault {
        id: String::new(),
        name: String::new(),
        root: root.to_path_buf(),
    };
    let scanned = scan(&vault);
    let files = all_files(root);

    let mut items = Vec::new();
    let missing_folders: Vec<String> = FOLDERS
        .iter()
        .map(|(f, _)| f.to_string())
        .filter(|f| !root.join(f).is_dir())
        .collect();
    if !missing_folders.is_empty() {
        items.push(PlanItem {
            id: "folders".into(),
            group: "folders".into(),
            summary: format!(
                "Add the standard folders ({}), AGENTS.md and templates. Nothing existing is changed.",
                missing_folders.len()
            ),
            ticked: true,
            reversible: true,
            action: PlanAction::CreateFolders {
                folders: missing_folders.clone(),
            },
        });
    }

    // folders at the top that HQ doesn't know
    let mut unknown_folders = Vec::new();
    let mut tops: Vec<String> = std::fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !skipped_dir(n) && !is_standard(n))
        .collect();
    tops.sort();
    for name in tops {
        if ATTACHMENT_DIRS.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        unknown_folders.push(name.clone());
        let (to, ticked, why) = match known_target(&name) {
            Some(t) => (t.to_string(), true, "the same folder in HQ's layout"),
            None => (
                format!("05-References/{name}"),
                false,
                "a guess: leave it if it is fine where it is",
            ),
        };
        items.push(PlanItem {
            id: format!("mv:{name}/"),
            group: "folders".into(),
            summary: format!("Move folder {name}/ to {to}/ ({why})"),
            ticked,
            reversible: true,
            action: PlanAction::Move {
                from: name.clone(),
                to,
            },
        });
    }

    let mut loose_root = Vec::new();
    let mut no_frontmatter = 0u32;
    let mut incomplete = 0u32;
    let mut untagged_notes: Vec<&Note> = Vec::new();
    let mut fm_items = Vec::new();
    for n in &scanned.notes {
        let has_fm = !n.props.is_empty();
        if !has_fm {
            no_frontmatter += 1;
        }
        let add = missing_props(n);
        if has_fm && !add.is_empty() {
            incomplete += 1;
        }
        if !add.is_empty() {
            let keys: Vec<&str> = add.iter().map(|p| p.key.as_str()).collect();
            fm_items.push(PlanItem {
                id: format!("fm:{}", n.rel),
                group: "frontmatter".into(),
                summary: if has_fm {
                    format!("{}: add {}", n.rel, keys.join(", "))
                } else {
                    format!("{}: add frontmatter ({})", n.rel, keys.join(", "))
                },
                ticked: true,
                reversible: true,
                action: PlanAction::AddFrontmatter {
                    rel: n.rel.clone(),
                    base_hash: n.hash.clone(),
                    add,
                },
            });
        }
        if n.props
            .get("tags")
            .is_none_or(|t| frontmatter::list(t).is_empty())
            && !inline_tags(&n.body).iter().any(|_| true)
        {
            untagged_notes.push(n);
        }
        if !n.rel.contains('/') && !ROOT_OK.contains(&n.rel.as_str()) {
            loose_root.push(n.rel.clone());
        }
    }
    items.extend(fm_items);

    for rel in &loose_root {
        let to = format!("00-Inbox/{rel}");
        items.push(PlanItem {
            id: format!("mv:{rel}"),
            group: "loose".into(),
            summary: format!("Move {rel} into 00-Inbox/ to be filed"),
            ticked: true,
            reversible: true,
            action: PlanAction::Move {
                from: rel.clone(),
                to,
            },
        });
    }

    let untagged = untagged_notes.len() as u32;
    for n in untagged_notes.iter().take(MAX_SUGGEST) {
        let text = excerpt(n);
        let tags = match suggest {
            Some(f) => f(&text).unwrap_or_else(|_| heuristic_tags(n)),
            None => heuristic_tags(n),
        };
        let tags: Vec<String> = clean_tags(tags);
        if tags.is_empty() {
            continue;
        }
        items.push(PlanItem {
            id: format!("tag:{}", n.rel),
            group: "tags".into(),
            summary: format!("{}: tag {}", n.rel, tags.join(", ")),
            // suggestions are guesses; the user opts in
            ticked: false,
            reversible: true,
            action: PlanAction::AddTags {
                rel: n.rel.clone(),
                base_hash: n.hash.clone(),
                tags,
            },
        });
    }

    let (broken, broken_count) = broken_links(&scanned.notes, &files);

    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    let mut attachments = 0u32;
    let mut attachment_bytes = 0u64;
    for (rel, size) in &files {
        if rel.ends_with(".md") {
            continue;
        }
        attachments += 1;
        attachment_bytes += size;
        let ext = Path::new(rel)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| "(none)".into());
        *kinds.entry(ext).or_default() += 1;
    }
    let mut attachment_kinds: Vec<KindCount> = kinds
        .into_iter()
        .map(|(kind, count)| KindCount { kind, count })
        .collect();
    attachment_kinds.sort_by(|a, b| b.count.cmp(&a.count).then(a.kind.cmp(&b.kind)));

    Ok(ImportAnalysis {
        root: root.to_string_lossy().into_owned(),
        notes: scanned.notes.len() as u32,
        no_frontmatter,
        incomplete_frontmatter: incomplete,
        loose_root,
        unknown_folders,
        broken_links: broken,
        broken_link_count: broken_count,
        untagged,
        attachments,
        attachment_bytes,
        attachment_kinds,
        unreadable: scanned.unreadable,
        missing_folders,
        plan: ImportPlan {
            root: root.to_string_lossy().into_owned(),
            items,
        },
    })
}

/// Every file under the root (skipping the same folders as the scan), with its size.
fn all_files(root: &Path) -> Vec<(String, u64)> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !skipped_dir(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .into_owned();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            Some((rel, size))
        })
        .collect()
}

fn file_date(mtime: i64) -> String {
    chrono::DateTime::from_timestamp(mtime, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(util::today)
}

/// A `YYYY-MM-DD` at the start of a file stem (daily notes, dated captures).
fn stem_date(stem: &str) -> Option<String> {
    let d = stem.get(..10)?;
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
    Some(d.to_string())
}

fn guess_type(rel: &str, stem: &str) -> &'static str {
    let lower = rel.to_lowercase();
    let first = lower.split('/').next().unwrap_or("");
    let target = known_target(first).unwrap_or(first);
    if stem.len() == 10 && stem_date(stem).is_some() {
        return "daily";
    }
    if stem.ends_with("-findings") {
        return "findings";
    }
    match target {
        t if t.starts_with("01-projects") || t == "01-Projects" => "project",
        t if t.starts_with("02-areas") || t == "02-Areas" => "area",
        t if t.starts_with("05-references") || t == "05-References" => "reference",
        t if t.starts_with("06-memory") || t == "06-Memory" => "memory",
        t if t.starts_with("07-agents") || t == "07-Agents" => "agent",
        _ => "note",
    }
}

/// YAML-ish values that would read wrong unquoted get quotes (the parser strips them).
fn yaml_value(s: &str) -> String {
    let risky = s.starts_with(|c: char| "#[]{}&*!|>'\"%@`-?,".contains(c))
        || s.contains(": ")
        || s.ends_with(':')
        || s.trim() != s;
    if risky {
        format!("\"{}\"", s.replace('"', "'"))
    } else {
        s.to_string()
    }
}

fn missing_props(n: &Note) -> Vec<Prop> {
    let created = stem_date(&n.stem).unwrap_or_else(|| file_date(n.mtime));
    let mut out = Vec::new();
    for key in REQUIRED {
        if n.props.get(key).is_some_and(|v| !v.is_empty()) {
            continue;
        }
        let value = match key {
            "title" => yaml_value(&n.stem),
            "type" => guess_type(&n.rel, &n.stem).to_string(),
            "created" => created.clone(),
            "updated" => file_date(n.mtime),
            _ => "human".to_string(),
        };
        out.push(Prop {
            key: key.into(),
            value,
        });
    }
    out
}

/// `#tag` and `#a/b` in the body, outside headings and code.
fn inline_tags(body: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut prev = ' ';
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '#' && (prev.is_whitespace() || prev == '(') {
                let tag: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_alphanumeric() || "-_/".contains(**c))
                    .collect();
                if !tag.is_empty() && !tag.chars().all(|c| c.is_ascii_digit()) {
                    out.insert(tag.to_lowercase());
                }
                i += 1 + tag.chars().count();
                prev = 'x';
                continue;
            }
            prev = c;
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// Without a model: the note's folder names, as tags.
pub fn heuristic_tags(n: &Note) -> Vec<String> {
    let mut parts: Vec<&str> = n.rel.split('/').collect();
    parts.pop();
    parts
        .into_iter()
        .filter(|p| !is_standard(p) && known_target(p).is_none())
        .map(|p| util::slug(p, 30))
        .filter(|t| t != "untitled")
        .collect()
}

fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tags.into_iter()
        .map(|t| {
            t.trim()
                .trim_start_matches('#')
                .to_lowercase()
                .replace(' ', "-")
        })
        .filter(|t| {
            !t.is_empty()
                && t.chars().all(|c| c.is_alphanumeric() || "-_/".contains(c))
                && seen.insert(t.clone())
        })
        .take(5)
        .collect()
}

fn excerpt(n: &Note) -> String {
    let body: String = n.body.chars().take(2000).collect();
    format!("{}\n\n{}", n.title(), body)
}

/// `[[target]]` and relative markdown links that resolve to nothing in the vault.
fn broken_links(notes: &[Note], files: &[(String, u64)]) -> (Vec<BrokenLink>, u32) {
    let rels: BTreeSet<String> = files.iter().map(|(r, _)| r.clone()).collect();
    let lower: BTreeSet<String> = rels.iter().map(|r| r.to_lowercase()).collect();
    let mut names: HashMap<String, ()> = HashMap::new();
    for r in &rels {
        let p = Path::new(r);
        if let Some(n) = p.file_name() {
            names.insert(n.to_string_lossy().to_lowercase(), ());
        }
        if r.ends_with(".md") {
            if let Some(s) = p.file_stem() {
                names.insert(s.to_string_lossy().to_lowercase(), ());
            }
        }
    }
    let resolves = |target: &str| {
        let t = target.trim().trim_start_matches('/');
        let tl = t.to_lowercase();
        let md = format!("{tl}.md");
        lower.contains(&tl)
            || lower.contains(&md)
            || lower
                .iter()
                .any(|r| r.ends_with(&format!("/{tl}")) || r.ends_with(&format!("/{md}")))
            || names.contains_key(&tl)
            || names.contains_key(&md)
    };
    let mut out = Vec::new();
    let mut count = 0u32;
    for n in notes {
        let dir = Path::new(&n.rel)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        for (target, relative) in links(&n.body) {
            let ok = if relative {
                let joined = normalize(&dir, &target);
                joined.as_deref().is_some_and(|j| {
                    lower.contains(&j.to_lowercase())
                        || lower.contains(&format!("{}.md", j.to_lowercase()))
                })
            } else {
                resolves(&target)
            };
            if !ok && seen.insert(target.clone()) {
                count += 1;
                if out.len() < MAX_BROKEN {
                    out.push(BrokenLink {
                        from: n.rel.clone(),
                        target,
                    });
                }
            }
        }
    }
    (out, count)
}

/// `dir/../x` → `x`; None when it would leave the vault.
fn normalize(dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in target.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    Some(parts.join("/"))
}

/// Link targets in a note body: `(target, is_relative_markdown_link)`.
fn links(body: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut rest = line;
        while let Some(i) = rest.find("[[") {
            let after = &rest[i + 2..];
            let Some(j) = after.find("]]") else { break };
            let inner = &after[..j];
            let target = inner
                .split('|')
                .next()
                .unwrap_or("")
                .split('#')
                .next()
                .unwrap_or("")
                .trim();
            if !target.is_empty() {
                out.push((target.to_string(), false));
            }
            rest = &after[j + 2..];
        }
        let mut rest = line;
        while let Some(i) = rest.find("](") {
            let after = &rest[i + 2..];
            let Some(j) = after.find(')') else { break };
            let raw = after[..j].split_whitespace().next().unwrap_or("");
            let target = raw.split('#').next().unwrap_or("").replace("%20", " ");
            let external = raw.contains("://")
                || raw.starts_with("mailto:")
                || raw.starts_with('#')
                || raw.starts_with("obsidian:");
            if !external && !target.is_empty() {
                out.push((target, true));
            }
            rest = &after[j + 1..];
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A deliberately messy Obsidian-style vault.
    pub fn messy(root: &Path) {
        let w = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w(".obsidian/app.json", "{}");
        w(
            "Untitled.md",
            "a loose note linking [[Garden]] and [[Nowhere]]\n",
        );
        w("README.md", "# readme stays\n");
        w(
            "Projects/Garden.md",
            "---\ntitle: Garden\ntags: [outdoor]\n---\n# Garden\n![[photo.png]] [rel](../Notes/Ideas.md) [web](https://x.y)\n",
        );
        w(
            "Projects/Shed.md",
            "---\ntitle: Shed\ntype: project\n---\nsee [[Garden#Plan|plan]]\n",
        );
        w(
            "Notes/Ideas.md",
            "Some #idea here and #42 not a tag\n```\n#nope [[Ghost]]\n```\n",
        );
        w(
            "Notes/Cooking/Bread.md",
            "no frontmatter at all, [[Missing Recipe]]\n",
        );
        w("Daily Notes/2026-10-01.md", "## Work\n- [ ] a task\n");
        w("attachments/photo.png", "png-bytes");
        w("attachments/doc.pdf", "pdf");
        w("Notes/stray.jpg", "jpg");
        w("_templates/t.md", "{{title}}");
    }

    #[test]
    fn finds_what_does_not_fit() {
        let dir = tempfile::tempdir().unwrap();
        messy(dir.path());
        let a = analyze(dir.path(), None).unwrap();
        assert!(
            !dir.path().join(".vault-hq").exists(),
            "analysis writes nothing"
        );
        assert_eq!(a.notes, 7);
        assert_eq!(a.no_frontmatter, 5); // Untitled, README, Ideas, Bread, daily
        assert_eq!(a.incomplete_frontmatter, 2);
        assert_eq!(a.loose_root, vec!["Untitled.md"]);
        assert_eq!(a.unknown_folders, vec!["Daily Notes", "Notes", "Projects"]);
        let targets: Vec<&str> = a.broken_links.iter().map(|b| b.target.as_str()).collect();
        assert_eq!(
            targets,
            vec!["Missing Recipe", "Nowhere"],
            "{:?}",
            a.broken_links
        );
        assert_eq!(a.broken_link_count, 2);
        assert_eq!(a.attachments, 3);
        assert_eq!(a.missing_folders.len(), 11);
        // Garden has tags; Ideas has an inline tag
        assert_eq!(a.untagged, 5);

        let item = |id: &str| a.plan.items.iter().find(|i| i.id == id).cloned();
        assert!(item("folders").unwrap().ticked);
        match item("mv:Projects/").unwrap().action {
            PlanAction::Move { to, .. } => assert_eq!(to, "01-Projects"),
            _ => panic!(),
        }
        match item("mv:Daily Notes/").unwrap().action {
            PlanAction::Move { to, .. } => assert_eq!(to, "09-Planning/Daily"),
            _ => panic!(),
        }
        let notes = item("mv:Notes/").unwrap();
        assert!(!notes.ticked, "unknown folders are only a suggestion");
        assert!(item("mv:Untitled.md").unwrap().ticked);
        assert!(item("mv:README.md").is_none());
        match item("fm:Daily Notes/2026-10-01.md").unwrap().action {
            PlanAction::AddFrontmatter { add, .. } => {
                let get = |k: &str| add.iter().find(|p| p.key == k).unwrap().value.clone();
                assert_eq!(get("type"), "daily");
                assert_eq!(get("created"), "2026-10-01");
                assert_eq!(get("author"), "human");
            }
            _ => panic!(),
        }
        match item("fm:Projects/Shed.md").unwrap().action {
            PlanAction::AddFrontmatter { add, .. } => {
                let keys: Vec<&str> = add.iter().map(|p| p.key.as_str()).collect();
                assert_eq!(keys, vec!["created", "updated", "author"]);
            }
            _ => panic!(),
        }
        // heuristic tags from folder names; never ticked by default
        let bread = item("tag:Notes/Cooking/Bread.md").unwrap();
        assert!(!bread.ticked);
        match bread.action {
            PlanAction::AddTags { tags, .. } => assert_eq!(tags, vec!["notes", "cooking"]),
            _ => panic!(),
        }
    }

    #[test]
    fn suggester_is_used_and_its_failure_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        messy(dir.path());
        let model = |text: &str| -> Result<Vec<String>> {
            Ok(if text.contains("frontmatter at all") {
                vec!["#Baking".into(), "baking".into(), "bad tag!".into()]
            } else {
                vec![]
            })
        };
        let a = analyze(dir.path(), Some(&model)).unwrap();
        let bread = a
            .plan
            .items
            .iter()
            .find(|i| i.id == "tag:Notes/Cooking/Bread.md")
            .unwrap();
        match &bread.action {
            PlanAction::AddTags { tags, .. } => assert_eq!(tags, &vec!["baking".to_string()]),
            _ => panic!(),
        }
        let down = |_: &str| -> Result<Vec<String>> { Err(Error::Unavailable("no model".into())) };
        let a = analyze(dir.path(), Some(&down)).unwrap();
        assert!(a
            .plan
            .items
            .iter()
            .any(|i| i.id == "tag:Notes/Cooking/Bread.md"));
    }

    #[test]
    fn helpers() {
        assert_eq!(
            inline_tags("#h1 heading? no: x #a/b and (#c) #1"),
            vec!["a/b", "c", "h1"]
        );
        assert_eq!(known_target("01 - Projects"), Some("01-Projects"));
        assert_eq!(known_target("Journal"), Some("09-Planning/Daily"));
        assert_eq!(normalize("a/b", "../c.md").as_deref(), Some("a/c.md"));
        assert_eq!(normalize("", "../c.md"), None);
        assert_eq!(yaml_value("Plan: v2"), "\"Plan: v2\"");
        assert_eq!(yaml_value("#hash"), "\"#hash\"");
        assert_eq!(yaml_value("plain"), "plain");
        assert!(analyze(Path::new("/definitely/not/here"), None).is_err());
    }
}
