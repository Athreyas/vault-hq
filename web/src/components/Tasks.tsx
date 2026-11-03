// The tasks view: a keyboard-first list over the vault's checkbox lines (SPEC 4).
// Keys (when not typing): j/k move · x done · s start/stop · c cancel · D D delete · e edit
// a annotate · enter details · o open note · n add · / filter · g group · v stats · ? help.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Notify } from "../App.tsx";
import { emptyQuery, onNotesChanged, taskAdd, taskAnnotate, taskCancel, taskDelete, taskDone, taskStart, tasksQuery, taskUpdate } from "../api/tasks.ts";
import type { Task } from "../bindings/Task.ts";
import type { TaskGroupBy } from "../bindings/TaskGroupBy.ts";
import type { TaskGroups } from "../bindings/TaskGroups.ts";
import type { TaskSort } from "../bindings/TaskSort.ts";
import "./Tasks.css";
import TasksStats from "./TasksStats.tsx";
import { addDays, parseFilter, parseQuickAdd, patchFrom, taskToSyntax } from "./TasksSyntax.ts";

const GROUPS: { key: TaskGroupBy; label: string }[] = [
  { key: "none", label: "none" },
  { key: "due", label: "due" },
  { key: "project", label: "project" },
  { key: "tag", label: "tag" },
  { key: "priority", label: "priority" },
  { key: "note", label: "note" },
  { key: "status", label: "status" },
];
const SORTS: TaskSort[] = ["urgency", "due", "priority", "created", "project"];
const PRIO = { highest: "H+", high: "H", medium: "M", low: "L", lowest: "L-" } as const;
const BOX = { pending: "[ ]", started: "[/]", waiting: "[>]", done: "[x]", cancelled: "[-]" } as const;

function localToday(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

const typing = (t: EventTarget | null) => t instanceof HTMLElement && Boolean(t.closest("input, textarea, select, [contenteditable]"));

function readPref<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(`hq.tasks.${key}`);
    return v && (allowed as readonly string[]).includes(v) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

function writePref(key: string, value: string) {
  try {
    localStorage.setItem(`hq.tasks.${key}`, value);
  } catch {
    // a remembered choice is a convenience only
  }
}

/** `11-06`, or a word for the days next to today. */
function shortDate(d: string, today: string): string {
  if (d === today) return "today";
  if (d === addDays(today, 1)) return "tomorrow";
  if (d === addDays(today, -1)) return "yesterday";
  return d.slice(0, 4) === today.slice(0, 4) ? d.slice(5) : d;
}

interface Props {
  notify: Notify;
  onChange: () => void;
  /** Open a note (by note id) in the app's reader, at a line. */
  onOpenNote?: (note: string, line: number) => void;
  /** Select this task (by key) once it is listed: a clicked reminder. */
  focus?: string | null;
}

export default function Tasks({ notify, onChange, onOpenNote, focus }: Props) {
  const today = localToday();
  const [filter, setFilter] = useState("");
  const [group, setGroup] = useState<TaskGroupBy>(() => readPref("group", GROUPS.map((g) => g.key), "due"));
  const [sort, setSort] = useState<TaskSort>(() => readPref("sort", SORTS, "urgency"));
  const [view, setView] = useState<"list" | "stats">("list");
  const [data, setData] = useState<TaskGroups | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [sel, setSel] = useState(0);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<{ key: string; text: string } | null>(null);
  const [annotating, setAnnotating] = useState<{ key: string; text: string } | null>(null);
  const [armedDelete, setArmedDelete] = useState<string | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [help, setHelp] = useState(false);
  const addRef = useRef<HTMLInputElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const parsedFilter = useMemo(() => parseFilter(filter, today), [filter, today]);
  const quick = useMemo(() => parseQuickAdd(draft, today), [draft, today]);
  const reload = useCallback(() => setTick((n) => n + 1), []);

  // an agent, an editor or a sync changed a note: the engine has re-read it, so ask again
  useEffect(() => onNotesChanged(reload), [reload]);

  useEffect(() => {
    let live = true;
    const timer = window.setTimeout(() => {
      tasksQuery({ ...emptyQuery(), ...parsedFilter.query, group, sort })
        .then((g) => live && (setData(g), setError(null)))
        .catch((e: Error) => live && setError(e.message));
    }, 90);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [parsedFilter, group, sort, tick]);

  // one row per (group, task): a task with two tags shows twice when grouped by tag
  const rows = useMemo(() => (data?.groups ?? []).flatMap((g) => g.tasks.map((t) => ({ group: g.key, task: t }))), [data]);
  const current = rows[Math.min(sel, rows.length - 1)]?.task;

  // a reminder was clicked: select its task (once per focus, when it is in the list)
  const focused = useRef<string | null>(null);
  useEffect(() => {
    if (!focus || focused.current === focus) return;
    const i = rows.findIndex((r) => r.task.key === focus || r.task.id === focus);
    if (i >= 0) {
      focused.current = focus;
      setSel(i);
    }
  }, [focus, rows]);

  useEffect(() => {
    listRef.current?.querySelector(".tk-row.is-selected")?.scrollIntoView({ block: "nearest" });
  }, [sel, rows.length]);

  const act = useCallback(
    async (label: string, fn: () => Promise<unknown>) => {
      try {
        await fn();
        notify(label);
        reload();
        onChange();
      } catch (e) {
        notify((e as Error).message, "error");
        reload();
      }
    },
    [notify, onChange, reload],
  );

  async function add(e: React.FormEvent) {
    e.preventDefault();
    if (quick.errors.length) {
      notify(`Not understood: ${quick.errors.join(" ")}`, "error");
      return;
    }
    if (!quick.input.description) return;
    const input = quick.input;
    await act(`Added: ${input.description}`, () => taskAdd(input));
    setDraft("");
  }

  async function saveEdit(t: Task, text: string) {
    const parsed = parseQuickAdd(text, today);
    if (parsed.errors.length) {
      notify(`Not understood: ${parsed.errors.join(" ")}`, "error");
      return;
    }
    const patch = patchFrom(t, parsed);
    setEditing(null);
    listRef.current?.focus();
    if (Object.keys(patch).length === 0) return;
    await act("Saved", () => taskUpdate(t.key, patch));
  }

  async function saveAnnotation(t: Task, text: string) {
    setAnnotating(null);
    listRef.current?.focus();
    if (!text.trim()) return;
    await act("Annotated", () => taskAnnotate(t.key, text.trim()));
    setOpen((o) => new Set(o).add(t.key));
  }

  const done = (t: Task) => act(t.recur ? `Done; next one added (${t.recur})` : `Done: ${t.description}`, () => taskDone(t.key));
  const start = (t: Task) => act(t.status === "started" ? `Stopped: ${t.description}` : `Started: ${t.description}`, () => taskStart(t.key));
  const cancel = (t: Task) => act(`Cancelled: ${t.description}`, () => taskCancel(t.key));
  const remove = (t: Task) => {
    if (armedDelete !== t.key) {
      setArmedDelete(t.key);
      notify("Press D again to delete the line from its note.");
      return;
    }
    setArmedDelete(null);
    void act(`Deleted: ${t.description}`, () => taskDelete(t.key));
  };
  const toggle = (t: Task) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(t.key)) n.delete(t.key);
      else n.add(t.key);
      return n;
    });

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey || typing(e.target)) return;
      const k = e.key;
      const t = current;
      const move = (d: number) => {
        e.preventDefault();
        setArmedDelete(null);
        setSel((s) => Math.max(0, Math.min(rows.length - 1, s + d)));
      };
      switch (k) {
        case "j":
        case "ArrowDown":
          return move(1);
        case "k":
        case "ArrowUp":
          return move(-1);
        case "G":
          return move(rows.length);
        case "/":
          e.preventDefault();
          return filterRef.current?.focus();
        case "n":
        case "i":
          e.preventDefault();
          return addRef.current?.focus();
        case "g": {
          const i = GROUPS.findIndex((g) => g.key === group);
          const next = GROUPS[(i + 1) % GROUPS.length]!.key;
          writePref("group", next);
          return setGroup(next);
        }
        case "v":
          return setView((v) => (v === "list" ? "stats" : "list"));
        case "?":
          return setHelp((h) => !h);
        case "Escape":
          setArmedDelete(null);
          return setHelp(false);
      }
      if (!t || view !== "list") return;
      switch (k) {
        case "x":
        case " ":
          e.preventDefault();
          return void done(t);
        case "s":
          return void start(t);
        case "c":
          return void cancel(t);
        case "D":
          return remove(t);
        case "e":
          e.preventDefault();
          return setEditing({ key: t.key, text: taskToSyntax(t) });
        case "a":
          e.preventDefault();
          return setAnnotating({ key: t.key, text: "" });
        case "Enter":
          return toggle(t);
        case "o":
          return onOpenNote?.(t.note, t.line);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const nowHm = new Date().toTimeString().slice(0, 5);
  const late = (t: Task) =>
    t.due !== null && (t.due < today || (t.due === today && t.dueTime !== null && t.dueTime < nowHm)) && (t.status === "pending" || t.status === "started" || t.status === "waiting");
  let flat = -1;

  return (
    <section className="tk">
      <form className="tk-add" onSubmit={add}>
        <label className="tk-prompt" htmlFor="tk-add">
          add&gt;
        </label>
        <input
          id="tk-add"
          ref={addRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && (setDraft(""), listRef.current?.focus())}
          placeholder="Fix the gate #home +house due:fri p:h rec:weekly"
          autoComplete="off"
          spellCheck={false}
        />
        {draft && (
          <div className="tk-preview" aria-live="polite">
            {quick.input.description ? <span className="tk-chip">{quick.input.description}</span> : <span className="tk-chip is-bad">needs a description</span>}
            {quick.input.tags.map((x) => (
              <span key={x} className="tk-chip is-tag">
                #{x}
              </span>
            ))}
            {quick.input.project && <span className="tk-chip">+{quick.input.project}</span>}
            {quick.input.priority && <span className="tk-chip">priority {quick.input.priority}</span>}
            {quick.input.due && (
              <span className="tk-chip">
                due {quick.input.due}
                {quick.input.dueTime ? ` ${quick.input.dueTime}` : ""}
              </span>
            )}
            {quick.input.scheduled && (
              <span className="tk-chip">
                {quick.input.waiting ? "wait until" : "scheduled"} {quick.input.scheduled}
              </span>
            )}
            {quick.input.start && <span className="tk-chip">start {quick.input.start}</span>}
            {quick.input.recur && <span className="tk-chip">{quick.input.recur}</span>}
            {quick.input.depends.length > 0 && <span className="tk-chip">after {quick.input.depends.join(", ")}</span>}
            {quick.errors.map((x) => (
              <span key={x} className="tk-chip is-bad">
                ? {x}
              </span>
            ))}
          </div>
        )}
      </form>

      <div className="tk-bar">
        <label className="tk-prompt" htmlFor="tk-filter">
          filter&gt;
        </label>
        <input
          id="tk-filter"
          ref={filterRef}
          className={parsedFilter.errors.length ? "is-bad" : ""}
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value);
            setSel(0);
          }}
          onKeyDown={(e) => (e.key === "Escape" || e.key === "Enter") && listRef.current?.focus()}
          placeholder="+tag -tag project:x due:eow p:h status:done +OVERDUE words"
          list="tk-filter-words"
          autoComplete="off"
          spellCheck={false}
        />
        <datalist id="tk-filter-words">
          {(data?.projects ?? []).map((p) => (
            <option key={`p${p}`} value={`project:${p}`} />
          ))}
          {(data?.tags ?? []).map((t) => (
            <option key={`t${t}`} value={`+${t}`} />
          ))}
        </datalist>
        <div className="segmented" role="group" aria-label="Group by">
          {GROUPS.map((g) => (
            <button
              key={g.key}
              className={group === g.key ? "is-active" : ""}
              aria-pressed={group === g.key}
              onClick={() => {
                writePref("group", g.key);
                setGroup(g.key);
              }}
            >
              {g.label}
            </button>
          ))}
        </div>
        <select
          aria-label="Sort by"
          value={sort}
          onChange={(e) => {
            const s = e.target.value as TaskSort;
            writePref("sort", s);
            setSort(s);
          }}
        >
          {SORTS.map((s) => (
            <option key={s} value={s}>
              sort: {s}
            </option>
          ))}
        </select>
        <div className="segmented" role="group" aria-label="View">
          <button className={view === "list" ? "is-active" : ""} aria-pressed={view === "list"} onClick={() => setView("list")}>
            list
          </button>
          <button className={view === "stats" ? "is-active" : ""} aria-pressed={view === "stats"} onClick={() => setView("stats")}>
            stats
          </button>
        </div>
      </div>

      {help && (
        <p className="tk-help">
          <kbd>j</kbd>/<kbd>k</kbd> move · <kbd>x</kbd> done · <kbd>s</kbd> start/stop · <kbd>c</kbd> cancel · <kbd>D</kbd>
          <kbd>D</kbd> delete · <kbd>e</kbd> edit · <kbd>a</kbd> annotate · <kbd>enter</kbd> details · <kbd>o</kbd> open note · <kbd>n</kbd> add · <kbd>/</kbd>{" "}
          filter · <kbd>g</kbd> group · <kbd>v</kbd> stats
        </p>
      )}

      {view === "stats" && <TasksStats project={parsedFilter.query.project ?? null} tag={parsedFilter.query.tagsAll?.[0] ?? null} />}

      {view === "list" && (
        <div className="tk-list" ref={listRef} tabIndex={0} aria-label="Tasks" aria-activedescendant={current ? `tk-${sel}` : undefined}>
          {error && <p className="empty">Could not read tasks: {error}</p>}
          {!data && !error && <p className="empty">Reading tasks…</p>}
          {data && rows.length === 0 && <p className="empty">{filter ? "No tasks match this filter." : "No open tasks. Add one above."}</p>}
          {data?.groups.map((g) => (
            <section key={g.key} className="tk-group">
              {group !== "none" && (
                <h2 className={g.key === "overdue" ? "is-late" : ""}>
                  {g.label} <span className="count">{g.tasks.length}</span>
                </h2>
              )}
              <ul>
                {g.tasks.map((t) => {
                  flat += 1;
                  const i = flat;
                  const selected = i === Math.min(sel, rows.length - 1);
                  const isOpen = open.has(t.key);
                  return (
                    <li
                      key={`${g.key}/${t.key}`}
                      id={`tk-${i}`}
                      className={`tk-row is-${t.status} ${selected ? "is-selected" : ""} ${t.blocked ? "is-blocked" : ""}`}
                      onClick={() => setSel(i)}
                      onDoubleClick={() => setEditing({ key: t.key, text: taskToSyntax(t) })}
                    >
                      <button
                        className="tk-box"
                        aria-label={`Mark done: ${t.description}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          void done(t);
                        }}
                      >
                        {BOX[t.status]}
                      </button>
                      <span className={`tk-prio ${t.priority ? `is-${t.priority}` : ""}`}>{t.priority ? PRIO[t.priority] : ""}</span>
                      {editing?.key === t.key ? (
                        <input
                          className="tk-edit"
                          autoFocus
                          value={editing.text}
                          aria-label={`Edit ${t.description}`}
                          spellCheck={false}
                          onChange={(e) => setEditing({ key: t.key, text: e.target.value })}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") void saveEdit(t, editing.text);
                            if (e.key === "Escape") {
                              setEditing(null);
                              listRef.current?.focus();
                            }
                          }}
                          onBlur={() => setEditing(null)}
                        />
                      ) : (
                        <span className="tk-desc">
                          {t.description}
                          {t.tags.map((x) => (
                            <span key={x} className="tk-tag">
                              #{x}
                            </span>
                          ))}
                          {t.project && group !== "project" && <span className="tk-project">+{t.project}</span>}
                        </span>
                      )}
                      <span className="tk-flags">
                        {t.recur && <span title={t.recur}>↻</span>}
                        {t.blocked && <span title="Waiting on another task">blocked</span>}
                        {t.blocking && <span title="Another task waits on this">blocking</span>}
                        {t.annotations.length > 0 && <span title="Annotations">+{t.annotations.length}</span>}
                        {t.status === "waiting" && t.scheduled && <span>until {shortDate(t.scheduled, today)}</span>}
                      </span>
                      <span className={`tk-due ${late(t) ? "is-late" : ""}`}>{t.due ? shortDate(t.due, today) : ""}
                        {t.due && t.dueTime ? <span className="tk-time"> {t.dueTime}</span> : null}
                      </span>
                      <span className="tk-urg" title="Urgency">
                        {t.status === "done" || t.status === "cancelled" ? (t.done ?? t.cancelled ?? "").slice(5) : t.urgency.toFixed(1)}
                      </span>
                      {(isOpen || annotating?.key === t.key) && (
                        <div className="tk-detail">
                          {t.annotations.map((a, j) => (
                            <p key={j} className="tk-ann">
                              <span className="tk-ann-date">{a.date ?? ""}</span> {a.text}
                            </p>
                          ))}
                          {annotating?.key === t.key && (
                            <input
                              className="tk-edit"
                              autoFocus
                              placeholder="annotation, Enter to add"
                              value={annotating.text}
                              aria-label={`Annotate ${t.description}`}
                              onChange={(e) => setAnnotating({ key: t.key, text: e.target.value })}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") void saveAnnotation(t, annotating.text);
                                if (e.key === "Escape") {
                                  setAnnotating(null);
                                  listRef.current?.focus();
                                }
                              }}
                              onBlur={() => setAnnotating(null)}
                            />
                          )}
                          {isOpen && (
                            <p className="tk-meta">
                              {t.id && <span>id {t.id}</span>}
                              {t.created && <span>added {t.created}</span>}
                              {t.start && <span>start {t.start}</span>}
                              {t.scheduled && <span>scheduled {t.scheduled}</span>}
                              {t.depends.length > 0 && <span>after {t.depends.join(", ")}</span>}
                              <button className="link-button" onClick={() => onOpenNote?.(t.note, t.line)} disabled={!onOpenNote}>
                                {t.noteTitle}:{t.line}
                              </button>
                            </p>
                          )}
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
          {data && data.total > rows.length && group !== "tag" && <p className="empty">{data.total - rows.length} more not shown.</p>}
        </div>
      )}
      <p className="tk-foot">
        {data ? `${data.total} task${data.total === 1 ? "" : "s"}` : ""} · <kbd>?</kbd> keys
      </p>
    </section>
  );
}
