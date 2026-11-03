// The only place the UI talks to the app: Tauri `invoke` for commands, `listen` for
// events (SPEC 10). Everything is typed: generated bindings where a stream has exported
// its types, ./shapes.ts where it has not yet. A command that is not in this build
// comes back as an HqError of kind "Unavailable", so a view can say so and carry on.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { AppStatus } from "./bindings/AppStatus.ts";
import type { CaptureKind } from "./bindings/CaptureKind.ts";
import type { CaptureResult } from "./bindings/CaptureResult.ts";
import type { Card } from "./bindings/Card.ts";
import type { CardAction } from "./bindings/CardAction.ts";
import type { Day } from "./bindings/Day.ts";
import type { DecideResult } from "./bindings/DecideResult.ts";
import type { Home } from "./bindings/Home.ts";
import type { InboxItem } from "./bindings/InboxItem.ts";
import type { MoveResult } from "./bindings/MoveResult.ts";
import type { Need } from "./bindings/Need.ts";
import type { NoteFilter } from "./bindings/NoteFilter.ts";
import type { NoteInput } from "./bindings/NoteInput.ts";
import type { NoteSummary } from "./bindings/NoteSummary.ts";
import type { NoteView } from "./bindings/NoteView.ts";
import type { SaveResult } from "./bindings/SaveResult.ts";
import type { TemplateInfo } from "./bindings/TemplateInfo.ts";
import type { VaultCandidate } from "./bindings/VaultCandidate.ts";
import type { VaultLayout } from "./bindings/VaultLayout.ts";
import type { Current } from "./bindings/Current.ts";
import type { JobProgress } from "./bindings/JobProgress.ts";
import type { Settings } from "./bindings/Settings.ts";
import type { Priority } from "./bindings/Priority.ts";
import type { Source } from "./bindings/Source.ts";
import type { StatsQuery } from "./bindings/StatsQuery.ts";
import type { Task } from "./bindings/Task.ts";
import type { TaskGroups } from "./bindings/TaskGroups.ts";
import type { TaskImportReport } from "./bindings/TaskImportReport.ts";
import type { TaskInput } from "./bindings/TaskInput.ts";
import type { TaskPatch } from "./bindings/TaskPatch.ts";
import type { TaskQuery } from "./bindings/TaskQuery.ts";
import type { TaskStats } from "./bindings/TaskStats.ts";
import type { Theme } from "./bindings/Theme.ts";
import type { VaultChanged } from "./bindings/VaultChanged.ts";
import type { VaultInfo } from "./bindings/VaultInfo.ts";
import type * as S from "./shapes.ts";

export type { AppStatus, Current, JobProgress, Settings, Source, Theme, VaultChanged, VaultInfo };
export type { Priority, StatsQuery, Task, TaskGroups, TaskImportReport, TaskInput, TaskPatch, TaskQuery, TaskStats };
export type {
  CaptureKind,
  CaptureResult,
  Card,
  CardAction,
  Day,
  DecideResult,
  Home,
  InboxItem,
  MoveResult,
  Need,
  NoteFilter,
  NoteInput,
  NoteSummary,
  NoteView,
  SaveResult,
  TemplateInfo,
  VaultCandidate,
  VaultLayout,
};
/** Older names the views use for the same A types. */
export type NotesFilter = NoteFilter;
export type NoteCreateInput = NoteInput;
export type Template = TemplateInfo;
export type {
  AskEvent,
  ConversationSummary,
  NoteId,
  OllamaStatus,
  Recommendation,
  SearchHit,
  SearchScope,
} from "./shapes.ts";

// ---- errors ------------------------------------------------------------------------------

export type ErrorKind = "NotFound" | "Conflict" | "Invalid" | "Denied" | "Unavailable" | "Internal";
const KINDS: ErrorKind[] = ["NotFound", "Conflict", "Invalid", "Denied", "Unavailable", "Internal"];

export class HqError extends Error {
  readonly kind: ErrorKind;
  readonly current?: Current;
  constructor(kind: ErrorKind, message: string, current?: Current) {
    super(message);
    this.name = "HqError";
    this.kind = kind;
    this.current = current;
  }
}

/** A feature whose command is not in this build (another stream has not landed it). */
export const notBuilt = (e: unknown): boolean => e instanceof HqError && e.kind === "Unavailable";

/** Whatever `invoke` rejected with, as an HqError. Exported for tests. */
export function toHqError(raw: unknown, cmd: string): HqError {
  if (raw instanceof HqError) return raw;
  if (raw && typeof raw === "object" && "kind" in raw && "message" in raw) {
    const r = raw as { kind: unknown; message: unknown; current?: Current };
    const kind = KINDS.find((k) => k === r.kind) ?? "Internal";
    return new HqError(kind, String(r.message), r.current);
  }
  const message = typeof raw === "string" ? raw : raw instanceof Error ? raw.message : String(raw);
  // Tauri's own refusals for a command this build does not have
  if (cmd && /not found|not allowed by ACL/i.test(message) && message.includes(cmd)) {
    return new HqError("Unavailable", `${cmd} is not in this build of Vault HQ yet.`);
  }
  return new HqError("Internal", message);
}

const inApp = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ---- commands ----------------------------------------------------------------------------

type None = Record<string, never>;

/** SPEC 10 commands the UI calls: [arguments, result]. Argument keys are camelCase (Tauri's default). */
interface Commands {
  // app (shell)
  app_status: [None, AppStatus];
  settings_get: [None, Settings];
  settings_update: [{ patch: Record<string, unknown> }, Settings];
  reveal: [{ what: string }, null];
  capture_open: [None, null];
  capture_hide: [None, null];
  // vaults
  vaults_list: [None, VaultInfo[]];
  vault_add: [{ path: string }, VaultInfo];
  vault_create: [{ path: string; name: string; layout?: VaultLayout }, VaultInfo];
  vault_remove: [{ id: string }, VaultInfo[]];
  vault_set_primary: [{ id: string }, VaultInfo[]];
  vault_candidates: [None, VaultCandidate[]];
  // notes
  notes_list: [{ filter?: NoteFilter }, NoteSummary[]];
  /** `reference`: a note id, or a wiki-style target resolved in the primary vault first. */
  note_get: [{ reference: string }, NoteView];
  note_source: [{ id: string }, Source];
  /** A stale `base` rejects with an HqError of kind "Conflict" carrying `current`. */
  note_save: [{ id: string; text: string; base: string }, SaveResult];
  note_create: [{ input: NoteInput }, NoteView];
  /** `dryRun: true` lists the notes whose links would change, writing nothing. */
  note_move: [{ id: string; to: string; dryRun?: boolean }, MoveResult];
  note_archive: [{ id: string }, MoveResult];
  templates_list: [None, TemplateInfo[]];
  // capture
  capture: [{ text: string; kind?: CaptureKind; vault?: string }, CaptureResult];
  // home
  home: [{ date?: string }, Home];
  days: [{ from: string; span: number }, Day[]];
  needs: [None, Need[]];
  needs_decide: [{ id: string; outcome: string; why?: string }, DecideResult];
  inbox: [None, InboxItem[]];
  inbox_archive: [{ id: string }, MoveResult];
  // tasks
  tasks_query: [{ query: TaskQuery }, TaskGroups];
  task_add: [{ input: TaskInput }, Task];
  task_update: [{ id: string; patch: TaskPatch }, Task];
  task_done: [{ id: string }, Task];
  task_cancel: [{ id: string }, Task];
  task_delete: [{ id: string }, null];
  task_annotate: [{ id: string; text: string }, Task];
  task_start: [{ id: string }, Task];
  tasks_stats: [{ query: StatsQuery }, TaskStats];
  tasks_import_taskwarrior: [{ dryRun: boolean }, TaskImportReport];
  // search
  search: [{ q: string; scope: S.SearchScope }, S.SearchHit[]];
  ask_start: [{ question: string; scope: S.SearchScope; conversation?: string }, string];
  ask_cancel: [{ id: string }, null];
  conversations_list: [None, S.ConversationSummary[]];
  // ai
  ollama_status: [None, S.OllamaStatus];
  ollama_recommend: [None, S.Recommendation];
  // bookmarks
  bookmark_add: [{ url: string; note?: string }, unknown];
  // ---- streams: add the commands your views call below ----
}

export type CommandName = keyof Commands;

export async function call<K extends CommandName>(cmd: K, args: Commands[K][0]): Promise<Commands[K][1]> {
  if (!inApp()) throw new HqError("Unavailable", "Vault HQ's views run inside the app (npx tauri dev).");
  try {
    return await invoke<Commands[K][1]>(cmd, args as Record<string, unknown>);
  } catch (e) {
    throw toHqError(e, cmd);
  }
}

const none: None = {};

/** `base` with every field of `part` that is not undefined. */
function fill<T extends object>(base: T, part: Partial<T>): T {
  return Object.assign(base, Object.fromEntries(Object.entries(part).filter(([, v]) => v !== undefined)));
}

export function emptyTaskQuery(): TaskQuery {
  return {
    status: [],
    project: null,
    tagsAll: [],
    tagsAny: [],
    tagsNone: [],
    priority: [],
    noPriority: false,
    dueFrom: null,
    dueTo: null,
    scheduledFrom: null,
    scheduledTo: null,
    text: null,
    vaults: [],
    includeHidden: false,
    blocked: null,
    sort: "urgency",
    group: "none",
    limit: null,
    today: null,
  };
}

export function emptyTaskInput(): TaskInput {
  return {
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
}

/** Typed shorthands, so a view reads `api.home(date)`, not `call("home", { date })`. */
export const api = {
  status: () => call("app_status", none),
  settings: () => call("settings_get", none),
  updateSettings: (patch: Record<string, unknown>) => call("settings_update", { patch }),
  reveal: (what: string) => call("reveal", { what }),
  openCapture: () => call("capture_open", none),
  hideCapture: () => call("capture_hide", none),

  vaults: () => call("vaults_list", none),
  vaultAdd: (path: string) => call("vault_add", { path }),
  vaultCreate: (path: string, name: string, layout: VaultLayout = "default") => call("vault_create", { path, name, layout }),
  vaultRemove: (id: string) => call("vault_remove", { id }),
  vaultSetPrimary: (id: string) => call("vault_set_primary", { id }),
  vaultCandidates: () => call("vault_candidates", none),

  notes: (filter: NoteFilter = {}) => call("notes_list", { filter }),
  note: (reference: string) => call("note_get", { reference }),
  noteSource: (id: string) => call("note_source", { id }),
  noteSave: (id: string, text: string, base: string) => call("note_save", { id, text, base }),
  noteCreate: (input: NoteInput) => call("note_create", { input }),
  noteMove: (id: string, to: string, dryRun = false) => call("note_move", { id, to, dryRun }),
  noteArchive: (id: string) => call("note_archive", { id }),
  templates: () => call("templates_list", none),

  capture: (text: string, kind?: CaptureKind, vault?: string) => call("capture", { text, kind, vault }),

  home: (date?: string) => call("home", { date }),
  days: (from: string, span: number) => call("days", { from, span }),
  needs: () => call("needs", none),
  needsDecide: (id: string, outcome: string, why: string) => call("needs_decide", { id, outcome, why }),
  inbox: () => call("inbox", none),
  inboxArchive: (id: string) => call("inbox_archive", { id }),

  // the Rust types take every field; a view names only what it sets
  tasks: (query: Partial<TaskQuery> = {}) => call("tasks_query", { query: fill(emptyTaskQuery(), query) }),
  taskAdd: (input: Partial<TaskInput> & { description: string }) => call("task_add", { input: fill(emptyTaskInput(), input) }),
  taskUpdate: (id: string, patch: TaskPatch) => call("task_update", { id, patch }),
  taskDone: (id: string) => call("task_done", { id }),
  taskCancel: (id: string) => call("task_cancel", { id }),
  taskDelete: (id: string) => call("task_delete", { id }),
  taskAnnotate: (id: string, text: string) => call("task_annotate", { id, text }),
  /** Starts the task, or stops it when it is already started. */
  taskStart: (id: string) => call("task_start", { id }),
  tasksStats: (query: Partial<StatsQuery> = {}) => call("tasks_stats", { query: { days: null, project: null, tag: null, vaults: [], today: null, ...query } }),
  tasksImportTaskwarrior: (dryRun: boolean) => call("tasks_import_taskwarrior", { dryRun }),

  search: (q: string, scope: S.SearchScope = {}) => call("search", { q, scope }),
  askStart: (question: string, scope: S.SearchScope = {}, conversation?: string) => call("ask_start", { question, scope, conversation }),
  askCancel: (id: string) => call("ask_cancel", { id }),
  conversations: () => call("conversations_list", none),

  ollamaStatus: () => call("ollama_status", none),
  ollamaRecommend: () => call("ollama_recommend", none),

  bookmarkAdd: (url: string, note?: string) => call("bookmark_add", { url, note }),
};

// ---- events ------------------------------------------------------------------------------

interface Events {
  "vault://changed": VaultChanged;
  "ask://event": S.AskEvent;
  "jobs://progress": JobProgress;
  "capture://open": null;
}

/** Subscribe; returns the unsubscribe. Outside the app it is a no-op. */
export function on<K extends keyof Events>(event: K, fn: (payload: Events[K]) => void): () => void {
  if (!inApp()) return () => {};
  let off: (() => void) | null = null;
  let cancelled = false;
  listen<Events[K]>(event, (e) => fn(e.payload)).then(
    (u) => {
      if (cancelled) u();
      else off = u;
    },
    () => {},
  );
  return () => {
    cancelled = true;
    off?.();
  };
}

// ---- data hook ---------------------------------------------------------------------------

export interface Loaded<T> {
  data: T | null;
  error: HqError | null;
  reload: () => void;
}

/**
 * Run `load` now, every `refreshMs`, on `reload()`, and (unless `live` is false) shortly
 * after any vault changes on disk, so a note an agent just wrote shows up by itself.
 * `key` names what is loaded: a new key loads again.
 */
export function useCall<T>(load: () => Promise<T>, key: string, opts: { refreshMs?: number; live?: boolean } = {}): Loaded<T> {
  const { refreshMs = 0, live = true } = opts;
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<HqError | null>(null);
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((n) => n + 1), []);
  const loader = useRef(load);
  loader.current = load;

  useEffect(() => {
    let alive = true;
    loader.current().then(
      (d) => {
        if (!alive) return;
        setData(d);
        setError(null);
      },
      (e: unknown) => {
        if (alive) setError(toHqError(e, ""));
      },
    );
    return () => {
      alive = false;
    };
  }, [tick, key]);

  useEffect(() => {
    const timer = refreshMs ? window.setInterval(reload, refreshMs) : undefined;
    let debounce: number | undefined;
    const off = live
      ? on("vault://changed", () => {
          window.clearTimeout(debounce);
          debounce = window.setTimeout(reload, 250);
        })
      : () => {};
    return () => {
      if (timer) window.clearInterval(timer);
      window.clearTimeout(debounce);
      off();
    };
  }, [refreshMs, live, reload]);

  return { data, error, reload };
}

// ---- links and dates ---------------------------------------------------------------------

/** A note in HQ's reader. Takes a note id or a bare [[link]] target. */
export const noteUrl = (ref: string) => `#/notes/${encodeURIComponent(ref)}`;

/** Where a Home card's button goes (the core names a view, the UI owns the routes). */
export function cardHref(a: CardAction): string {
  if (a.view === "note" && a.arg) return noteUrl(a.arg);
  if (a.view === "day") return a.arg ? `#/days/${a.arg}` : "#/days";
  return `#/${a.view}`;
}

export function iso(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function addDays(date: string, n: number): string {
  const [y, m, d] = date.split("-").map(Number);
  return iso(new Date(y!, m! - 1, d! + n));
}

export function parseDate(date: string): Date {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y!, m! - 1, d!);
}

// ---- theme: kept in settings; mirrored in this window's storage so it paints right ----

const THEME_KEY = "hq.theme";

export function savedTheme(): Theme {
  try {
    return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

export function applyTheme(t: Theme): void {
  if (t === "light") document.documentElement.dataset.theme = "light";
  else delete document.documentElement.dataset.theme;
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {
    /* storage refused: the choice still lives in settings */
  }
}

/** Apply now and save to settings, so every window and the next launch agree. */
export async function setTheme(t: Theme): Promise<void> {
  applyTheme(t);
  await api.updateSettings({ theme: t });
}
