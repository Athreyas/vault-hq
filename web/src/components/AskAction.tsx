import { useState } from "react";
import "./Ask.css";
import { noteUrl } from "../api.ts";
import type { ActionAdded, ActionDraft } from "../api/search.ts";
import type { Priority } from "../bindings/Priority.ts";
import { draftReady, whenLabel } from "./AskState.ts";

const PRIORITIES: (Priority | "")[] = ["", "highest", "high", "medium", "low", "lowest"];
const LOOSE = "09-Planning/Tasks.md";

const relOf = (id: string | null) => (id ? id.slice(id.indexOf(":") + 1) : "");

/**
 * A task read from what the user wrote, every field editable, written only on "add task".
 * `compact` is the quick-capture window's version: what, when, where, and the button.
 */
export default function AskAction({
  draft,
  added,
  busy,
  error,
  compact = false,
  onChange,
  onAdd,
  onCancel,
  onAsk,
}: {
  draft: ActionDraft;
  added: ActionAdded | null;
  busy: boolean;
  error: string | null;
  compact?: boolean;
  onChange: (d: ActionDraft) => void;
  onAdd: () => void;
  onCancel: () => void;
  /** Treat the text as a question after all. */
  onAsk?: () => void;
}) {
  const [newTag, setNewTag] = useState("");
  // where it was filed first, so switching to the loose list can be undone
  const [first] = useState(() => ({ note: draft.note, title: draft.noteTitle }));
  const d = draft;
  const set = (patch: Partial<ActionDraft>) => onChange({ ...d, ...patch });
  const problem = draftReady(d);
  const isLoose = relOf(d.note) === LOOSE;

  if (added) {
    const t = added.task;
    return (
      <div className="ask-action is-added" role="status">
        <p>
          <span className="ask-action-ok">✓ task added</span> {t.description}
          <span className="ask-action-when"> · {whenLabel(t.due, t.dueTime)}</span>
          {t.dueTime && <span className="ask-action-when"> · reminder set</span>}
        </p>
        <p className="ask-action-links">
          in{" "}
          <a className="note-link" href={noteUrl(t.note)}>
            {t.noteTitle}
          </a>
          {!compact && (
            <>
              {" · "}
              <a className="note-link" href="#/tasks">
                open in tasks
              </a>
            </>
          )}
          {added.related && <span className="ask-action-when"> · {added.related}</span>}
        </p>
      </div>
    );
  }

  const addTag = (raw: string) => {
    const t = raw.trim().replace(/^#/, "").replace(/\s+/g, "-").toLowerCase();
    if (t && !d.tags.includes(t)) set({ tags: [...d.tags, t], suggestedTags: d.suggestedTags.filter((x) => x !== t) });
    setNewTag("");
  };

  return (
    <form
      className={`ask-action ${compact ? "is-compact" : ""}`}
      onSubmit={(e) => {
        e.preventDefault();
        if (!problem && !busy) onAdd();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onCancel();
        }
      }}
    >
      <div className="ask-action-row">
        <label className="sr-only" htmlFor="action-desc">
          Task
        </label>
        <span className="ask-action-box" aria-hidden="true">
          ☐
        </span>
        <input
          id="action-desc"
          className="ask-action-desc"
          value={d.description}
          onChange={(e) => set({ description: e.target.value })}
          autoFocus
          autoComplete="off"
          spellCheck
        />
      </div>

      <div className="ask-action-row ask-action-fields">
        <label>
          <span className="label">day</span>
          <input type="date" value={d.due ?? ""} onChange={(e) => set({ due: e.target.value || null, dueTime: e.target.value ? d.dueTime : null })} />
        </label>
        <label>
          <span className="label">time</span>
          <input type="time" value={d.dueTime ?? ""} onChange={(e) => set({ dueTime: e.target.value || null })} />
        </label>
        <label>
          <span className="label">in</span>
          <select
            value={isLoose ? "loose" : "note"}
            onChange={(e) =>
              e.target.value === "loose"
                ? set({ note: d.vault ? `${d.vault}:${LOOSE}` : null, noteTitle: "Tasks" })
                : set({ note: first.note, noteTitle: first.title })
            }
            title={isLoose ? "The loose task list of your primary vault" : relOf(d.note)}
          >
            {relOf(first.note) !== LOOSE && <option value="note">{first.title}</option>}
            <option value="loose">Tasks (loose list)</option>
          </select>
        </label>
        {!compact && (
          <label>
            <span className="label">priority</span>
            <select value={d.priority ?? ""} onChange={(e) => set({ priority: (e.target.value || null) as Priority | null })}>
              {PRIORITIES.map((p) => (
                <option key={p} value={p}>
                  {p || "none"}
                </option>
              ))}
            </select>
          </label>
        )}
        {!compact && isLoose && (
          <label>
            <span className="label">project</span>
            <input value={d.project ?? ""} onChange={(e) => set({ project: e.target.value || null })} placeholder="none" size={14} />
          </label>
        )}
      </div>

      {!compact && (
        <div className="ask-action-row ask-action-tags">
          <span className="label">tags</span>
          {d.tags.map((t) => (
            <button key={t} type="button" className="ask-tag is-on" onClick={() => set({ tags: d.tags.filter((x) => x !== t) })} title="Remove">
              #{t} ×
            </button>
          ))}
          {d.suggestedTags.map((t) => (
            <button key={t} type="button" className="ask-tag" onClick={() => addTag(t)} title="From the matched notes: add">
              +#{t}
            </button>
          ))}
          <input
            className="ask-tag-input"
            value={newTag}
            onChange={(e) => setNewTag(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === ",") {
                e.preventDefault();
                addTag(newTag);
              }
            }}
            placeholder="add tag"
            size={10}
            aria-label="Add a tag"
          />
        </div>
      )}

      {!compact && d.related.length > 0 && (
        <div className="ask-action-row ask-action-related">
          <span className="label">related</span>
          {d.related.map((r, i) => (
            <label key={r.id} title={`${r.id} · ${r.score.toFixed(2)}`}>
              <input
                type="checkbox"
                checked={r.include}
                onChange={() => set({ related: d.related.map((x, j) => (j === i ? { ...x, include: !x.include } : x)) })}
              />
              [[{r.stem}]]{r.kind === "bookmark" && <span className="ask-badge">bookmark</span>}
            </label>
          ))}
        </div>
      )}

      {!compact && d.notes.length > 0 && (
        <ul className="ask-action-notes">
          {d.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}

      <div className="ask-action-row ask-action-buttons">
        <button className="button is-primary" disabled={busy || problem !== null} title={problem ?? "Write it to the note (Enter)"}>
          {busy ? "adding…" : "add task"}
        </button>
        <button type="button" className="button" onClick={onCancel} disabled={busy}>
          {compact ? "back" : "dismiss"}
        </button>
        {onAsk && (
          <button type="button" className="link-button" onClick={onAsk} disabled={busy}>
            ask it as a question instead
          </button>
        )}
        <span className="ask-action-src">{problem ?? (d.source === "model" ? "read by the local model, checked by HQ" : "read by HQ's rules (no model)")}</span>
      </div>
      {error && <p className="is-error">{error}</p>}
    </form>
  );
}
