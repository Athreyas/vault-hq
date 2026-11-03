import { useMemo, useState } from "react";
import type { TwTask } from "../../../server/types.ts";
import type { Notify } from "../App.tsx";
import { addDays, iso, noteUrl, post, useData } from "../api.ts";
import { RichLine } from "./Rich.tsx";

const FILTERS = [
  { key: "all", label: "All" },
  { key: "soon", label: "Due this week" },
  { key: "high", label: "High priority" },
] as const;
type Filter = (typeof FILTERS)[number]["key"];

const PRIO_LABEL = { H: "High", M: "Medium", L: "Low" } as const;

export default function Tasks({ notify, onChange }: { notify: Notify; onChange: () => void }) {
  const { data, error, reload } = useData<TwTask[]>("/api/tasks");
  const [filter, setFilter] = useState<Filter>("all");
  const [draft, setDraft] = useState({ description: "", project: "", due: "", priority: "" });
  const today = iso(new Date());

  const projects = useMemo(() => [...new Set((data ?? []).map((t) => t.project).filter((p): p is string => Boolean(p)))].sort(), [data]);
  const groups = useMemo(() => {
    const week = addDays(today, 7);
    const shown = (data ?? []).filter((t) => (filter === "soon" ? t.due !== null && t.due <= week : filter === "high" ? t.priority === "H" : true));
    const map = new Map<string, TwTask[]>();
    for (const t of shown) {
      const key = t.project ?? (t.note ? t.note.replace(/^.*\//, "").replace(/\.md$/, "") : "No project");
      map.set(key, [...(map.get(key) ?? []), t]);
    }
    return [...map.entries()];
  }, [data, filter, today]);

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
    const body = Object.fromEntries(Object.entries(draft).filter(([, v]) => v));
    await act(`Added: ${draft.description}`, () => post("/api/tasks/add", body));
    setDraft({ description: "", project: draft.project, due: "", priority: "" });
  }

  if (error) return <p className="empty">Could not read Taskwarrior: {error}</p>;
  return (
    <section className="tasks">
      <form className="task-add" onSubmit={add}>
        <label className="sr-only" htmlFor="task-desc">
          New task
        </label>
        <input id="task-desc" value={draft.description} onChange={(e) => setDraft({ ...draft, description: e.target.value })} placeholder="Add a task" required />
        <select value={draft.project} onChange={(e) => setDraft({ ...draft, project: e.target.value })} aria-label="Project">
          <option value="">Tasks inbox</option>
          {projects.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <input type="date" value={draft.due} onChange={(e) => setDraft({ ...draft, due: e.target.value })} aria-label="Due date" />
        <select value={draft.priority} onChange={(e) => setDraft({ ...draft, priority: e.target.value })} aria-label="Priority">
          <option value="">No priority</option>
          <option value="H">High</option>
          <option value="M">Medium</option>
          <option value="L">Low</option>
        </select>
        <button className="button is-primary" type="submit">
          Add task
        </button>
      </form>

      <div className="segmented" role="group" aria-label="Filter">
        {FILTERS.map((f) => (
          <button key={f.key} className={filter === f.key ? "is-active" : ""} onClick={() => setFilter(f.key)} aria-pressed={filter === f.key}>
            {f.label}
          </button>
        ))}
      </div>

      {!data && <p className="empty">Reading Taskwarrior…</p>}
      {data && groups.length === 0 && <p className="empty">No tasks match this filter.</p>}
      <div className="task-groups">
        {groups.map(([group, list]) => (
          <section key={group} className="task-group">
            <h2>
              {group} <span className="count">{list.length}</span>
            </h2>
            <ul>
              {list.map((t) => {
                const late = t.due !== null && t.due < today;
                return (
                  <li key={t.uuid} className="task">
                    <button className="task-check" aria-label={`Mark done: ${t.description}`} onClick={() => act(`Done: ${t.description}`, () => post(`/api/tasks/${t.uuid}/done`, {}))} />
                    <span className="task-desc">
                      <RichLine text={t.description} />
                      {t.note && (
                        <a className="note-link task-note" href={noteUrl(t.note)}>
                          {t.note.replace(/^.*\//, "").replace(/\.md$/, "")}
                        </a>
                      )}
                    </span>
                    {t.priority && <span className={`prio prio-${t.priority}`}>{PRIO_LABEL[t.priority]}</span>}
                    <input
                      type="date"
                      className={`task-due ${late ? "is-late" : ""} ${t.due ? "" : "is-empty"}`}
                      value={t.due ?? ""}
                      aria-label={`Due date for ${t.description}`}
                      onChange={(e) => act("Due date saved", () => post(`/api/tasks/${t.uuid}/modify`, { due: e.target.value || null }))}
                    />
                    <button className="task-drop" aria-label={`Drop: ${t.description}`} onClick={() => act(`Dropped: ${t.description}`, () => post(`/api/tasks/${t.uuid}/delete`, {}))}>
                      Drop
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </div>
    </section>
  );
}
