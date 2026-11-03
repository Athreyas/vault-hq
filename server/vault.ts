// Read-only view of the vault. One full scan is cached for a few seconds; the vault
// is ~150 small files, so a rescan is cheap and keeps every view consistent.
import fs from "node:fs";
import path from "node:path";
import { config, TASK_SCOPE_EXCLUDE } from "./config.ts";
import type { AgentLogLine, Day, InboxItem, Project, VaultTask } from "./types.ts";

export interface Note {
  rel: string;
  stem: string;
  props: Record<string, string>;
  body: string;
  mtime: number;
}

const CACHE_MS = 4000;
let cache: { at: number; notes: Note[]; unreadable: string[] } | null = null;

export function notes(): Note[] {
  return scan().notes;
}

/** Notes the last scan could not read (macOS refuses some iCloud reads to background
 *  processes; see README "Known behaviour"). Shown in the top bar, never silently dropped. */
export function unreadable(): string[] {
  return scan().unreadable;
}

function scan(): { notes: Note[]; unreadable: string[] } {
  if (cache && Date.now() - cache.at < CACHE_MS) return cache;
  const out: Note[] = [];
  const bad: string[] = [];
  const walk = (dir: string) => {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      bad.push(`${path.relative(config.vault, dir) || "."}/`);
      return;
    }
    for (const e of entries) {
      if (e.name.startsWith(".") || e.name === "_tools") continue;
      const full = path.join(dir, e.name);
      if (e.isDirectory()) walk(full);
      else if (e.name.endsWith(".md")) {
        try {
          out.push(readNote(full));
        } catch {
          bad.push(path.relative(config.vault, full)); // one unreadable note must not blank every view
        }
      }
    }
  };
  walk(config.vault);
  cache = { at: Date.now(), notes: out, unreadable: bad };
  return cache;
}

export function invalidate(): void {
  cache = null;
}

function readNote(full: string): Note {
  const text = fs.readFileSync(full, "utf8");
  const rel = path.relative(config.vault, full);
  const { props, body } = frontmatter(text);
  return { rel, stem: path.basename(rel, ".md"), props, body, mtime: fs.statSync(full).mtimeMs };
}

export function frontmatter(text: string): { props: Record<string, string>; body: string } {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(text);
  if (!m) return { props: {}, body: text };
  const props: Record<string, string> = {};
  for (const line of (m[1] ?? "").split("\n")) {
    const kv = /^([A-Za-z_]+):\s*(.*)$/.exec(line);
    if (kv?.[1]) props[kv[1]] = (kv[2] ?? "").replace(/^"(.*)"$/, "$1");
  }
  return { props, body: text.slice(m[0].length) };
}

// ---- tasks (Obsidian Tasks syntax, same grammar as task-sync.py) -----------------

const LINE = /^\s*- \[([ xX-])\] (.*)$/;
const PRIO: Record<string, VaultTask["prio"]> = { "🔺": "H", "⏫": "H", "🔼": "M", "🔽": "L", "⏬": "L" };
const FIELD = /(🔺|⏫|🔼|🔽|⏬)|(📅|⏳|🛫|✅|❌|➕|🆔|⛔)\s*([\w\-,]+)|(🔁)[^📅⏳🛫✅❌➕🆔⛔]*/gu;

export function parseTask(line: string, note: string): VaultTask | null {
  const m = LINE.exec(line);
  if (!m) return null;
  const body = m[2] ?? "";
  let prio: VaultTask["prio"] = null;
  const fields: Record<string, string> = {};
  for (const f of body.matchAll(FIELD)) {
    if (f[1]) prio = PRIO[f[1]] ?? null;
    else if (f[2] && f[3]) fields[f[2]] = f[3];
  }
  const desc = body.replace(FIELD, " ").replace(/\s+/g, " ").trim();
  if (!desc) return null;
  const mark = m[1] ?? " ";
  return {
    id: fields["🆔"] ?? null,
    desc,
    prio,
    due: fields["📅"] ?? null,
    done: fields["✅"] ?? null,
    status: mark === " " ? "pending" : mark === "-" ? "deleted" : "completed",
    note,
  };
}

export function tasks(includeOutOfScope = false): VaultTask[] {
  const out: VaultTask[] = [];
  for (const n of notes()) {
    if (!includeOutOfScope && TASK_SCOPE_EXCLUDE.some((p) => n.rel.startsWith(p))) continue;
    for (const line of n.body.split("\n")) {
      const t = parseTask(line, n.rel);
      if (t) out.push(t);
    }
  }
  return out;
}

// ---- days -------------------------------------------------------------------------

export function dailyRel(date: string): string {
  return `09-Planning/Daily/${date}.md`;
}

function sections(body: string): Map<string, string[]> {
  const out = new Map<string, string[]>();
  let current: string | null = null;
  for (const line of body.split("\n")) {
    const h = /^## (.+)$/.exec(line);
    if (h?.[1]) {
      current = h[1].trim();
      out.set(current, []);
    } else if (current) {
      out.get(current)?.push(line);
    }
  }
  return out;
}

const HUMAN_SECTIONS = ["Quick Wins", "Work", "Learning", "Tomorrow"];

function agentLog(lines: string[]): AgentLogLine[] {
  return lines
    .filter((l) => l.startsWith("- "))
    .map((l) => {
      const text = l.slice(2).trim();
      const time = /^(\d{1,2}:\d{2})\b/.exec(text)?.[1] ?? null;
      const links = [...text.matchAll(/\[\[([^\]|#]+)/g)].map((m) => m[1] ?? "");
      return { time, text: time ? text.slice(time.length).trim() : text, links };
    });
}

export function day(date: string): Day {
  const all = notes();
  const note = all.find((n) => n.rel === dailyRel(date));
  const secs = note ? sections(note.body) : new Map<string, string[]>();
  const metrics: Day["metrics"] = {};
  for (const [k, v] of Object.entries(note?.props ?? {})) {
    // metrics are the only underscored properties a daily note has
    if (k.includes("_")) {
      metrics[k] = /^-?\d+(\.\d+)?$/.test(v) ? Number(v) : v;
    }
  }
  const allTasks = tasks(true);
  return {
    date,
    exists: Boolean(note),
    metrics,
    human: HUMAN_SECTIONS.map((heading) => ({
      heading,
      lines: (secs.get(heading) ?? []).map((l) => l.replace(/^- ?/, "").trim()).filter(Boolean),
    })),
    agentLog: agentLog(secs.get("Agent log") ?? []),
    completed: allTasks.filter((t) => t.done === date),
    captures: all
      .filter((n) => n.rel.startsWith("00-Inbox/") && path.basename(n.rel).startsWith(date))
      .map((n) => ({ rel: n.rel, title: n.props.title ?? n.stem })),
    dueTasks: allTasks.filter((t) => t.status === "pending" && t.due === date).length,
  };
}

// ---- projects, inbox, findings ---------------------------------------------------------

export function projects(): Project[] {
  const open = tasks().filter((t) => t.status === "pending");
  return notes()
    .filter((n) => n.rel.startsWith("01-Projects/") && n.stem !== "AGENTS")
    .map((n) => {
      const progress = /\*\*Progress:\*\*\s*(\d+)%/.exec(n.body)?.[1];
      const desc = sections(n.body).get("Description") ?? [];
      const summary = desc.join(" ").replace(/\s+/g, " ").replace(/\[\[([^\]|]+)(\|[^\]]+)?\]\]/g, "$1").trim();
      return {
        rel: n.rel,
        stem: n.stem,
        title: n.props.title ?? n.stem,
        status: n.props.status ?? "active",
        progress: progress ? Number(progress) : null,
        updated: n.props.updated ?? "",
        summary: summary.length > 220 ? `${summary.slice(0, 217).replace(/\s\S*$/, "")}…` : summary,
        openTasks: open.filter((t) => t.note === n.rel).sort(byUrgency),
      };
    });
}

const PRIO_RANK = { H: 0, M: 1, L: 2 } as const;
export function byUrgency(a: VaultTask, b: VaultTask): number {
  const pa = a.prio ? PRIO_RANK[a.prio] : 3;
  const pb = b.prio ? PRIO_RANK[b.prio] : 3;
  if (pa !== pb) return pa - pb;
  return (a.due ?? "9999").localeCompare(b.due ?? "9999");
}

export function inbox(today: string): InboxItem[] {
  return notes()
    .filter((n) => n.rel.startsWith("00-Inbox/") && n.stem !== "AGENTS")
    .map((n) => {
      const created = n.props.created ?? today;
      const kind: InboxItem["kind"] = n.stem.includes("-compact-")
        ? "compact"
        : n.stem.includes("-session-")
          ? "session"
          : n.stem.includes("-proposal-")
            ? "proposal"
            : "dump";
      return {
        rel: n.rel,
        title: n.props.title ?? n.stem,
        kind,
        created,
        ageDays: Math.max(0, Math.round((Date.parse(today) - Date.parse(created)) / 86_400_000)),
        author: n.props.author ?? "unknown",
      };
    })
    .sort((a, b) => b.created.localeCompare(a.created));
}

export function findings(): { rel: string; title: string; latest: string | null }[] {
  return notes()
    .filter((n) => n.rel.startsWith("05-References/") && n.stem.endsWith("-findings"))
    .map((n) => {
      const runs = [...n.body.matchAll(/^### (\d{4}-\d{2}-\d{2}.*)$/gm)].map((m) => m[1] ?? "");
      return { rel: n.rel, title: n.props.title ?? n.stem, latest: runs.at(-1) ?? null };
    });
}

export function excerpt(rel: string, max = 600): string {
  const n = notes().find((x) => x.rel === rel);
  if (!n) return "";
  const text = n.body
    .replace(/^\s*# .*\n+/, "")
    .replace(/^#{2,} /gm, "")
    .replace(/^(\s*)- \[ \] /gm, "$1○ ")
    .replace(/^(\s*)- \[[xX]\] /gm, "$1✓ ")
    .replace(/^(\s*)- \[-\] /gm, "$1✕ ")
    .replace(/\s*🆔\s*[\w-]+/gu, "")
    .replace(/\s*(🔺|⏫|🔼|🔽|⏬)/gu, "")
    .replace(/\s*📅\s*(\d{4}-\d{2}-\d{2})/gu, " (due $1)")
    .replace(/\s*✅\s*(\d{4}-\d{2}-\d{2})/gu, " (done $1)")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .trim();
  return text.length > max ? `${text.slice(0, max).replace(/\s\S*$/, "")}…` : text;
}
