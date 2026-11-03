//! Types that cross to the UI. Each derives `TS` and is exported to web/src/bindings
//! (`cargo test -p hq-core export_bindings`). Streams add types only in their own section.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---- shared (Phase 0) -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub primary: bool,
    pub notes: u32,
    pub unreadable: u32,
}

/// `"<vaultId>:<vault-relative path>"`: how the UI and MCP name a note.
pub type NoteId = String;

pub fn note_id(vault: &str, rel: &str) -> NoteId {
    format!("{vault}:{rel}")
}

pub fn split_note_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':')
        .filter(|(v, r)| !v.is_empty() && !r.is_empty())
}

/// Text plus the hash an editor sends back with its save.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: NoteId,
    pub text: String,
    pub hash: String,
}

// ---- A core -----------------------------------------------------------------------------

// ---- B tasks ----------------------------------------------------------------------------

// ---- C search ---------------------------------------------------------------------------

// ---- E onboarding -----------------------------------------------------------------------

// ---- F bookmarks ------------------------------------------------------------------------

/// The file format behind a bookmark source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum BookmarkFormat {
    Chromium,
    Firefox,
    Safari,
    Netscape,
}

/// One browser profile (or an exported HTML file) HQ can read bookmarks from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSource {
    /// `<browser>:<profile folder>` (`chrome:Default`), `safari`, or `html:<path>`.
    /// The key in `Settings.browsers`.
    pub id: String,
    /// `chrome`, `brave`, `edge`, `arc`, `dia`, `vivaldi`, `chromium`, `firefox`, `safari`, `html`.
    pub browser: String,
    /// What the user sees: "Chrome · Default".
    pub label: String,
    pub profile: String,
    pub format: BookmarkFormat,
    pub path: String,
    pub allowed: bool,
    /// Bookmarks in the source; only known once the user allowed it.
    pub count: Option<u32>,
    /// macOS refused the read (Safari without Full Disk Access).
    pub needs_access: bool,
    /// What to do about a source HQ can't read, in steps the user can follow.
    pub guidance: Option<String>,
}

/// Where a link points, for its badge (Daymark idea).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Youtube,
    Github,
    X,
    Reddit,
    Hackernews,
    Instagram,
    Tiktok,
    Pdf,
    Article,
}

/// A bookmark note, as the UI lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: NoteId,
    pub title: String,
    pub url: String,
    pub source: String,
    pub folder: String,
    pub platform: Platform,
    /// `YYYY-MM-DD` the browser says it was saved (or the day it was pasted).
    pub added: String,
    pub tags: Vec<String>,
    /// The paragraph above `## Notes`: the page description or the enriched summary.
    pub summary: Option<String>,
    /// The user wrote something under `## Notes`.
    pub has_notes: bool,
    /// `removed-in-browser` once every browser that had it dropped it.
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct BookmarkFilter {
    pub source: Option<String>,
    pub tag: Option<String>,
    pub platform: Option<Platform>,
    /// Leave out bookmarks no browser has any more.
    pub hide_removed: bool,
    /// At most this many, newest first; 0 means 500.
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Facet {
    pub value: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkPage {
    pub items: Vec<Bookmark>,
    /// Matches before `limit`.
    pub total: u32,
    /// Counts over every bookmark, so the filters show what exists.
    pub sources: Vec<Facet>,
    pub tags: Vec<Facet>,
    pub platforms: Vec<Facet>,
}

/// What an import found and did (or, on a dry run, would do).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkImportReport {
    pub source_id: String,
    pub dry_run: bool,
    /// http(s) bookmarks in the source.
    pub found: u32,
    /// New notes.
    pub added: u32,
    /// Existing notes this source now also lists, or that came back after a removal.
    pub updated: u32,
    pub unchanged: u32,
    /// Notes newly marked `removed-in-browser`.
    pub removed: u32,
    /// The same URL more than once in this source.
    pub duplicates: u32,
    /// A few of the new ones, so a dry run shows what would land where.
    pub sample: Vec<BookmarkPreview>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkPreview {
    pub title: String,
    pub url: String,
    pub folder: String,
    pub rel: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EnrichReport {
    pub enriched: u32,
    pub skipped: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

/// A bookmark brought back on Home (Daymark's daily memory).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: NoteId,
    pub title: String,
    pub url: String,
    pub platform: Platform,
    pub source: String,
    pub added: String,
    pub age_days: u32,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub has_notes: bool,
}

// ---- W widgets --------------------------------------------------------------------------
