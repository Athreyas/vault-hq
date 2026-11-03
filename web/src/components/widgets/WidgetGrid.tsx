// Home's widget row: the user's own arrangement of small, quiet widgets. "arrange" turns
// on editing: drag a card (or use its ⠿ handle with the keyboard) to reorder, ⚙ to
// configure, ✕ to remove, "+ add" to pick a new one. Every change is saved at once.
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { WidgetCatalogEntry } from "../../bindings/WidgetCatalogEntry.ts";
import type { WidgetData } from "../../bindings/WidgetData.ts";
import type { WidgetInstance } from "../../bindings/WidgetInstance.ts";
import type { WidgetLayout } from "../../bindings/WidgetLayout.ts";
import { errorText, widgetData, widgetsCatalog, widgetsLayoutGet, widgetsLayoutSet } from "../../api/widgets.ts";
import { CatalogPicker } from "./CatalogPicker.tsx";
import { addWidget, move, moveById, pollSecs, removeWidget, setConfig, widgetTitle } from "./layout.ts";
import { WidgetBody } from "./renderers.tsx";
import { WidgetSettings } from "./WidgetSettings.tsx";
import "./widgets.css";

interface Grab {
  id: string;
  original: WidgetLayout;
}

export default function WidgetGrid() {
  const [catalog, setCatalog] = useState<WidgetCatalogEntry[]>([]);
  const [layout, setLayout] = useState<WidgetLayout | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [adding, setAdding] = useState(false);
  const [configuring, setConfiguring] = useState<string | null>(null);
  const [removed, setRemoved] = useState<{ before: WidgetLayout; title: string } | null>(null);
  const [grab, setGrab] = useState<Grab | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const [announce, setAnnounce] = useState("");

  useEffect(() => {
    Promise.all([widgetsCatalog(), widgetsLayoutGet()])
      .then(([c, l]) => {
        setCatalog(c);
        setLayout(l);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  const entryOf = useCallback((w: WidgetInstance) => catalog.find((e) => e.kind === w.kind), [catalog]);

  // Optimistic: show the change, save it, and put the old layout back if the save fails.
  const commit = useCallback(
    async (next: WidgetLayout, prev: WidgetLayout | null = layout): Promise<string | null> => {
      setLayout(next);
      try {
        setLayout(await widgetsLayoutSet(next));
        setError(null);
        return null;
      } catch (e) {
        if (prev) setLayout(prev);
        const m = errorText(e);
        setError(m);
        return m;
      }
    },
    [layout],
  );

  if (!layout) {
    return <section className="w-grid-wrap">{error ? <p className="w-note is-bad">Widgets: {error}</p> : <p className="w-muted">…</p>}</section>;
  }

  const widgets = layout.widgets;
  const n = widgets.length;
  const nameAt = (id: string) => {
    const w = widgets.find((x) => x.id === id);
    return w ? widgetTitle(w, entryOf(w)) : "widget";
  };

  const onHandleKey = (e: KeyboardEvent, w: WidgetInstance, i: number) => {
    const name = widgetTitle(w, entryOf(w));
    if (!grab) {
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setGrab({ id: w.id, original: layout });
        setAnnounce(`Picked up ${name}, position ${i + 1} of ${n}. Arrow keys move it, Space drops it, Escape cancels.`);
      }
      return;
    }
    const to =
      e.key === "ArrowLeft" || e.key === "ArrowUp" ? i - 1 : e.key === "ArrowRight" || e.key === "ArrowDown" ? i + 1 : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : null;
    if (to !== null) {
      e.preventDefault();
      const t = Math.max(0, Math.min(n - 1, to));
      setLayout({ ...layout, widgets: move(widgets, i, t) });
      setAnnounce(`${name}, position ${t + 1} of ${n}.`);
    } else if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      const original = grab.original;
      setGrab(null);
      setAnnounce(`Dropped ${name} at position ${i + 1} of ${n}.`);
      void commit(layout, original);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setLayout(grab.original);
      setGrab(null);
      setAnnounce(`Cancelled. ${name} is back where it was.`);
    }
  };

  const dropOn = (insertAt: number) => {
    if (!dragId) return;
    const from = widgets.findIndex((w) => w.id === dragId);
    const to = insertAt > from ? insertAt - 1 : insertAt;
    setDragId(null);
    setDropAt(null);
    if (from < 0 || to === from) return;
    setAnnounce(`Moved ${nameAt(dragId)} to position ${to + 1} of ${n}.`);
    void commit(moveById(layout, dragId, to));
  };

  const configured = configuring ? widgets.find((w) => w.id === configuring) : undefined;
  const configuredEntry = configured && entryOf(configured);

  return (
    <section className={`w-grid-wrap${editing ? " is-editing" : ""}`} aria-label="Widgets">
      <div className="w-toolbar">
        <span className="label">widgets</span>
        {removed && (
          <span className="w-muted">
            removed {removed.title} ·{" "}
            <button
              type="button"
              className="link-button"
              onClick={() => {
                void commit(removed.before);
                setRemoved(null);
              }}
            >
              undo
            </button>
          </span>
        )}
        {error && (
          <span className="w-note is-bad" role="alert">
            {error}
          </span>
        )}
        <span className="w-toolbar-gap" />
        {editing && (
          <button type="button" className="button" onClick={() => setAdding(true)}>
            + add
          </button>
        )}
        <button
          type="button"
          className={`button${editing ? " is-primary" : ""}`}
          aria-pressed={editing}
          onClick={() => {
            setEditing(!editing);
            setGrab(null);
            setRemoved(null);
          }}
        >
          {editing ? "done" : "arrange"}
        </button>
      </div>

      {n === 0 ? (
        <p className="w-muted">
          No widgets on Home.{" "}
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setEditing(true);
              setAdding(true);
            }}
          >
            + add one
          </button>
        </p>
      ) : (
        <ol
          className="w-grid"
          onDragOver={(e) => {
            // past the last card: drop at the end
            if (dragId && e.target === e.currentTarget) {
              e.preventDefault();
              setDropAt(n);
            }
          }}
          onDrop={(e) => {
            if (e.target === e.currentTarget) {
              e.preventDefault();
              dropOn(n);
            }
          }}
        >
          {widgets.map((w, i) => {
            const entry = entryOf(w);
            const title = widgetTitle(w, entry);
            return (
              <li
                key={w.id || `new-${i}`}
                className={[
                  "w-cell",
                  dragId === w.id ? "is-dragging" : "",
                  grab?.id === w.id ? "is-grabbed" : "",
                  dropAt === i ? "is-drop-before" : "",
                  dropAt === n && i === n - 1 ? "is-drop-after" : "",
                ].join(" ")}
                draggable={editing && !!w.id}
                onDragStart={(e) => {
                  setDragId(w.id);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", w.id);
                }}
                onDragEnd={() => {
                  setDragId(null);
                  setDropAt(null);
                }}
                onDragOver={(e) => {
                  if (!dragId) return;
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  const box = e.currentTarget.getBoundingClientRect();
                  setDropAt(e.clientX < box.left + box.width / 2 ? i : i + 1);
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  if (dropAt !== null) dropOn(dropAt);
                }}
              >
                <WidgetCard instance={w} entry={entry} title={title}>
                  {editing && (
                    <span className="w-controls">
                      <button
                        type="button"
                        className="link-button w-handle"
                        aria-label={`Move ${title}, position ${i + 1} of ${n}`}
                        aria-pressed={grab?.id === w.id}
                        disabled={!w.id}
                        onKeyDown={(e) => onHandleKey(e, w, i)}
                        onBlur={() => {
                          if (grab?.id === w.id) {
                            const original = grab.original;
                            setGrab(null);
                            void commit(layout, original);
                          }
                        }}
                        title="Drag, or press Space then arrow keys"
                      >
                        ⠿
                      </button>
                      {entry && entry.fields.length > 0 && (
                        <button type="button" className="link-button" aria-label={`${title} settings`} title="Settings" onClick={() => setConfiguring(w.id)} disabled={!w.id}>
                          ⚙
                        </button>
                      )}
                      <button
                        type="button"
                        className="link-button"
                        aria-label={`Remove ${title}`}
                        title="Remove"
                        onClick={() => {
                          setRemoved({ before: layout, title });
                          setAnnounce(`Removed ${title}.`);
                          void commit(removeWidget(layout, w.id));
                        }}
                      >
                        ✕
                      </button>
                    </span>
                  )}
                </WidgetCard>
              </li>
            );
          })}
        </ol>
      )}

      <p className="sr-only" role="status" aria-live="polite">
        {announce}
      </p>

      {adding && (
        <CatalogPicker
          catalog={catalog}
          layout={layout}
          onClose={() => setAdding(false)}
          onPick={(e) => {
            setAdding(false);
            setAnnounce(`Added ${e.title}.`);
            void commit(addWidget(layout, e));
          }}
        />
      )}
      {configured && configuredEntry && (
        <WidgetSettings instance={configured} entry={configuredEntry} onClose={() => setConfiguring(null)} onSave={(config) => commit(setConfig(layout, configured.id, config))} />
      )}
    </section>
  );
}

/** Polls the widget's data at its kind's pace, and not at all while the window is hidden. */
function useWidgetData(w: WidgetInstance, entry: WidgetCatalogEntry | undefined) {
  const [data, setData] = useState<WidgetData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const configKey = JSON.stringify(w.config);
  const every = pollSecs(entry, w.config);
  const live = useRef(true);

  useEffect(() => {
    if (!entry) return;
    live.current = true;
    const config = JSON.parse(configKey) as Record<string, unknown>;
    const run = () => {
      if (document.hidden) return;
      widgetData(w.kind, config)
        .then((d) => live.current && (setData(d), setError(null)))
        .catch((e) => live.current && setError(errorText(e)));
    };
    run();
    const timer = every > 0 ? setInterval(run, every * 1000) : undefined;
    const onVisible = () => !document.hidden && run();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      live.current = false;
      if (timer) clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [w.kind, configKey, every, entry]);

  return { data, error };
}

function WidgetCard({ instance, entry, title, children }: { instance: WidgetInstance; entry: WidgetCatalogEntry | undefined; title: string; children?: ReactNode }) {
  const { data, error } = useWidgetData(instance, entry);
  const status = error ? "bad" : (data?.status ?? "unknown");
  return (
    <article className={`w-card is-${status}`} aria-label={data ? `${title}: ${data.summary}` : title}>
      <header className="w-head">
        <h3 className="w-title" title={entry?.description}>
          {title}
        </h3>
        {children ?? (
          <span className="w-kicker" title={data?.summary}>
            {instance.kind === "clock" ? "" : (data?.summary ?? "")}
          </span>
        )}
      </header>
      {error ? <p className="w-note is-bad">{error}</p> : <WidgetBody instance={instance} data={data} />}
    </article>
  );
}

export { WidgetGrid };
