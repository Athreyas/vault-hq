// The tasks view's inline syntax, Taskwarrior-flavoured:
//   quick add / inline edit:  Fix the gate #home +garden due:fri p:h rec:weekly sched:+3d wait:mon dep:abc123
//   filter:                   +garden -travel project:home due:eow p:h status:done +OVERDUE +BLOCKED words
// Dates: today, tomorrow, yesterday, mon…sun (the next one), +3d / 2w / 1m / 1y, eow, eom, eoy, YYYY-MM-DD.
import type { Priority } from "../bindings/Priority.ts";
import type { Task } from "../bindings/Task.ts";
import type { TaskInput } from "../bindings/TaskInput.ts";
import type { TaskPatch } from "../bindings/TaskPatch.ts";
import type { TaskQuery } from "../bindings/TaskQuery.ts";
import type { TaskStatus } from "../bindings/TaskStatus.ts";

const DAY_NAMES = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

function iso(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function fromIso(s: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return iso(d) === s ? d : null;
}

export function addDays(date: string, n: number): string {
  const d = fromIso(date)!;
  return iso(new Date(d.getFullYear(), d.getMonth(), d.getDate() + n));
}

/** A date word relative to `today` (YYYY-MM-DD), or null if it isn't one. */
export function resolveDate(word: string, today: string): string | null {
  const w = word.trim().toLowerCase();
  const t = fromIso(today);
  if (!t || !w) return null;
  if (fromIso(w)) return w;
  const y = t.getFullYear();
  const m = t.getMonth();
  const d = t.getDate();
  switch (w) {
    case "today":
    case "tod":
    case "now":
      return today;
    case "tomorrow":
    case "tom":
      return addDays(today, 1);
    case "yesterday":
      return addDays(today, -1);
    case "eow": // Sunday ends the week, as in Taskwarrior
      return addDays(today, (7 - t.getDay()) % 7);
    case "eom":
      return iso(new Date(y, m + 1, 0));
    case "eoy":
      return iso(new Date(y, 11, 31));
  }
  // a day name or any prefix of it from three letters ("fri", "thurs"): the next one after today
  const day = w.length >= 3 ? DAY_NAMES.findIndex((n) => n.startsWith(w)) : -1;
  if (day >= 0) {
    const ahead = (day - t.getDay() + 7) % 7 || 7;
    return addDays(today, ahead);
  }
  const rel = /^\+?(\d{1,4})(d|w|m|y)$/.exec(w);
  if (rel) {
    const n = Number(rel[1]);
    switch (rel[2]) {
      case "d":
        return addDays(today, n);
      case "w":
        return addDays(today, 7 * n);
      case "m": {
        // clamp to the month's last day, like the engine does
        const last = new Date(y, m + n + 1, 0).getDate();
        return iso(new Date(y, m + n, Math.min(d, last)));
      }
      case "y": {
        const last = new Date(y + n, m + 1, 0).getDate();
        return iso(new Date(y + n, m, Math.min(d, last)));
      }
    }
  }
  return null;
}

const PRIORITY_WORDS: Record<string, Priority> = {
  hh: "highest",
  highest: "highest",
  h: "high",
  high: "high",
  m: "medium",
  med: "medium",
  medium: "medium",
  l: "low",
  low: "low",
  ll: "lowest",
  lowest: "lowest",
};

export function parsePriority(word: string): Priority | null | undefined {
  const w = word.toLowerCase();
  if (w === "" || w === "none" || w === "-") return null;
  return PRIORITY_WORDS[w];
}

/** Taskwarrior-style repeat words as `🔁` rules. */
export function resolveRecur(word: string): string | null {
  const w = word.trim().toLowerCase();
  const named: Record<string, string> = {
    daily: "every day",
    weekly: "every week",
    weekdays: "every weekday",
    weekday: "every weekday",
    biweekly: "every 2 weeks",
    fortnightly: "every 2 weeks",
    monthly: "every month",
    quarterly: "every 3 months",
    yearly: "every year",
    annual: "every year",
  };
  if (named[w]) return named[w];
  const m = /^(\d{1,3})(d|w|m|y)$/.exec(w);
  if (!m) return null;
  const unit = { d: "day", w: "week", m: "month", y: "year" }[m[2] as "d" | "w" | "m" | "y"];
  const n = Number(m[1]);
  if (n === 0) return null;
  return n === 1 ? `every ${unit}` : `every ${n} ${unit}s`;
}

const TAG_RE = /^#([\p{L}\p{N}_/-]*[\p{L}_/-][\p{L}\p{N}_/-]*)$/u;

export interface QuickParse {
  input: TaskInput;
  /** Tokens that looked like syntax but did not parse, e.g. `due:someday`. */
  errors: string[];
  /** Fields that were set explicitly, for an edit to tell "cleared" from "untouched". */
  set: Set<"priority" | "project" | "due" | "scheduled" | "start" | "recur" | "depends" | "tags">;
}

/** Parse a quick-add (or inline edit) line. Unknown words stay in the description. */
export function parseQuickAdd(text: string, today: string): QuickParse {
  const input: TaskInput = {
    description: "",
    tags: [],
    project: null,
    note: null,
    vault: null,
    priority: null,
    start: null,
    scheduled: null,
    due: null,
    recur: null,
    depends: [],
    waiting: false,
  };
  const errors: string[] = [];
  const set: QuickParse["set"] = new Set();
  const words: string[] = [];
  for (const tok of text.split(/\s+/).filter(Boolean)) {
    const tag = TAG_RE.exec(tok);
    if (tag) {
      if (!input.tags.includes(tag[1]!)) input.tags.push(tag[1]!);
      set.add("tags");
      continue;
    }
    if (/^\+[\p{L}\p{N}_./-]+$/u.test(tok) && !/^\+\d+[dwmy]$/.test(tok)) {
      input.project = tok.slice(1);
      set.add("project");
      continue;
    }
    const kv = /^([a-z]+):(.*)$/i.exec(tok);
    if (kv) {
      const key = kv[1]!.toLowerCase();
      const value = kv[2]!;
      const date = (field: "due" | "scheduled" | "start") => {
        if (value === "") {
          input[field] = null;
          set.add(field);
          return;
        }
        const d = resolveDate(value, today);
        if (d) {
          input[field] = d;
          set.add(field);
        } else errors.push(tok);
      };
      switch (key) {
        case "due":
          date("due");
          continue;
        case "sched":
        case "scheduled":
        case "sch":
          date("scheduled");
          continue;
        case "start":
          date("start");
          continue;
        case "wait": {
          const d = value === "" ? null : resolveDate(value, today);
          if (value !== "" && !d) errors.push(tok);
          else {
            input.waiting = d !== null;
            input.scheduled = d;
            set.add("scheduled");
          }
          continue;
        }
        case "p":
        case "pri":
        case "priority": {
          const p = parsePriority(value);
          if (p === undefined) errors.push(tok);
          else {
            input.priority = p;
            set.add("priority");
          }
          continue;
        }
        case "pro":
        case "proj":
        case "project":
          input.project = value || null;
          set.add("project");
          continue;
        case "rec":
        case "recur":
        case "repeat": {
          const r = value === "" ? null : resolveRecur(value);
          if (value !== "" && !r) errors.push(tok);
          else {
            input.recur = r;
            set.add("recur");
          }
          continue;
        }
        case "dep":
        case "depends":
          input.depends = value.split(",").filter((x) => /^[a-z0-9_-]+$/i.test(x));
          set.add("depends");
          continue;
      }
    }
    words.push(tok);
  }
  input.description = words.join(" ");
  return { input, errors, set };
}

/** A task written back in the quick-add syntax, for inline editing. */
export function taskToSyntax(t: Task): string {
  const parts = [t.description, ...t.tags.map((x) => `#${x}`)];
  if (t.project) parts.push(`+${t.project}`);
  const p = t.priority ? { highest: "hh", high: "h", medium: "m", low: "l", lowest: "ll" }[t.priority] : null;
  if (p) parts.push(`p:${p}`);
  if (t.start) parts.push(`start:${t.start}`);
  if (t.scheduled) parts.push(`${t.status === "waiting" ? "wait" : "sched"}:${t.scheduled}`);
  if (t.due) parts.push(`due:${t.due}`);
  if (t.recur) {
    const short = /^every (?:(\d+) )?(day|week|month|year)s?$/.exec(t.recur);
    if (short) parts.push(`rec:${short[1] ?? 1}${short[2]![0]}`);
    else if (t.recur === "every weekday") parts.push("rec:weekdays");
  }
  if (t.depends.length) parts.push(`dep:${t.depends.join(",")}`);
  return parts.join(" ");
}

const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/** What changed between a task and its edited syntax line: only differing fields. */
export function patchFrom(t: Task, edited: QuickParse): TaskPatch {
  const i = edited.input;
  const patch: TaskPatch = {};
  if (i.description && i.description !== t.description) patch.description = i.description;
  if (!same(i.tags, t.tags)) patch.tags = i.tags;
  if ((i.project ?? null) !== (t.project ?? null)) patch.project = i.project;
  if ((i.priority ?? null) !== (t.priority ?? null)) patch.priority = i.priority;
  if ((i.due ?? null) !== t.due) patch.due = i.due;
  if ((i.scheduled ?? null) !== t.scheduled) patch.scheduled = i.scheduled;
  if ((i.start ?? null) !== t.start) patch.start = i.start;
  // a rule the short syntax cannot express is left alone unless the edit names one
  const recurShown = t.recur === null || taskToSyntax(t).includes(" rec:");
  if (edited.set.has("recur") || recurShown) {
    if ((i.recur ?? null) !== t.recur) patch.recur = i.recur;
  }
  if (!same(i.depends, t.depends)) patch.depends = i.depends;
  if (i.waiting && t.status !== "waiting") patch.status = "waiting";
  if (!i.waiting && t.status === "waiting") patch.status = "pending";
  return patch;
}

const STATUSES: TaskStatus[] = ["pending", "started", "waiting", "done", "cancelled"];

/** Parse the filter bar into the query fields it sets; other words become the text search. */
export function parseFilter(text: string, today: string): { query: Partial<TaskQuery>; errors: string[] } {
  const q: Partial<TaskQuery> = {};
  const errors: string[] = [];
  const words: string[] = [];
  const push = (k: "tagsAll" | "tagsNone", v: string) => {
    q[k] = [...(q[k] ?? []), v];
  };
  for (const tok of text.split(/\s+/).filter(Boolean)) {
    const virt = /^([+-])([A-Z]+)$/.exec(tok);
    if (virt) {
      const on = virt[1] === "+";
      switch (virt[2]) {
        case "OVERDUE":
          q.dueTo = addDays(today, -1);
          continue;
        case "TODAY":
          q.dueFrom = today;
          q.dueTo = today;
          continue;
        case "WEEK":
          q.dueTo = addDays(today, 7);
          continue;
        case "BLOCKED":
          q.blocked = on;
          continue;
        case "WAITING":
          q.status = ["waiting"];
          q.includeHidden = true;
          continue;
      }
    }
    const tag = /^([+-]|#)([\p{L}\p{N}_/-]+)$/u.exec(tok);
    if (tag) {
      push(tag[1] === "-" ? "tagsNone" : "tagsAll", tag[2]!);
      continue;
    }
    const kv = /^([a-z.]+):(.+)$/i.exec(tok);
    if (kv) {
      const key = kv[1]!.toLowerCase();
      const value = kv[2]!;
      const date = (f: (d: string) => void) => {
        const d = resolveDate(value, today);
        if (d) f(d);
        else errors.push(tok);
      };
      switch (key) {
        case "pro":
        case "proj":
        case "project":
          q.project = value;
          continue;
        case "status": {
          const v = value.toLowerCase();
          if (v === "all") q.status = [...STATUSES];
          else if (v === "open") q.status = [];
          else {
            const list = v.split(",").filter((s): s is TaskStatus => (STATUSES as string[]).includes(s));
            if (list.length) q.status = list;
            else errors.push(tok);
          }
          continue;
        }
        case "due":
        case "due.by":
          date((d) => (q.dueTo = d));
          continue;
        case "due.before":
          date((d) => (q.dueTo = addDays(d, -1)));
          continue;
        case "due.after":
          date((d) => (q.dueFrom = addDays(d, 1)));
          continue;
        case "sched":
        case "scheduled":
          date((d) => (q.scheduledTo = d));
          continue;
        case "p":
        case "pri":
        case "priority": {
          const v = value.toLowerCase();
          if (v === "none") q.noPriority = true;
          else if (v === "h") q.priority = [...(q.priority ?? []), "highest", "high"];
          else if (v === "l") q.priority = [...(q.priority ?? []), "low", "lowest"];
          else {
            const p = parsePriority(v);
            if (p) q.priority = [...(q.priority ?? []), p];
            else errors.push(tok);
          }
          continue;
        }
      }
    }
    words.push(tok);
  }
  if (words.length) q.text = words.join(" ");
  return { query: q, errors };
}

/**
 * Display names for projects. A project is keyed by its note's stem ("garden-irrigation");
 * when a task lives in that note, the note's title ("Garden drip irrigation") is the name.
 * A `#project/x` with no note of its own keeps its key.
 */
export function projectTitles(tasks: Pick<Task, "project" | "note" | "noteTitle">[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const t of tasks) {
    if (!t.project || out.has(t.project) || !t.noteTitle) continue;
    const stem = t.note.replace(/^[^:]*:/, "").replace(/^.*\//, "").replace(/\.md$/i, "");
    if (stem === t.project) out.set(t.project, t.noteTitle);
  }
  return out;
}
