import { useCallback, useEffect, useState } from "react";
import { api, applyTheme, iso, useCall, type AppStatus } from "./api.ts";
import Ask from "./components/Ask.tsx";
import Bookmarks from "./components/Bookmarks.tsx";
import Create, { type CreateKind } from "./components/Create.tsx";
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
import { onboardingState, setStep } from "./api/onboarding.ts";
import Onboarding from "./components/onboarding/Onboarding.tsx";

// ---- routes: streams add a view name below and its element in <main> ----
export const VIEWS = ["home", "needs", "days", "tasks", "inbox", "notes", "ask", "bookmarks"] as const;
export type View = (typeof VIEWS)[number];
export interface Route {
  view: View;
  date: string;
  span: 1 | 3 | 7;
  note: string | null; // notes view: a note id or a [[link]] target
}
export interface AskRequest {
  id: number;
  q: string;
  mode: "ask" | "search";
}
export interface Counts {
  needs: number;
  inbox: number;
}
export interface CreateRequest {
  kind: CreateKind;
  text: string;
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

/** Badge counts; a view whose command is not in this build counts as zero. */
async function loadCounts(): Promise<Counts> {
  const [needs, inbox] = await Promise.allSettled([api.needs(), api.inbox()]);
  return {
    needs: needs.status === "fulfilled" ? needs.value.length : 0,
    inbox: inbox.status === "fulfilled" ? inbox.value.length : 0,
  };
}

export default function App() {
  const [route, setRoute] = useState(parseHash);
  const [now, setNow] = useState(() => new Date());
  const [toast, setToast] = useState<{ message: string; tone: "ok" | "error" } | null>(null);
  const [palette, setPalette] = useState(false);
  const [settings, setSettings] = useState(false);
  const [create, setCreate] = useState<CreateRequest | null>(null);
  const [askReq, setAskReq] = useState<AskRequest | null>(null);
  const status = useCall<AppStatus>(api.status, "status", { refreshMs: 60_000 });
  // first run: the setup wizard until onboarding is done; Settings can bring it back
  const [setup, setSetup] = useState(false);
  const counts = useCall<Counts>(loadCounts, "counts", { refreshMs: 60_000 });
  const reloadStatus = status.reload;
  const reloadCounts = counts.reload;

  useEffect(() => {
    const onHash = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHash);
    const clock = setInterval(() => setNow(new Date()), 15_000);
    // the theme lives in settings; storage only paints the first frame
    api.settings().then(
      (s) => applyTheme(s.theme),
      () => {},
    );
    return () => {
      window.removeEventListener("hashchange", onHash);
      clearInterval(clock);
    };
  }, []);

  useEffect(() => {
    onboardingState().then(
      (o) => setSetup(!o.done),
      () => setSetup(false),
    );
    const rerun = () => {
      setSettings(false);
      setStep("welcome").then(
        () => setSetup(true),
        () => setSetup(true),
      );
    };
    window.addEventListener("hq:setup", rerun);
    return () => window.removeEventListener("hq:setup", rerun);
  }, []);

  const notify: Notify = useCallback((message, tone = "ok") => {
    setToast({ message, tone });
    window.setTimeout(() => setToast(null), 4200);
  }, []);
  const changed = useCallback(() => {
    reloadStatus();
    reloadCounts();
  }, [reloadStatus, reloadCounts]);

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

  const startCreate = useCallback((kind: CreateKind, text = "") => {
    setPalette(false);
    setCreate({ kind, text });
  }, []);

  // The tray menu and the palette hotkey reach the page through these (windows.rs).
  useEffect(() => {
    const openPalette = () => {
      setSettings(false);
      setPalette(true);
    };
    const openSettings = () => {
      setPalette(false);
      setSettings(true);
    };
    window.addEventListener("hq:palette", openPalette);
    window.addEventListener("hq:settings", openSettings);
    return () => {
      window.removeEventListener("hq:palette", openPalette);
      window.removeEventListener("hq:settings", openSettings);
    };
  }, []);

  // Global keys. Ctrl/⌘K works everywhere, including inside inputs; the single-key
  // shortcuts only fire when no field has focus and no dialog is open.
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
      if (setup || palette || settings || create || typing(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
      const n = Number(e.key);
      if (n >= 1 && n <= VIEWS.length) go(`#/${VIEWS[n - 1]}`);
      else if (e.key === "/") {
        e.preventDefault();
        go("#/ask");
        window.setTimeout(() => document.getElementById("ask-q")?.focus(), 0);
      } else if (e.key === "n") {
        e.preventDefault();
        startCreate("note");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, setup, palette, settings, create, startCreate]);

  if (setup) {
    return (
      <Onboarding
        onFinish={() => {
          setSetup(false);
          reloadStatus();
        }}
      />
    );
  }

  return (
    <div className="app">
      <TopBar now={now} route={route} status={status.data} counts={counts.data} onCreate={startCreate} />
      <main className="stage" id="main">
        {route.view === "home" && <Home date={route.date} onAsk={ask} />}
        {route.view === "needs" && <NeedsYou notify={notify} onChange={changed} />}
        {route.view === "days" && <Days date={route.date} span={route.span} notify={notify} />}
        {route.view === "tasks" && <Tasks notify={notify} onChange={changed} />}
        {route.view === "inbox" && <Inbox notify={notify} onChange={changed} />}
        {route.view === "notes" && <Notes note={route.note} notify={notify} />}
        {route.view === "ask" && <Ask req={askReq} suggestions={[]} />}
        {route.view === "bookmarks" && (
          <Bookmarks notify={(m, kind) => notify(m, kind === "error" ? "error" : "ok")} onOpenNote={(id) => go(`#/notes/${encodeURIComponent(id)}`)} />
        )}
        {/* ---- streams: add view elements above this line ---- */}
      </main>
      <Hints view={route.view} />
      {palette && <Palette onClose={() => setPalette(false)} go={go} ask={ask} notify={notify} onSaved={changed} openSettings={() => setSettings(true)} onCreate={startCreate} />}
      {settings && <Settings status={status.data} onClose={() => setSettings(false)} notify={notify} onChange={changed} />}
      {create && <Create request={create} status={status.data} onClose={() => setCreate(null)} notify={notify} go={go} onSaved={changed} />}
      <div className={`toast ${toast ? "is-shown" : ""} ${toast?.tone === "error" ? "is-error" : ""}`} role="status" aria-live="polite">
        {toast?.message}
      </div>
    </div>
  );
}
