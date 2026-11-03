// Shapes of commands whose Rust types have not landed in web/src/bindings yet. Each is
// the UI's best reading of SPEC 10; when a stream exports the real type from
// hq-core/src/types.rs, delete it here and re-export the binding from api.ts instead.
// Components never import this file: they import types from api.ts, so the swap is
// one line per type.

export type NoteId = string; // "<vaultId>:<vault-relative path>"

// ---- A core: real types are in ./bindings (re-exported from api.ts) ------------------

// ---- B tasks: real types in web/src/bindings (Task, TaskQuery, …), re-exported by api.ts ----

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
