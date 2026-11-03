import { useCallback, useEffect, useState } from "react";
import type { Status } from "../../server/types.ts";
import { iso, setVaultName, useData } from "./api.ts";
import Ask from "./components/Ask.tsx";
import Days from "./components/Days.tsx";
import Hints from "./components/Hints.tsx";
import Home from "./components/Home.tsx";
import Inbox from "./components/Inbox.tsx";
import NeedsYou from "./components/NeedsYou.tsx";
import Notes from "./components/Notes.tsx";
import Palette from "./components/Palette.tsx";
import Settings from "./components/Settings.tsx";
import Tasks from "./components/Tasks.tsx";
import TopBar from "./components/TopBar.tsx";

export const VIEWS = ["home", "needs", "days", "tasks", "inbox", "notes", "ask"] as const;
export type View = (typeof VIEWS)[number];
export interface Route {
  view: View;
  date: string;
  span: 1 | 3 | 7;
  note: string | null; // notes view: a vault path or [[stem]]
}
export interface AskRequest {
  id: number;
  q: string;
  mode: "ask" | "search";
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function parseHash(): Route {
  const [, view = "home", date, span] = window.location.hash.replace(/^#/, "").split("/");
  const v = VIEWS.find((x) => x === view) ?? "home";
  const s = Number(span);
  let note: string | null = null;
  if (v === "notes" && date) {
    try {
      note = decodeURIComponent(date);
    } catch {
      note = date;
    }
  }
  return { view: v, date: date && DATE_RE.test(date) ? date : iso(new Date()), span: s === 3 || s === 7 ? s : 1, note };
}

export type Notify = (message: string, tone?: "ok" | "error") => void;

const typing = (t: EventTarget | null) => t instanceof HTMLElement && Boolean(t.closest("input, textarea, select, [contenteditable]"));

export default function App() {
  const [route, setRoute] = useState(parseHash);
  const [now, setNow] = useState(() => new Date());
  const [toast, setToast] = useState<{ message: string; tone: "ok" | "error" } | null>(null);
  const [palette, setPalette] = useState(false);
  const [settings, setSettings] = useState(false);
  const [askReq, setAskReq] = useState<AskRequest | null>(null);
  const status = useData<Status>("/api/status", 60_000);

  useEffect(() => {
    const onHash = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHash);
    const clock = setInterval(() => setNow(new Date()), 15_000);
    return () => {
      window.removeEventListener("hashchange", onHash);
      clearInterval(clock);
    };
  }, []);

  if (status.data) setVaultName(status.data.vault);

  const notify: Notify = useCallback((message, tone = "ok") => {
    setToast({ message, tone });
    window.setTimeout(() => setToast(null), 4200);
  }, []);
  const changed = status.reload;

  const go = useCallback((hash: string) => {
    window.location.hash = hash;
  }, []);

  const ask = useCallback(
    (q: string, mode: "ask" | "search" = "ask") => {
      setAskReq({ id: Date.now(), q, mode });
      go("#/ask");
    },
    [go],
  );

  // Global keys. Ctrl+K works everywhere, including inside inputs; the single-key
  // shortcuts only fire when no field has focus.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === ",") {
        e.preventDefault();
        setPalette(false);
        setSettings((s) => !s);
        return;
      }
      if (e.key === "Escape") {
        setPalette(false);
        if (typing(e.target)) (e.target as HTMLElement).blur();
        return;
      }
      if (palette || settings || typing(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
      const n = Number(e.key);
      if (n >= 1 && n <= VIEWS.length) go(`#/${VIEWS[n - 1]}`);
      else if (e.key === "/") {
        e.preventDefault();
        go("#/ask");
        window.setTimeout(() => document.getElementById("ask-q")?.focus(), 0);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, palette, settings]);

  return (
    <div className="app">
      <TopBar now={now} route={route} status={status.data} />
      <main className="stage" id="main">
        {route.view === "home" && <Home date={route.date} onAsk={ask} suggestions={status.data?.tips.suggestions ?? []} />}
        {route.view === "needs" && <NeedsYou notify={notify} onChange={changed} />}
        {route.view === "days" && <Days date={route.date} span={route.span} notify={notify} />}
        {route.view === "tasks" && <Tasks notify={notify} onChange={changed} />}
        {route.view === "inbox" && <Inbox notify={notify} onChange={changed} />}
        {route.view === "notes" && <Notes note={route.note} notify={notify} />}
        {route.view === "ask" && <Ask req={askReq} suggestions={status.data?.tips.suggestions ?? []} />}
      </main>
      <Hints view={route.view} status={status.data} />
      {palette && <Palette status={status.data} onClose={() => setPalette(false)} go={go} ask={ask} notify={notify} onSaved={changed} openSettings={() => setSettings(true)} />}
      {settings && <Settings onClose={() => setSettings(false)} notify={notify} />}
      <div className={`toast ${toast ? "is-shown" : ""} ${toast?.tone === "error" ? "is-error" : ""}`} role="status" aria-live="polite">
        {toast?.message}
      </div>
    </div>
  );
}
