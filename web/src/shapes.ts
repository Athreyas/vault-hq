// Shapes of commands whose Rust types have not landed in web/src/bindings yet. Each is
// the UI's best reading of SPEC 10; when a stream exports the real type from
// hq-core/src/types.rs, delete it here and re-export the binding from api.ts instead.
// Components never import this file: they import types from api.ts, so the swap is
// one line per type.

export type NoteId = string; // "<vaultId>:<vault-relative path>"

// ---- A core: home, days, needs, inbox, notes, capture -------------------------------

export interface Card {
  id: string;
  kind: string;
  kicker: string;
  title: string;
  lines: string[];
  progress: number | null;
  action: { label: string; href: string } | null;
}

export interface Home {
  greeting: string;
  cards: Card[];
  suggestions: string[];
}

export interface AgentLogLine {
  time: string | null;
  who?: string;
  text: string;
}

export interface Day {
  date: string;
  /** The daily note, when it exists. */
  note: NoteId | null;
  metrics: Record<string, number | string>;
  human: { heading: string; lines: string[] }[];
  agentLog: AgentLogLine[];
  completed: { text: string; note: NoteId | null }[];
  captures: { id: NoteId; title: string }[];
  dueTasks: number;
}

export interface Need {
  id: string;
  kind: string;
  label: string;
  title: string;
  detail: string;
  evidence: { note: NoteId | null; excerpt: string; when: string | null };
  outcomes: { key: string; label: string; hint: string }[];
}

export interface InboxItem {
  id: NoteId;
  title: string;
  kind: string;
  created: string;
  ageDays: number;
  author: string;
}

export interface NoteSummary {
  id: NoteId;
  vault: string;
  rel: string;
  title: string;
  folder: string;
  updated: string;
  type?: string;
}

export interface NotesFilter {
  vault?: string;
  folder?: string;
  type?: string;
  text?: string;
}

export interface NoteView extends NoteSummary {
  props: [string, string][];
  body: string;
  backlinks: NoteSummary[];
}

export interface SaveResult {
  id: NoteId;
  hash: string;
}

export interface Template {
  id: string;
  name: string;
  vault: string;
}

export interface NoteCreateInput {
  title: string;
  vault?: string;
  folder?: string;
  template?: string;
  body?: string;
}

export type CaptureKind = "note" | "task" | "bookmark";

export interface VaultCandidate {
  path: string;
  name: string;
}

// ---- B tasks ---------------------------------------------------------------------------

export type Priority = "highest" | "high" | "medium" | "low" | "lowest";

export interface Task {
  id: string;
  description: string;
  status: string;
  priority: Priority | null;
  due: string | null;
  project: string | null;
  note: NoteId;
  urgency: number;
  tags: string[];
}

export interface TaskQuery {
  status?: string[];
  text?: string;
  group?: string;
  sort?: string;
}

export interface TaskGroups {
  groups: { key: string; label: string; tasks: Task[] }[];
}

export interface TaskInput {
  description: string;
  project?: string;
  due?: string;
  priority?: Priority;
  vault?: string;
}

export type TaskPatch = Partial<Omit<TaskInput, "vault">>;

// ---- C search and local AI -------------------------------------------------------------

export interface SearchScope {
  vaults?: string[];
  /** A window of days ending on `until` (default today); absent = all time. */
  days?: number;
  until?: string;
  kinds?: ("notes" | "bookmarks")[];
}

export interface SearchHit {
  note: NoteId;
  title: string;
  heading: string;
  excerpt: string;
  score: number;
}

export interface AskEvent {
  id: string;
  kind: "sources" | "token" | "done" | "error";
  sources?: SearchHit[];
  token?: string;
  message?: string;
}

export interface ConversationSummary {
  id: string;
  title: string;
  updated: string;
}

export interface OllamaStatus {
  up: boolean;
  url: string;
  models: string[];
  chatReady: boolean;
  embedReady: boolean;
  binary: boolean;
}

export interface Recommendation {
  memoryGb: number;
  embed: string;
  chat: string[];
  pulls: string[];
}
