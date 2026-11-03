import { useCallback, useEffect, useState } from "react";

export async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  const data = (await r.json()) as T & { error?: string };
  if (!r.ok) throw new Error(data.error ?? `Request failed (${r.status})`);
  return data;
}

export async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json", "X-HQ": "1" },
    body: JSON.stringify(body),
  });
  const data = (await r.json()) as T & { error?: string };
  if (!r.ok) throw new Error(data.error ?? `Request failed (${r.status})`);
  return data;
}

/** Fetch `path`, refetch every `refreshMs` and whenever `reload()` is called. */
export function useData<T>(path: string, refreshMs = 0) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    let live = true;
    get<T>(path)
      .then((d) => live && (setData(d), setError(null)))
      .catch((e: Error) => live && setError(e.message));
    const timer = refreshMs ? setInterval(() => setTick((n) => n + 1), refreshMs) : undefined;
    return () => {
      live = false;
      if (timer) clearInterval(timer);
    };
  }, [path, refreshMs, tick]);

  return { data, error, reload };
}

// Set once from /api/status: the vault this Mac's HQ serves.
let vaultName = "";
export function setVaultName(name: string): void {
  vaultName = name;
}

/** A note in HQ's own reader. Takes a vault path or a bare [[stem]]. */
export const noteUrl = (ref: string) => `#/notes/${encodeURIComponent(ref)}`;

export const obsidianUrl = (rel: string) =>
  `obsidian://open?vault=${encodeURIComponent(vaultName)}&file=${encodeURIComponent(rel.replace(/\.md$/, ""))}`;

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

// ---- ask history: finished exchanges kept in this browser so recent searches reopen instantly ----

export interface AskSource {
  rel: string;
  stem: string;
  heading: string;
  excerpt: string;
  score: number;
}

export interface Exchange {
  id: number;
  question: string;
  mode: "ask" | "search";
  scope: { span: number; date: string }; // span 0 = all notes
  answer: string | null;
  sources: AskSource[];
  error: string | null;
  done: boolean;
}

const ASK_KEY = "hq.ask.v1";
export const ASK_KEEP = 20;

export function loadAsks(): Exchange[] {
  try {
    const list = JSON.parse(localStorage.getItem(ASK_KEY) ?? "[]") as Exchange[];
    return list.filter((x) => x.done && !x.error).slice(0, ASK_KEEP);
  } catch {
    return [];
  }
}

export function saveAsks(list: Exchange[]): void {
  try {
    localStorage.setItem(ASK_KEY, JSON.stringify(list.filter((x) => x.done && !x.error).slice(0, ASK_KEEP)));
  } catch {
    /* private mode or full storage: history just won't persist */
  }
}

export function scopeLabel(s: Exchange["scope"]): string {
  return s.span === 0 ? "all notes" : s.span === 1 ? s.date : `${s.span}d to ${s.date}`;
}

// ---- theme: dark unless the user picked light; kept per browser ----

export type Theme = "dark" | "light";
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
    /* private mode: the choice lasts for this page only */
  }
}
