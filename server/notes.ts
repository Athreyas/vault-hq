// Notes for the in-app reader: a browsable list and one note with its links and backlinks.
// Read-only. A note is looked up only among the scanned vault notes (never by joining a
// path from the request), so a reference can't reach a file outside the vault.
import { obsidian } from "./config.ts";
import type { NoteSummary, NoteView } from "./types.ts";
import { notes, type Note } from "./vault.ts";

const title = (n: Note) => n.props.title || n.stem;
const folder = (rel: string) => (rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "");
const updated = (n: Note) => n.props.updated || new Date(n.mtime).toISOString().slice(0, 10);

// [[target]], [[target|alias]], [[target#heading]], ![[embed]]
const WIKI = /!?\[\[([^\]|#]+)(?:#[^\]|]*)?(?:\|[^\]]*)?\]\]/g;

// Templater files hold raw template code, not notes worth reading.
const HIDDEN = /^_templates\//;

export function noteList(): NoteSummary[] {
  return notes()
    .filter((n) => !HIDDEN.test(n.rel))
    .map((n) => ({ rel: n.rel, title: title(n), folder: folder(n.rel), updated: updated(n) }))
    .sort((a, b) => a.folder.localeCompare(b.folder) || a.title.localeCompare(b.title));
}

/** Resolve a reference the way Obsidian does: exact path, path without .md, then stem. */
export function resolve(ref: string): Note | null {
  const all = notes();
  const clean = ref.trim().replace(/^\/+/, "");
  const withMd = clean.endsWith(".md") ? clean : `${clean}.md`;
  const lower = withMd.toLowerCase();
  return (
    all.find((n) => n.rel === withMd) ??
    all.find((n) => n.rel.toLowerCase() === lower) ??
    all.find((n) => n.rel.toLowerCase().endsWith(`/${lower}`)) ??
    all.find((n) => n.stem.toLowerCase() === clean.replace(/\.md$/, "").toLowerCase().split("/").pop()) ??
    null
  );
}

export function noteView(ref: string): NoteView | null {
  const n = resolve(ref);
  if (!n) return null;
  const backlinks = notes()
    .filter((o) => o.rel !== n.rel && [...o.body.matchAll(WIKI)].some((m) => resolve(m[1] ?? "")?.rel === n.rel))
    .map((o) => ({ rel: o.rel, title: title(o), folder: folder(o.rel), updated: updated(o) }))
    .sort((a, b) => b.updated.localeCompare(a.updated));
  const props = Object.entries(n.props).filter(([k]) => k !== "title");
  // most notes open with "# <title>", which the reader already shows as its heading
  const lead = /^\s*# (.+)\n/.exec(n.body);
  const body = lead && lead[1]?.trim().toLowerCase() === title(n).trim().toLowerCase() ? n.body.slice(lead[0].length) : n.body;
  return { rel: n.rel, title: title(n), folder: folder(n.rel), updated: updated(n), props, body, backlinks, obsidian: obsidian(n.rel) };
}
