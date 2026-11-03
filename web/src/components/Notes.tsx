import { useEffect, useMemo, useRef, useState } from "react";
import type { NoteSummary, NoteView } from "../../../server/types.ts";
import type { Notify } from "../App.tsx";
import { noteUrl, useData } from "../api.ts";
import Editor from "./Editor.tsx";
import { Markdown, RichLine } from "./Rich.tsx";

// The vault's notes, read inside HQ: a filterable list on the left, the note on the right
// with its properties and the notes that link to it.
export default function Notes({ note, notify }: { note: string | null; notify: Notify }) {
  const list = useData<NoteSummary[]>("/api/notes", 60_000);
  const view = useData<NoteView>(note ? `/api/note?ref=${encodeURIComponent(note)}` : "/api/notes", 30_000);
  const [filter, setFilter] = useState("");
  const [sort, setSort] = useState<"folder" | "recent">("folder");
  const [sel, setSel] = useState(0);
  const [editing, setEditing] = useState<string | null>(null); // rel of the note being edited
  const input = useRef<HTMLInputElement>(null);
  const shown = note && view.data && "body" in view.data ? view.data : null;

  const items = useMemo(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const all = (list.data ?? []).filter((n) => words.every((w) => `${n.title} ${n.rel}`.toLowerCase().includes(w)));
    return sort === "recent" ? [...all].sort((a, b) => b.updated.localeCompare(a.updated)) : all;
  }, [list.data, filter, sort]);

  useEffect(() => setSel(0), [filter, sort]);
  useEffect(() => setEditing(null), [note]); // a different note: leave the editor (its draft is kept)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea, select") || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "f") {
        e.preventDefault();
        input.current?.focus();
      } else if (e.key === "r") setSort((s) => (s === "folder" ? "recent" : "folder"));
      else if (e.key === "o" && shown) window.location.href = shown.obsidian;
      else if (e.key === "e" && shown && !editing) {
        e.preventDefault();
        setEditing(shown.rel);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  useEffect(() => {
    document.querySelector(".note-list .is-selected")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  function onFilterKey(e: React.KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, items.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter" && items[sel]) {
      e.preventDefault();
      window.location.hash = noteUrl(items[sel].rel);
    }
  }

  let lastFolder: string | null = null;
  return (
    <section className="notes">
      <aside className="note-browser" aria-label="Notes">
        <div className="prompt">
          <span aria-hidden="true">❯</span>
          <label className="sr-only" htmlFor="note-filter">
            Filter notes
          </label>
          <input id="note-filter" ref={input} value={filter} onChange={(e) => setFilter(e.target.value)} onKeyDown={onFilterKey} placeholder={`filter ${list.data?.length ?? ""} notes`} autoComplete="off" />
        </div>
        <div className="segmented" role="group" aria-label="Order">
          <button type="button" className={sort === "folder" ? "is-active" : ""} onClick={() => setSort("folder")}>
            by folder
          </button>
          <button type="button" className={sort === "recent" ? "is-active" : ""} onClick={() => setSort("recent")}>
            recent
          </button>
        </div>
        {list.error && <p className="error">{list.error}</p>}
        <ul className="note-list">
          {items.map((n, i) => {
            const head = sort === "folder" && n.folder !== lastFolder ? n.folder || "vault root" : null;
            lastFolder = n.folder;
            const active = shown?.rel === n.rel;
            return (
              <li key={n.rel}>
                {head && <p className="note-folder">{head}</p>}
                <a href={noteUrl(n.rel)} className={`note-item ${i === sel && filter ? "is-selected" : ""} ${active ? "is-active" : ""}`} aria-current={active ? "page" : undefined} title={n.rel}>
                  <span>{n.title}</span>
                  <small>{sort === "recent" ? `${n.updated} · ${n.folder}` : n.updated}</small>
                </a>
              </li>
            );
          })}
          {list.data && !items.length && <li className="empty">No note matches “{filter}”.</li>}
        </ul>
      </aside>

      <article className="note-reader">
        {!note && <p className="empty is-large">Pick a note, or press f and type to filter.</p>}
        {note && view.error && <p className="error">{view.error}</p>}
        {note && !shown && !view.error && <p className="empty">Opening {note}…</p>}
        {shown && (
          <>
            <header className="note-head">
              <p className="note-path">{shown.rel}</p>
              <h1>{shown.title}</h1>
              <div className="note-actions">
                {!editing && (
                  <button type="button" className="button is-primary" onClick={() => setEditing(shown.rel)} title="Edit here (e)">
                    edit
                  </button>
                )}
                <a className="button" href={shown.obsidian} title="Open in Obsidian (o)">
                  open in obsidian
                </a>
              </div>
              {shown.props.length > 0 && (
                <dl className="note-props">
                  {shown.props.map(([k, v]) => (
                    <div key={k}>
                      <dt>{k}</dt>
                      <dd>
                        <RichLine text={v} />
                      </dd>
                    </div>
                  ))}
                </dl>
              )}
            </header>
            {editing === shown.rel ? (
              <Editor
                rel={shown.rel}
                notify={notify}
                onClose={(saved) => {
                  setEditing(null);
                  if (saved) {
                    view.reload();
                    list.reload();
                  }
                }}
              />
            ) : (
              <div className="note-body">
                <Markdown text={shown.body} />
              </div>
            )}
            <footer className="note-backlinks">
              <h2>Linked from {shown.backlinks.length ? shown.backlinks.length : "no other note"}</h2>
              {shown.backlinks.length > 0 && (
                <ul>
                  {shown.backlinks.map((b) => (
                    <li key={b.rel}>
                      <a className="note-link" href={noteUrl(b.rel)}>
                        {b.title}
                      </a>{" "}
                      <small>{b.folder}</small>
                    </li>
                  ))}
                </ul>
              )}
            </footer>
          </>
        )}
      </article>
    </section>
  );
}
