import { useEffect, useState } from "react";
import type { Day } from "../../../server/types.ts";
import type { Notify } from "../App.tsx";
import { addDays, iso, noteUrl, parseDate, post, useData } from "../api.ts";
import { RichLine } from "./Rich.tsx";
import Sparkline from "./Sparkline.tsx";


const SPANS = [
  { span: 1, label: "day" },
  { span: 3, label: "3 days" },
  { span: 7, label: "week" },
] as const;

export default function Days({ date, span, notify }: { date: string; span: 1 | 3 | 7; notify: Notify }) {
  // One fetch feeds the calendar strip, the sparklines and the columns: 14 days up to the
  // selected date, plus 3 ahead for what is due.
  const all = useData<Day[]>(`/api/days?from=${addDays(date, -13)}&span=17`, 60_000);
  // core metrics plus any this machine's extension adds (server /api/metrics)
  const metrics = useData<{ key: string; label: string; unit?: string }[]>("/api/metrics", 3_600_000);
  const today = iso(new Date());
  const from = addDays(date, -(span - 1)); // views end on the selected date: the past is where the data is
  const step = (n: number) => `#/days/${addDays(date, n * span)}/${span}`;
  const data = all.data ?? [];
  const strip = data.filter((d) => d.date >= addDays(date, -10));
  const shown = data.filter((d) => d.date >= from && d.date <= date);
  const trend = data.filter((d) => d.date <= date);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea, select") || e.ctrlKey || e.metaKey) return;
      if (e.key === "[") window.location.hash = step(-1);
      if (e.key === "]") window.location.hash = step(1);
      if (e.key === "t") window.location.hash = `#/days/${today}/${span}`;
      if (e.key === "s") window.location.hash = `#/days/${date}/${span === 1 ? 3 : span === 3 ? 7 : 1}`;
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <section className="days">
      <header className="days-head">
        <div className="segmented" role="group" aria-label="Range">
          {SPANS.map((s) => (
            <a key={s.span} href={`#/days/${date}/${s.span}`} className={span === s.span ? "is-active" : ""} aria-current={span === s.span ? "true" : undefined}>
              {s.label}
            </a>
          ))}
        </div>
        <div className="days-nav">
          <a className="button" href={step(-1)} aria-label="Earlier" title="Earlier ( [ )">
            ‹ earlier
          </a>
          <a className="button" href={`#/days/${today}/${span}`} aria-label="Today" title="Today ( t )">
            today
          </a>
          <a className="button" href={step(1)} aria-label="Later" title="Later ( ] )">
            later ›
          </a>
        </div>
        <p className="days-range">
          {span === 1 ? date : `${from} to ${date}`}
        </p>
      </header>

      <ol className="strip" aria-label="Calendar">
        {strip.map((d) => {
          const dt = parseDate(d.date);
          const past = d.date <= today;
          const marks = Math.min(past ? d.agentLog.length + d.completed.length : d.dueTasks, 6);
          const inRange = d.date >= from && d.date <= date;
          return (
            <li key={d.date}>
              <a
                href={`#/days/${d.date}/${span}`}
                className={`strip-day ${inRange ? "is-in-range" : ""} ${d.date === date ? "is-end" : ""} ${d.date === today ? "is-today" : ""}`}
                aria-current={d.date === date ? "date" : undefined}
                title={`${d.date}: ${past ? `${d.agentLog.length} agent actions, ${d.completed.length} tasks done` : `${d.dueTasks} tasks due`}`}
              >
                <span className="strip-name">{dt.toLocaleDateString("en-US", { weekday: "short" })}</span>
                <span className="strip-num">{dt.getDate()}</span>
                <span className={`strip-marks ${past ? "" : "is-due"}`} aria-hidden="true">
                  {marks > 0 ? "▪".repeat(marks) : "·"}
                </span>
              </a>
            </li>
          );
        })}
      </ol>

      <div className="sparks" aria-label="Last 14 days">
        {(metrics.data ?? []).map((m) => (
          <Sparkline key={m.key} label={m.label} unit={m.unit} points={trend.slice(-14).map((d) => ({ date: d.date, value: typeof d.metrics[m.key] === "number" ? (d.metrics[m.key] as number) : null }))} />
        ))}
      </div>

      {all.error && <p className="empty">Could not load these days: {all.error}</p>}
      <div className={`day-columns span-${span}`}>
        {shown.map((d) => (
          <DayColumn key={d.date} day={d} compact={span === 7} isToday={d.date === today} notify={notify} onSaved={all.reload} />
        ))}
      </div>
    </section>
  );
}

// One line typed by the user into their own section of the daily note.
function AddLine({ date, section, notify, onSaved }: { date: string; section: string; notify: Notify; onSaved: () => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!text.trim()) return;
    setBusy(true);
    try {
      await post("/api/day/add", { date, section, text });
      setText("");
      onSaved();
    } catch (err) {
      notify((err as Error).message, "error");
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="add-line" onSubmit={save}>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={`Add to ${section.toLowerCase()}`}
        aria-label={`Add a line to ${section} for ${date}`}
        disabled={busy}
      />
    </form>
  );
}

function DayColumn({ day, compact, isToday, notify, onSaved }: { day: Day; compact: boolean; isToday: boolean; notify: Notify; onSaved: () => void }) {
  const date = parseDate(day.date);
  const written = day.human.some((s) => s.lines.length);
  const nothing = !written && !day.agentLog.length && !day.completed.length && !day.captures.length;
  return (
    <article className={`day ${isToday ? "is-today" : ""}`}>
      <header className="day-head">
        <h2>
          {date.toLocaleDateString("en-US", { weekday: compact ? "short" : "long" })} <span>{date.getDate()}</span>
        </h2>
        {day.exists && (
          <a className="note-link" href={noteUrl(`09-Planning/Daily/${day.date}.md`)}>
            Open note
          </a>
        )}
      </header>

      {nothing && compact && <p className="day-empty">{day.exists ? "Nothing logged." : "No daily note for this day."}</p>}

      {day.human.filter((s) => !compact || s.lines.length).map((s) => (
        <div key={s.heading} className="day-block is-human">
          <h3>{s.heading}</h3>
          {s.lines.length > 0 && (
            <ul>
              {s.lines.map((l, i) => (
                <li key={i}>
                  <RichLine text={l} />
                </li>
              ))}
            </ul>
          )}
          {!compact && <AddLine date={day.date} section={s.heading} notify={notify} onSaved={onSaved} />}
        </div>
      ))}

      {day.completed.length > 0 && (
        <div className="day-block">
          <h3>Completed</h3>
          <ul className="done-list">
            {day.completed.map((t, i) => (
              <li key={i}>
                <RichLine text={t.desc} />
              </li>
            ))}
          </ul>
        </div>
      )}

      {day.agentLog.length > 0 && (
        <div className="day-block">
          <h3>Agent log</h3>
          <ol className="log">
            {day.agentLog.map((a, i) => (
              <li key={i}>
                {a.time && <time>{a.time}</time>}
                <span>
                  <RichLine text={a.text} />
                </span>
              </li>
            ))}
          </ol>
        </div>
      )}

      {day.captures.length > 0 && (
        <div className="day-block">
          <h3>Captured</h3>
          <ul>
            {day.captures.map((c) => (
              <li key={c.rel}>
                <a className="note-link" href={noteUrl(c.rel)}>
                  {c.title}
                </a>
              </li>
            ))}
          </ul>
        </div>
      )}
    </article>
  );
}
