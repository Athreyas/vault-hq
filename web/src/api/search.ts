// Search, Ask and local-AI calls into the Rust core (SPEC 10). The only file of the Ask
// view that talks to Tauri.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AskEnvelope } from "../bindings/AskEnvelope.ts";
import type { Conversation } from "../bindings/Conversation.ts";
import type { ConversationSummary } from "../bindings/ConversationSummary.ts";
import type { OllamaStatus } from "../bindings/OllamaStatus.ts";
import type { Recommendation } from "../bindings/Recommendation.ts";
import type { SearchResult } from "../bindings/SearchResult.ts";
import type { SearchScope } from "../bindings/SearchScope.ts";
import type { VaultInfo } from "../bindings/VaultInfo.ts";

export type { AskEnvelope, Conversation, ConversationSummary, OllamaStatus, Recommendation, SearchResult, SearchScope };

/** A command error (`HqError`) or anything else, as words for the user. */
export function errorText(e: unknown): string {
  if (e && typeof e === "object" && "message" in e && typeof e.message === "string") return e.message;
  return typeof e === "string" ? e : "Something went wrong.";
}

export const search = (q: string, scope: SearchScope) => invoke<SearchResult>("search", { q, scope });
export const conversationsList = () => invoke<ConversationSummary[]>("conversations_list");
export const conversationGet = (id: string) => invoke<Conversation>("conversation_get", { id });
export const ollamaStatus = () => invoke<OllamaStatus>("ollama_status");
export const ollamaRecommend = () => invoke<Recommendation>("ollama_recommend");
export const askCancel = (id: string) => invoke<boolean>("ask_cancel", { id });

/** Open vaults for the scope picker; empty if the vault commands aren't there yet. */
export const vaultsList = () => invoke<VaultInfo[]>("vaults_list").catch(() => [] as VaultInfo[]);

// ---- streaming ---------------------------------------------------------------------------
// One listener for every question. Events can arrive before `ask_start` has returned the
// id they carry, so events for ids nobody has claimed yet are held until someone does.

type Handler = (e: AskEnvelope) => void;
const handlers = new Map<string, Handler>();
const early = new Map<string, AskEnvelope[]>();
let listening: Promise<unknown> | null = null;

function ensureListener(): Promise<unknown> {
  listening ??= listen<AskEnvelope>("ask://event", ({ payload }) => {
    const h = handlers.get(payload.id);
    if (h) h(payload);
    else early.set(payload.id, [...(early.get(payload.id) ?? []), payload]);
    if (payload.kind === "done" || payload.kind === "error") {
      if (h) handlers.delete(payload.id);
    }
  });
  return listening;
}

/** Start a question; `onEvent` gets sources, tokens, then done or error. Resolves to its id. */
export async function askStart(question: string, scope: SearchScope, conversation: string | null, onEvent: Handler): Promise<string> {
  await ensureListener();
  const id = await invoke<string>("ask_start", { question, scope, conversation });
  const held = early.get(id) ?? [];
  early.delete(id);
  let finished = false;
  for (const e of held) {
    onEvent(e);
    finished ||= e.kind === "done" || e.kind === "error";
  }
  if (!finished) handlers.set(id, onEvent);
  return id;
}
