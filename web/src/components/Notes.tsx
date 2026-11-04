import { useEffect, useMemo, useRef, useState } from "react";
import type { Notify } from "../App.tsx";
import { api, noteUrl, useCall, type NoteView } from "../api.ts";
import Editor from "./Editor.tsx";
import Problem from "./Problem.tsx";
import { Markdown, RichLine } from "./Rich.tsx";

const ARCHIVE = "08-Archive/";

// The vaults' notes, read inside HQ: a filterable list on the left, the note on the right
// with its properties and the notes that link to it.
export default function Notes({ note, notify }: { note: string | null; notify: Notify }) {
  const list = useCall(() => api.notes({ hideSystem: true }), "notes", { refreshMs: 60_000 });
  const view = useCall<NoteView | null>(() => (note ? api.note(note) : Promise.resolve(null)), note ?? "", { refreshMs: 30_000 });
  const [filter, setFilter] = useState("");
  const [sort, setSort] = useState<"folder" | "recent">("folder");
  const [sel, setSel] = useState(0);
  const [editing, setEditing] = useState<string | null>(null); // id of the note being edited
  const input = useRef<HTMLInputElement>(null);
  const shown = note && view.data ? view.data : null;
  const vaults = useMemo(() => new Set((list.data ?? []).map((n) => n.vault)).size, [list.data]);
  const vaultInfo = useCall(() => api.vaults(), "vaults");
  const vaultName = useMemo(() => new Map((vaultInfo.data ?? []).map((v) => [v.id, v.name])), [vaultInfo.data]);

  const items = useMemo(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const all = (list.data ?? []).filter((n) => words.every((w) => `${n.title} ${n.rel}`.toLowerCase().includes(w)));
    return sort === "recent" ? [...all].sort((a, b) => b.updated.localeCompare(a.updated)) : all;
  }, [list.data, filter, sort]);

  useEffect(() => setSel(0), [filter, sort]);
  useEffect(() => setEditing(null), [note]); // a different note: leave the editor (its draft is kept)

  async function reveal() {
    if (!shown) return;
    try {
      await api.reveal(shown.id);
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  async function archive() {
    if (!shown || shown.rel.startsWith(ARCHIVE)) return;
    if (!window.confirm(`Move “${shown.title}” to ${ARCHIVE}? Links to it keep working.`)) return;
    try {
      const moved = await api.noteArchive(shown.id);
      notify(`Archived: ${shown.title}`);
      window.location.hash = noteUrl(moved.id);
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea, select, [role=dialog]") || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "f") {
        e.preventDefault();
        input.current?.focus();
      } else if (e.key === "r") setSort((s) => (s === "folder" ? "recent" : "folder"));
      else if (e.key === "o" && shown) void reveal();
      else if (e.key === "e" && shown && !editing) {
        e.preventDefault();
        setEditing(shown.id);
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
      window.location.hash = noteUrl(items[sel].id);
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
        {list.error && <Problem error={list.error} what="the note list" />}
        <ul className="note-list">
          {items.map((n, i) => {
            const folder = vaults > 1 ? `${vaultName.get(n.vault) ?? n.vault.slice(0, 6)} · ${n.folder}` : n.folder;
            const head = sort === "folder" && folder !== lastFolder ? folder || "vault root" : null;
            lastFolder = folder;
            const active = shown?.id === n.id;
            return (
              <li key={n.id}>
                {head && <p className="note-folder">{head}</p>}
                <a href={noteUrl(n.id)} className={`note-item ${i === sel && filter ? "is-selected" : ""} ${active ? "is-active" : ""}`} aria-current={active ? "page" : undefined} title={n.rel}>
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
        {note && view.error && <Problem error={view.error} what={note} />}
        {note && !shown && !view.error && <p className="empty">Opening {note}…</p>}
        {shown && (
          <>
            <header className="note-head">
              <p className="note-path">{shown.rel}</p>
              <h1>{shown.title}</h1>
              <div className="note-actions">
                {!editing && (
                  <button type="button" className="button is-primary" onClick={() => setEditing(shown.id)} title="Edit here (e)">
                    edit
                  </button>
                )}
                <button type="button" className="button" onClick={() => void reveal()} title="Show in Finder (o)">
                  show in finder
                </button>
                {!shown.rel.startsWith(ARCHIVE) && (
                  <button type="button" className="button" onClick={() => void archive()} title={`Move to ${ARCHIVE}`}>
                    archive
                  </button>
                )}
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
            {editing === shown.id ? (
              <Editor
                id={shown.id}
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
                    <li key={b.id}>
                      <a className="note-link" href={noteUrl(b.id)}>
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
