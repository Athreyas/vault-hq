// The Ask view's state, kept free of React and Tauri so it can be tested on its own.
import type { ActionAdded } from "../bindings/ActionAdded.ts";
import type { ActionDraft } from "../bindings/ActionDraft.ts";
import type { AskEnvelope } from "../bindings/AskEnvelope.ts";
import type { Conversation } from "../bindings/Conversation.ts";
import type { SearchHit } from "../bindings/SearchHit.ts";
import type { SearchScope } from "../bindings/SearchScope.ts";

export interface UiTurn {
  key: string;
  /** `action`: the text was an action item; the turn is a task card to confirm. */
  mode: "ask" | "search" | "action";
  question: string;
  scope: SearchScope;
  /** Streamed text while the model writes; the tidied answer once done. */
  answer: string;
  sources: SearchHit[];
  /** Stems cited, for turns loaded from a saved conversation (no full hits). */
  cited: string[];
  semantic: boolean | null;
  streaming: boolean;
  stopped: boolean;
  error: string | null;
  model: string | null;
  /** The running request, while streaming. */
  reqId: string | null;
  at: string | null;
  /** The task read from the text, while the user edits it. */
  action: ActionDraft | null;
  /** What was written once they confirmed. */
  added: ActionAdded | null;
}

let seq = 0;
export const newKey = () => `t${Date.now()}-${seq++}`;

export function newTurn(mode: UiTurn["mode"], question: string, scope: SearchScope): UiTurn {
  return {
    key: newKey(),
    mode,
    question,
    scope,
    answer: "",
    sources: [],
    cited: [],
    semantic: null,
    streaming: true,
    stopped: false,
    error: null,
    model: null,
    reqId: null,
    at: null,
    action: null,
    added: null,
  };
}

/** A draft as the user can send it back: ticked tags and links only. */
export function draftReady(d: ActionDraft): string | null {
  if (!d.description.trim()) return "The task needs a description.";
  if (d.dueTime && !d.due) return "A time needs a day.";
  return null;
}

/** `Fri 11-06 09:00`, or "no date". */
export function whenLabel(due: string | null, time: string | null): string {
  if (!due) return "no date";
  const [y, m, d] = due.split("-").map(Number);
  const day = new Date(y!, m! - 1, d!).toLocaleDateString(undefined, { weekday: "short" });
  return `${day} ${due.slice(5)}${time ? ` ${time}` : ""}`;
}

/** Fold one `ask://event` into its turn. */
export function applyEvent(t: UiTurn, e: AskEnvelope): UiTurn {
  switch (e.kind) {
    case "sources":
      return { ...t, sources: e.sources, semantic: e.semantic };
    case "token":
      return { ...t, answer: t.answer + e.text };
    case "done":
      return { ...t, answer: e.answer, model: e.model, stopped: e.stopped, streaming: false, reqId: null };
    case "error":
      return { ...t, error: e.message, streaming: false, reqId: null };
  }
}

/** A saved conversation as turns the view can show. */
export function fromConversation(c: Conversation): UiTurn[] {
  return c.turns.map((x) => ({
    ...newTurn("ask", x.question, { vaults: [], kinds: "both" }),
    answer: x.answer,
    cited: x.sources,
    model: x.model,
    streaming: false,
    at: x.at,
  }));
}

export const DAY_SPANS = [
  { days: 0, label: "all time" },
  { days: 1, label: "one day" },
  { days: 3, label: "3 days" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
] as const;

export function scopeLabel(s: SearchScope, vaultNames: Map<string, string> = new Map()): string {
  const parts: string[] = [];
  parts.push(s.kinds === "both" ? "notes + bookmarks" : s.kinds);
  if (s.days) parts.push(`${s.days === 1 ? "one day" : `${s.days} days`}${s.until ? ` to ${s.until}` : ""}`);
  else parts.push("all time");
  if (s.vaults.length) parts.push(s.vaults.map((v) => vaultNames.get(v) ?? v.slice(0, 6)).join(", "));
  return parts.join(" · ");
}
