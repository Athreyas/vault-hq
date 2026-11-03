import { useEffect, useRef, useState } from "react";
import type { AppStatus } from "../api.ts";
import type { Counts, Route, View } from "../App.tsx";
import { VIEWS } from "../App.tsx";
import type { CreateKind } from "./Create.tsx";

const TIPS: Record<View, string> = {
  home: "Overview of the day",
  needs: "Decisions waiting on you",
  days: "What happened, day by day",
  tasks: "Every open task across the vaults",
  inbox: "Captures and agent notes to curate",
  notes: "Read and edit any note",
  ask: "Search and ask the vault on a local model",
  bookmarks: "Every saved link, from your browsers and pasted",
};

function badge(view: View, c: Counts | null): number {
  if (!c) return 0;
  return view === "needs" ? c.needs : view === "inbox" ? c.inbox : 0;
}

const CREATE: { kind: CreateKind; label: string; hint: string }[] = [
  { kind: "note", label: "note", hint: "from a template" },
  { kind: "task", label: "task", hint: "into Tasks.md or a project" },
  { kind: "bookmark", label: "bookmark", hint: "save a link" },
];

function CreateMenu({ onCreate }: { onCreate: (k: CreateKind) => void }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div className="create-menu" ref={box}>
      <button type="button" className="button create-button" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)} title="New note, task or bookmark (n)">
        +
      </button>
      {open && (
        <ul className="create-list" role="menu">
          {CREATE.map((c) => (
            <li key={c.kind} role="none">
              <button
                type="button"
                role="menuitem"
                autoFocus={c.kind === "note"}
                onClick={() => {
                  setOpen(false);
                  onCreate(c.kind);
                }}
              >
                <span>{c.label}</span>
                <small>{c.hint}</small>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function TopBar({ now, route, status, counts, onCreate }: { now: Date; route: Route; status: AppStatus | null; counts: Counts | null; onCreate: (k: CreateKind) => void }) {
  const primary = status?.vaults.find((v) => v.primary) ?? status?.vaults[0] ?? null;
  const unreadable = status?.vaults.reduce((n, v) => n + v.unreadable, 0) ?? 0;
  const problems = status?.problems ?? [];
  return (
    <header className="bar">
      <a className="brand" href="#/home" title={status ? `Vault HQ ${status.version}` : "Vault HQ"}>
        <span aria-hidden="true">▌</span>hq<span className="brand-vault">{primary ? `:${primary.name}` : ""}</span>
      </a>
      <nav className="tabs" aria-label="Sections">
        {VIEWS.map((v, i) => {
          const n = badge(v, counts);
          return (
            <a key={v} href={`#/${v}`} className={`tab ${route.view === v ? "is-active" : ""}`} aria-current={route.view === v ? "page" : undefined} title={TIPS[v]}>
              <kbd>{i + 1}</kbd>
              {v}
              {n > 0 && <b className={`tab-count ${v === "needs" ? "is-attention" : ""}`}>{n}</b>}
            </a>
          );
        })}
      </nav>
      <div className="bar-right">
        {problems.length > 0 && (
          <span className="sync is-attention" title={problems.join("\n")}>
            {problems.length === 1 ? "1 vault problem" : `${problems.length} vault problems`}
          </span>
        )}
        {unreadable > 0 && (
          <span className="sync is-attention" title="HQ could not read these notes, so they are missing from every view. In Finder, right-click the vault folder and choose Keep Downloaded.">
            {unreadable} notes unreadable
          </span>
        )}
        {status && status.vaults.length > 1 && <span className="sync">{status.vaults.length} vaults</span>}
        <CreateMenu onCreate={onCreate} />
        <time className="clock" title={now.toLocaleDateString("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" })}>
          {now.toLocaleDateString("en-US", { month: "short", day: "numeric" })} {now.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false })}
        </time>
      </div>
    </header>
  );
}
