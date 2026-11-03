import { useCallback, useEffect, useState } from "react";
import { ageLabel, errorMessage, host, memories, memoryDismiss, type Memory } from "../api/bookmarks.ts";
import PlatformBadge from "./BookmarksBadge.tsx";
import "./Bookmarks.css";

function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/**
 * Home's memories card: one or two old bookmarks, the same all day. "Show different ones"
 * steps to the next pair; "dismiss" hides one for good.
 */
export default function Memories() {
  const [salt, setSalt] = useState(0);
  const [items, setItems] = useState<Memory[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState(false);

  // A Home card that can't load (no vault yet, an older build) stays out of the way.
  const load = useCallback(() => {
    memories(today(), salt)
      .then((m) => (setItems(m), setError(null), setHidden(false)))
      .catch(() => setHidden(true));
  }, [salt]);
  useEffect(load, [load]);

  async function dismiss(id: string) {
    try {
      await memoryDismiss(id);
      load();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  if (hidden) return null;
  return (
    <section className="panel bm-memories" aria-label="Memories">
      <header className="panel-head bm-memories-head">
        <span className="panel-kicker">memories</span>
        {items && items.length > 0 && (
          <button className="link-button" onClick={() => setSalt((s) => s + 1)}>
            show different ones
          </button>
        )}
      </header>
      {error && <p className="is-error">{error}</p>}
      {!error && !items && <p className="bm-faint">…</p>}
      {items && items.length === 0 && (
        <p className="bm-faint">Nothing to bring back yet. Bookmarks older than a week turn up here.</p>
      )}
      {items && (
        <ul className="bm-memory-list">
          {items.map((m) => (
            <li key={m.id} className="bm-memory">
              <div className="bm-line">
                <PlatformBadge platform={m.platform} />
                <a className="bm-title" href={m.url} target="_blank" rel="noreferrer">
                  {m.title}
                </a>
              </div>
              <p className="bm-meta">
                {host(m.url)} · saved {ageLabel(m.ageDays)} · {m.source}
              </p>
              {m.summary && <p className="bm-summary">{m.summary}</p>}
              {m.tags.length > 0 && (
                <p className="bm-tags">
                  {m.tags.map((t) => (
                    <span key={t} className="bm-tag">
                      #{t}
                    </span>
                  ))}
                </p>
              )}
              <button className="link-button bm-dismiss" onClick={() => dismiss(m.id)}>
                dismiss
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
