import { useEffect, useMemo, useRef, useState } from "react";
import type { Notify } from "../App.tsx";
import { VIEWS } from "../App.tsx";
import { addDays, api, iso, noteUrl, notBuilt, savedTheme, setTheme, type NoteSummary } from "../api.ts";
import { isLoneUrl } from "../lib/text.ts";
import type { CreateKind } from "./Create.tsx";

interface Item {
  id: string;
  group: string;
  label: string;
  hint: string;
  run: () => void | Promise<void>;
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

const HINTS: Record<(typeof VIEWS)[number], string> = {
  home: "overview",
  needs: "decisions waiting",
  days: "day, 3 days, week",
  tasks: "open tasks",
  inbox: "captures to curate",
  notes: "read and edit",
  ask: "search and ask",
};

// ⌘K: one box to jump to a view or note, ask, search, create, or capture a thought.
export default function Palette({
  onClose,
  go,
  ask,
  notify,
  onSaved,
  openSettings,
  onCreate,
}: {
  onClose: () => void;
  go: (hash: string) => void;
  ask: (q: string, mode?: "ask" | "search") => void;
  notify: Notify;
  onSaved: () => void;
  openSettings: () => void;
  onCreate: (kind: CreateKind, text?: string) => void;
}) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const list = useRef<HTMLUListElement>(null);
  const text = q.trim();
  const [noteList, setNoteList] = useState<NoteSummary[]>([]);
  useEffect(() => {
    api.notes().then(setNoteList, () => setNoteList([]));
  }, []);

  const items = useMemo<Item[]>(() => {
    const t = iso(new Date());
    const done = (fn: () => void | Promise<void>) => async () => {
      onClose();
      await fn();
    };
    const attempt = (fn: () => Promise<unknown>, ok: string, what: string) =>
      done(async () => {
        try {
          await fn();
          notify(ok);
          onSaved();
        } catch (e) {
          notify(notBuilt(e) ? `${what} is not in this build yet` : (e as Error).message, "error");
        }
      });

    const nav: Item[] = VIEWS.map((v, i) => ({ id: `go-${v}`, group: "go to", label: v, hint: `${HINTS[v]} · key ${i + 1}`, run: done(() => go(`#/${v}`)) }));
    const other = savedTheme() === "dark" ? "light" : "dark";
    nav.push({ id: "settings", group: "settings", label: "settings", hint: "vaults, local AI, keys · ⌘,", run: done(openSettings) });
    nav.push({
      id: "theme",
      group: "settings",
      label: `theme: ${other}`,
      hint: `switch to the ${other} theme`,
      run: done(() => setTheme(other).catch((e: Error) => notify(e.message, "error"))),
    });
    nav.push({ id: "new-note", group: "create", label: "new note", hint: "from a template · n", run: done(() => onCreate("note", text)) });
    nav.push({ id: "new-task", group: "create", label: "new task", hint: "due date, priority", run: done(() => onCreate("task", text)) });
    nav.push({ id: "save-link", group: "create", label: "save link", hint: "a bookmark note", run: done(() => onCreate("bookmark", isLoneUrl(text) ? text : "")) });
    nav.push({
      id: "quick-capture",
      group: "capture",
      label: "quick capture",
      hint: "the small window · ⌥⌘Space",
      run: done(async () => {
        await api.openCapture().catch((e: Error) => notify(e.message, "error"));
      }),
    });
    const days: Item[] = [
      { id: "d-today", group: "days", label: "days: today", hint: t, run: done(() => go(`#/days/${t}`)) },
      { id: "d-yest", group: "days", label: "days: yesterday", hint: addDays(t, -1), run: done(() => go(`#/days/${addDays(t, -1)}`)) },
      { id: "d-week", group: "days", label: "days: this week", hint: `7 days to ${t}`, run: done(() => go(`#/days/${t}/7`)) },
    ];
    const lower = text.toLowerCase();
    const matches = (i: Item) => !lower || i.label.toLowerCase().includes(lower);
    const actions: Item[] = [];
    if (DATE_RE.test(text)) actions.push({ id: "a-date", group: "days", label: `open day ${text}`, hint: "Days view", run: done(() => go(`#/days/${text}`)) });
    if (isLoneUrl(text)) {
      actions.push({ id: "a-link", group: "create", label: `save link: ${text}`, hint: "bookmark now", run: attempt(() => api.bookmarkAdd(text), "Bookmark saved", "Saving links") });
    } else if (text.length >= 3) {
      actions.push({ id: "a-ask", group: "ask", label: `ask: ${text}`, hint: "local model reads your notes", run: done(() => ask(text, "ask")) });
      actions.push({ id: "a-search", group: "ask", label: `search notes: ${text}`, hint: "matching notes, no model", run: done(() => ask(text, "search")) });
    }
    if (text.length >= 1 && !isLoneUrl(text)) {
      actions.push({ id: "a-capture", group: "capture", label: `capture: ${text}`, hint: "today's inbox note", run: attempt(() => api.capture(text, "note"), "Captured to today's inbox", "Capture") });
      actions.push({ id: "a-task", group: "create", label: `new task: ${text}`, hint: "add it now", run: attempt(() => api.taskAdd({ description: text }), `Task added: ${text}`, "Adding tasks") });
      actions.push({ id: "a-note", group: "create", label: `new note: ${text}`, hint: "pick a template", run: done(() => onCreate("note", text)) });
    }
    const shownNav = [...nav, ...days].filter(matches);
    // notes whose title matches every typed word, best (title starts with the text) first
    const words = lower.split(/\s+/).filter(Boolean);
    const noteHits: Item[] = words.length
      ? noteList
          .filter((n) => words.every((w) => n.title.toLowerCase().includes(w)))
          .sort((a, b) => Number(b.title.toLowerCase().startsWith(lower)) - Number(a.title.toLowerCase().startsWith(lower)) || b.updated.localeCompare(a.updated))
          .slice(0, 6)
          .map((n) => ({ id: `n-${n.id}`, group: "notes", label: n.title, hint: n.folder || "vault root", run: done(() => go(noteUrl(n.id))) }))
      : [];
    // When the text names a place, places come first; otherwise the actions do.
    return shownNav.length && text ? [...shownNav, ...noteHits, ...actions] : [...noteHits, ...actions, ...shownNav];
  }, [text, noteList, go, ask, notify, onSaved, onClose, openSettings, onCreate]);

  useEffect(() => setSel(0), [text]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(".is-selected")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  function onKey(e: React.KeyboardEvent) {
    if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, items.length - 1));
    } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      void items[sel]?.run();
    }
  }

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="prompt">
          <span aria-hidden="true">❯</span>
          <label className="sr-only" htmlFor="palette-q">
            Type a place, a question, a link or a thought
          </label>
          <input
            id="palette-q"
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onKey}
            placeholder="go to, ask, search, create or capture"
            autoComplete="off"
            spellCheck={false}
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-activedescendant={items[sel] ? `pal-${items[sel].id}` : undefined}
          />
        </div>
        <ul id="palette-list" className="palette-list" role="listbox" ref={list}>
          {items.map((it, i) => (
            <li key={it.id} id={`pal-${it.id}`} role="option" aria-selected={i === sel} className={`palette-item ${i === sel ? "is-selected" : ""}`} onMouseMove={() => setSel(i)} onClick={() => void it.run()}>
              <span className="palette-group">{it.group}</span>
              <span className="palette-label">{it.label}</span>
              <span className="palette-hint">{it.hint}</span>
            </li>
          ))}
          {items.length === 0 && <li className="palette-none">Nothing matches. Type more to ask, search or capture.</li>}
        </ul>
        <p className="palette-foot">
          <kbd>↑ ↓</kbd> choose <kbd>⏎</kbd> run <kbd>Esc</kbd> close
        </p>
      </div>
    </div>
  );
}
