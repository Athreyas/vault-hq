// Interim tasks view, moved off the v1 HTTP API onto api.ts so the app builds. Stream B
// owns Tasks* and replaces this with the full list, filters and stats.
import { useState } from "react";
import type { Notify } from "../App.tsx";
import { api, iso, noteUrl, useCall, type Priority } from "../api.ts";
import Problem from "./Problem.tsx";
import { RichLine } from "./Rich.tsx";

const PRIO: Record<Priority, string> = { highest: "🔺", high: "⏫", medium: "🔼", low: "🔽", lowest: "⏬" };

export default function Tasks({ notify, onChange }: { notify: Notify; onChange: () => void }) {
  const { data, error, reload } = useCall(() => api.tasks({ status: ["pending", "started"], group: "project", sort: "urgency" }), "tasks");
  const [draft, setDraft] = useState("");
  const today = iso(new Date());

  async function act(label: string, fn: () => Promise<unknown>) {
    try {
      await fn();
      notify(label);
      reload();
      onChange();
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  async function add(e: React.FormEvent) {
    e.preventDefault();
    if (!draft.trim()) return;
    await act(`Added: ${draft.trim()}`, () => api.taskAdd({ description: draft.trim() }));
    setDraft("");
  }

  if (error) return <Problem error={error} what="tasks" />;
  return (
    <section className="tasks">
      <form className="task-add" onSubmit={add}>
        <label className="sr-only" htmlFor="task-desc">
          New task
        </label>
        <input id="task-desc" value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Add a task" required />
        <button className="button is-primary" type="submit">
          Add task
        </button>
      </form>

      {!data && <p className="empty">Reading tasks…</p>}
      {data && data.groups.length === 0 && <p className="empty">No open tasks.</p>}
      <div className="task-groups">
        {data?.groups.map((g) => (
          <section key={g.key} className="task-group">
            <h2>
              {g.label} <span className="count">{g.tasks.length}</span>
            </h2>
            <ul>
              {g.tasks.map((t) => (
                <li key={t.id} className="task">
                  <button className="task-check" aria-label={`Mark done: ${t.description}`} onClick={() => void act(`Done: ${t.description}`, () => api.taskDone(t.id))} />
                  <span className="task-desc">
                    <RichLine text={t.description} />
                    <a className="note-link task-note" href={noteUrl(t.note)}>
                      {t.note.replace(/^.*[/:]/, "").replace(/\.md$/, "")}
                    </a>
                  </span>
                  {t.priority && <span className="prio">{PRIO[t.priority]}</span>}
                  <input
                    type="date"
                    className={`task-due ${t.due && t.due < today ? "is-late" : ""} ${t.due ? "" : "is-empty"}`}
                    value={t.due ?? ""}
                    aria-label={`Due date for ${t.description}`}
                    onChange={(e) => void act("Due date saved", () => api.taskUpdate(t.id, { due: e.target.value || undefined }))}
                  />
                  <button className="task-drop" aria-label={`Delete: ${t.description}`} onClick={() => void act(`Deleted: ${t.description}`, () => api.taskDelete(t.id))}>
                    Drop
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </section>
  );
}
