import { useCallback, useEffect, useState } from "react";
import {
  bookmarkAdd,
  bookmarksEnrich,
  bookmarksEnrichCancel,
  bookmarksQuery,
  emptyFilter,
  errorMessage,
  host,
  onProgress,
  onVaultChanged,
  PLATFORM_LABEL,
  type Bookmark,
  type BookmarkFilter,
  type BookmarkPage,
  type JobProgress,
  type Platform,
} from "../api/bookmarks";
import PlatformBadge from "./BookmarksBadge";
import BookmarksSources from "./BookmarksSources";
import "./Bookmarks.css";

type Layout = "grid" | "list";
type Say = (message: string, kind?: "error") => void;

const LAYOUT_KEY = "hq.bookmarks.layout";

function savedLayout(): Layout {
  try {
    return localStorage.getItem(LAYOUT_KEY) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

function NoteBox({ b, onSaved, say }: { b: Bookmark; onSaved: () => void; say: Say }) {
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  async function save() {
    if (!text.trim()) return;
    setSaving(true);
    try {
      await bookmarkAdd(b.url, text);
      setText("");
      onSaved();
    } catch (e) {
      say(errorMessage(e), "error");
    } finally {
      setSaving(false);
    }
  }
  return (
    <div className="bm-notebox">
      <textarea
        autoFocus
        rows={3}
        placeholder="Why this is worth keeping… (⌘Enter saves)"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && e.metaKey) void save();
        }}
      />
      <button className="button is-primary" disabled={saving || !text.trim()} onClick={save}>
        Add note
      </button>
    </div>
  );
}

function Item({
  b,
  layout,
  onOpenNote,
  onChanged,
  say,
}: {
  b: Bookmark;
  layout: Layout;
  onOpenNote?: (id: string) => void;
  onChanged: () => void;
  say: Say;
}) {
  const [noting, setNoting] = useState(false);
  const [enriching, setEnriching] = useState(false);

  async function enrich() {
    setEnriching(true);
    try {
      const r = await bookmarksEnrich([b.id]);
      if (r.failed) say(r.errors[0] ?? "Enrichment failed.", "error");
      onChanged();
    } catch (e) {
      say(errorMessage(e), "error");
    } finally {
      setEnriching(false);
    }
  }

  return (
    <li className={`bm-item is-${layout}${b.status ? " is-removed" : ""}`}>
      <div className="bm-line">
        <PlatformBadge platform={b.platform} />
        <a className="bm-title" href={b.url} target="_blank" rel="noreferrer" title={b.url}>
          {b.title}
        </a>
      </div>
      <p className="bm-meta">
        <span>{host(b.url)}</span>
        <span>{b.added}</span>
        <span>{b.source}</span>
        {b.folder && <span className="bm-folder">{b.folder}</span>}
        {b.status && <span className="bm-removed">removed in browser</span>}
      </p>
      {layout === "grid" && b.summary && <p className="bm-summary">{b.summary}</p>}
      {b.tags.length > 0 && (
        <p className="bm-tags">
          {b.tags.map((t) => (
            <span key={t} className="bm-tag">
              #{t}
            </span>
          ))}
        </p>
      )}
      <p className="bm-actions">
        <button className="link-button" onClick={() => setNoting((n) => !n)}>
          {noting ? "cancel" : b.hasNotes ? "+ note" : "add note"}
        </button>
        {onOpenNote && (
          <button className="link-button" onClick={() => onOpenNote(b.id)}>
            open note
          </button>
        )}
        <button className="link-button" disabled={enriching} onClick={enrich}>
          {enriching ? "enriching…" : "enrich"}
        </button>
      </p>
      {noting && (
        <NoteBox
          b={b}
          say={say}
          onSaved={() => {
            setNoting(false);
            onChanged();
          }}
        />
      )}
    </li>
  );
}

function PasteLink({ onAdded, say }: { onAdded: () => void; say: Say }) {
  const [url, setUrl] = useState("");
  async function add() {
    const u = url.trim();
    if (!u) return;
    try {
      const b = await bookmarkAdd(u);
      setUrl("");
      say(`Saved ${host(b.url)}. Fetching its title…`);
      onAdded();
    } catch (e) {
      say(errorMessage(e), "error");
    }
  }
  return (
    <form
      className="bm-paste prompt"
      onSubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <span className="bm-faint">+</span>
      <input
        type="url"
        placeholder="paste a link and press Enter"
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        spellCheck={false}
        aria-label="Paste a link to save"
      />
    </form>
  );
}

/**
 * The Bookmarks view: every bookmark note, searchable and filterable by source, tag and
 * platform, as a grid or a list. `onOpenNote` opens one in the note reader, when the shell
 * provides it; `notify` shows messages in the shell's toast, or in a line here.
 */
export default function Bookmarks({ onOpenNote, notify }: { onOpenNote?: (id: string) => void; notify?: Say }) {
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<BookmarkFilter>(emptyFilter);
  const [layout, setLayout] = useState<Layout>(savedLayout);
  const [page, setPage] = useState<BookmarkPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [showSources, setShowSources] = useState(false);
  const [status, setStatus] = useState<{ message: string; kind?: "error" } | null>(null);
  const [enrich, setEnrich] = useState<JobProgress | null>(null);
  const [enrichRunning, setEnrichRunning] = useState(false);
  const query = useDebounced(q, 200);

  const say: Say = useCallback(
    (message, kind) => (notify ? notify(message, kind) : setStatus({ message, kind })),
    [notify],
  );
  const reload = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    let live = true;
    bookmarksQuery(query, filter)
      .then((p) => live && (setPage(p), setError(null)))
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [query, filter, tick]);

  useEffect(() => {
    const offs = [onVaultChanged(reload), onProgress("bookmarks-enrich", setEnrich)];
    return () => {
      for (const off of offs) off.then((f) => f()).catch(() => {});
    };
  }, [reload]);

  useEffect(() => {
    try {
      localStorage.setItem(LAYOUT_KEY, layout);
    } catch {
      // private window: the choice just isn't remembered
    }
  }, [layout]);

  async function enrichAll() {
    setEnrichRunning(true);
    setEnrich(null);
    try {
      const r = await bookmarksEnrich();
      say(
        r.cancelled
          ? `Stopped after ${r.enriched}.`
          : r.failed
            ? `${r.enriched} enriched, ${r.failed} failed: ${r.errors[0] ?? ""}`
            : `${r.enriched} bookmark${r.enriched === 1 ? "" : "s"} tagged and summarised.`,
        r.failed && !r.enriched ? "error" : undefined,
      );
      reload();
    } catch (e) {
      say(errorMessage(e), "error");
    } finally {
      setEnrichRunning(false);
    }
  }

  const set = (patch: Partial<BookmarkFilter>) => setFilter((f) => ({ ...f, ...patch }));

  return (
    <section className="bookmarks">
      <header className="bm-head">
        <h1>
          Bookmarks {page && <span className="count">{page.total}</span>}
        </h1>
        <div className="bm-head-actions">
          {enrichRunning ? (
            <>
              <span className="bm-faint">
                enriching {enrich ? `${enrich.done}/${enrich.total}` : "…"}
              </span>
              <button className="button" onClick={() => bookmarksEnrichCancel()}>
                Stop
              </button>
            </>
          ) : (
            <button className="button" onClick={enrichAll} title="Tags and a one-line summary from your local model">
              Enrich
            </button>
          )}
          <button className={`button${showSources ? " is-primary" : ""}`} onClick={() => setShowSources((s) => !s)}>
            Browsers
          </button>
        </div>
      </header>

      {status && (
        <p className={`bm-status${status.kind === "error" ? " is-error" : ""}`} role="status">
          {status.message}
        </p>
      )}

      {showSources && <BookmarksSources onImported={reload} say={say} />}

      <PasteLink onAdded={reload} say={say} />

      <div className="bm-filters">
        <input
          type="search"
          className="bm-search"
          placeholder="search title, url, tags, notes"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          aria-label="Search bookmarks"
        />
        <select value={filter.source ?? ""} onChange={(e) => set({ source: e.target.value || null })} aria-label="Source">
          <option value="">all sources</option>
          {page?.sources.map((f) => (
            <option key={f.value} value={f.value}>
              {f.value} ({f.count})
            </option>
          ))}
        </select>
        <select
          value={filter.platform ?? ""}
          onChange={(e) => set({ platform: (e.target.value || null) as Platform | null })}
          aria-label="Platform"
        >
          <option value="">all kinds</option>
          {page?.platforms.map((f) => (
            <option key={f.value} value={f.value}>
              {PLATFORM_LABEL[f.value as Platform] ?? f.value} ({f.count})
            </option>
          ))}
        </select>
        <select value={filter.tag ?? ""} onChange={(e) => set({ tag: e.target.value || null })} aria-label="Tag">
          <option value="">all tags</option>
          {page?.tags.map((f) => (
            <option key={f.value} value={f.value}>
              #{f.value} ({f.count})
            </option>
          ))}
        </select>
        <label className="bm-check">
          <input type="checkbox" checked={filter.hideRemoved} onChange={(e) => set({ hideRemoved: e.target.checked })} />
          hide removed
        </label>
        <span className="segmented" role="group" aria-label="Layout">
          <button className={layout === "grid" ? "is-active" : ""} onClick={() => setLayout("grid")}>
            grid
          </button>
          <button className={layout === "list" ? "is-active" : ""} onClick={() => setLayout("list")}>
            list
          </button>
        </span>
      </div>

      {error && <p className="empty is-error">Could not read bookmarks: {error}</p>}
      {!error && !page && <p className="empty">Reading bookmarks…</p>}
      {page && page.total === 0 && (
        <div className="empty is-large">
          <p>{q || filter !== emptyFilter ? "Nothing matches." : "No bookmarks yet."}</p>
          <p className="empty-hint">
            Paste a link above, or open Browsers to bring in the bookmarks you already have. Each one becomes a note in
            10-Bookmarks/.
          </p>
        </div>
      )}
      {page && page.items.length > 0 && (
        <ul className={`bm-items is-${layout}`}>
          {page.items.map((b) => (
            <Item key={b.id} b={b} layout={layout} onOpenNote={onOpenNote} onChanged={reload} say={say} />
          ))}
        </ul>
      )}
      {page && page.items.length < page.total && (
        <p className="bm-faint">
          Showing the newest {page.items.length} of {page.total}. Search or filter to narrow it down.
        </p>
      )}
    </section>
  );
}
