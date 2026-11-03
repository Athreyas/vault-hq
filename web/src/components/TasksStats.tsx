// Task statistics: headline numbers, burn-down (open and overdue over time), done per day,
// and breakdowns. Charts are plain SVG; colours come from tokens via Tasks.css classes.
import { useEffect, useState } from "react";
import { tasksStats } from "../api/tasks.ts";
import type { DayStat } from "../bindings/DayStat.ts";
import type { StatBreakdown } from "../bindings/StatBreakdown.ts";
import type { TaskStats } from "../bindings/TaskStats.ts";

const RANGES = [14, 30, 90, 365] as const;
const W = 640;
const H = 160;
const PAD = { l: 34, r: 8, t: 10, b: 20 };

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** A round axis maximum and 2–4 tick values under it. */
function ticks(max: number): number[] {
  if (max <= 0) return [0];
  const step = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000].find((s) => max / s <= 4) ?? Math.ceil(max / 4);
  const out: number[] = [];
  for (let v = 0; v <= max; v += step) out.push(v);
  if (out[out.length - 1]! < max) out.push(out[out.length - 1]! + step);
  return out;
}

function useHover(n: number) {
  const [i, setI] = useState<number | null>(null);
  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * W;
    const f = (x - PAD.l) / (W - PAD.l - PAD.r);
    setI(Math.max(0, Math.min(n - 1, Math.round(f * (n - 1)))));
  };
  return { i, onMove, onLeave: () => setI(null) };
}

function Axis({ top, days }: { top: number[]; days: DayStat[] }) {
  const max = top[top.length - 1]!;
  const y = (v: number) => PAD.t + (1 - v / (max || 1)) * (H - PAD.t - PAD.b);
  const first = days[0]?.date.slice(5) ?? "";
  const last = days[days.length - 1]?.date.slice(5) ?? "";
  return (
    <g className="tk-axis">
      {top.map((v) => (
        <g key={v}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} className="tk-grid" />
          <text x={PAD.l - 6} y={y(v) + 4} textAnchor="end">
            {v}
          </text>
        </g>
      ))}
      <text x={PAD.l} y={H - 4}>
        {first}
      </text>
      <text x={W - PAD.r} y={H - 4} textAnchor="end">
        {last}
      </text>
    </g>
  );
}

function BurnDown({ days }: { days: DayStat[] }) {
  const n = days.length;
  const top = ticks(Math.max(1, ...days.map((d) => d.open)));
  const max = top[top.length - 1]!;
  const x = (i: number) => PAD.l + (n <= 1 ? 0 : (i / (n - 1)) * (W - PAD.l - PAD.r));
  const y = (v: number) => PAD.t + (1 - v / max) * (H - PAD.t - PAD.b);
  const path = (k: "open" | "overdue") => days.map((d, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(d[k]).toFixed(1)}`).join("");
  const hover = useHover(n);
  const h = hover.i === null ? null : days[hover.i];
  const lastDay = days[n - 1];
  return (
    <figure className="tk-chart">
      <figcaption>
        Burn-down <span className="tk-key is-open">open</span> <span className="tk-key is-overdue">overdue</span>
      </figcaption>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Open and overdue tasks per day" onMouseMove={hover.onMove} onMouseLeave={hover.onLeave}>
        <Axis top={top} days={days} />
        <path d={path("open")} className="tk-line is-open" />
        <path d={path("overdue")} className="tk-line is-overdue" />
        {lastDay && (
          <>
            <text x={W - PAD.r - 2} y={y(lastDay.open) - 6} textAnchor="end" className="tk-label">
              {lastDay.open} open
            </text>
            {lastDay.overdue > 0 && (
              <text x={W - PAD.r - 2} y={y(lastDay.overdue) - 6} textAnchor="end" className="tk-label">
                {lastDay.overdue} overdue
              </text>
            )}
          </>
        )}
        {h && hover.i !== null && (
          <g>
            <line x1={x(hover.i)} x2={x(hover.i)} y1={PAD.t} y2={H - PAD.b} className="tk-cross" />
            <circle cx={x(hover.i)} cy={y(h.open)} r={4} className="tk-dot is-open" />
            <circle cx={x(hover.i)} cy={y(h.overdue)} r={4} className="tk-dot is-overdue" />
          </g>
        )}
      </svg>
      <p className="tk-tip" aria-live="polite">
        {h ? `${h.date}: ${h.open} open, ${h.overdue} overdue` : " "}
      </p>
    </figure>
  );
}

function DonePerDay({ days }: { days: DayStat[] }) {
  const n = days.length;
  const top = ticks(Math.max(1, ...days.map((d) => d.done)));
  const max = top[top.length - 1]!;
  const span = (W - PAD.l - PAD.r) / Math.max(1, n);
  const gap = Math.min(2, span / 4);
  const y = (v: number) => PAD.t + (1 - v / max) * (H - PAD.t - PAD.b);
  const hover = useHover(n);
  const h = hover.i === null ? null : days[hover.i];
  return (
    <figure className="tk-chart">
      <figcaption>Done per day</figcaption>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Tasks done per day" onMouseMove={hover.onMove} onMouseLeave={hover.onLeave}>
        <Axis top={top} days={days} />
        {days.map((d, i) =>
          d.done > 0 ? (
            <rect
              key={d.date}
              x={PAD.l + i * span + gap / 2}
              y={y(d.done)}
              width={Math.max(1, span - gap)}
              height={H - PAD.b - y(d.done)}
              rx={Math.min(2, (span - gap) / 2)}
              className={`tk-bar-mark ${hover.i === i ? "is-hover" : ""}`}
            />
          ) : null,
        )}
      </svg>
      <p className="tk-tip" aria-live="polite">
        {h ? `${h.date}: ${h.done} done, ${h.created} added, ${h.cancelled} cancelled` : " "}
      </p>
    </figure>
  );
}

function Breakdown({ title, rows }: { title: string; rows: StatBreakdown[] }) {
  if (rows.length === 0) return null;
  return (
    <table className="tk-table">
      <caption>{title}</caption>
      <thead>
        <tr>
          <th scope="col"></th>
          <th scope="col">open</th>
          <th scope="col">overdue</th>
          <th scope="col">done</th>
          <th scope="col">cancelled</th>
        </tr>
      </thead>
      <tbody>
        {rows.slice(0, 12).map((r) => (
          <tr key={r.key}>
            <th scope="row">{r.key || "(none)"}</th>
            <td>{r.open}</td>
            <td className={r.overdue ? "is-late" : ""}>{r.overdue}</td>
            <td>{r.done}</td>
            <td>{r.cancelled}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export default function TasksStats({ project, tag }: { project: string | null; tag: string | null }) {
  const [days, setDays] = useState<(typeof RANGES)[number]>(30);
  const [stats, setStats] = useState<TaskStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asTable, setAsTable] = useState(false);

  useEffect(() => {
    let live = true;
    tasksStats({ days, project, tag, vaults: [], today: null })
      .then((s) => live && (setStats(s), setError(null)))
      .catch((e: Error) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [days, project, tag]);

  if (error) return <p className="empty">Could not read task stats: {error}</p>;
  if (!stats) return <p className="empty">Counting…</p>;
  const scope = [project && `+${project}`, tag && `#${tag}`].filter(Boolean).join(" ");
  return (
    <section className="tk-stats" aria-label="Task statistics">
      <div className="tk-bar">
        <span className="tk-prompt">stats{scope ? ` ${scope}` : ""}&gt;</span>
        <div className="segmented" role="group" aria-label="Range">
          {RANGES.map((r) => (
            <button key={r} className={days === r ? "is-active" : ""} aria-pressed={days === r} onClick={() => setDays(r)}>
              {r}d
            </button>
          ))}
        </div>
        <button className="button" onClick={() => setAsTable((t) => !t)} aria-pressed={asTable}>
          {asTable ? "charts" : "table"}
        </button>
      </div>
      <dl className="tk-tiles">
        <div>
          <dt>open</dt>
          <dd>{stats.open}</dd>
        </div>
        <div>
          <dt>overdue</dt>
          <dd className={stats.overdue ? "is-late" : ""}>{stats.overdue}</dd>
        </div>
        <div>
          <dt>done 7d / 30d</dt>
          <dd>
            {stats.done7} / {stats.done30}
          </dd>
        </div>
        <div>
          <dt>completion 7d / 30d</dt>
          <dd>
            {pct(stats.completion7)} / {pct(stats.completion30)}
          </dd>
        </div>
        <div>
          <dt>burn rate</dt>
          <dd>{stats.burnRate.toFixed(1)}/day</dd>
        </div>
        <div>
          <dt>net change</dt>
          <dd>
            {stats.netPerDay > 0 ? "+" : ""}
            {stats.netPerDay.toFixed(1)}/day
          </dd>
        </div>
        <div>
          <dt>zero by</dt>
          <dd>{stats.projectedZero ?? "—"}</dd>
        </div>
      </dl>
      {asTable ? (
        <table className="tk-table">
          <caption>Per day</caption>
          <thead>
            <tr>
              <th scope="col">date</th>
              <th scope="col">added</th>
              <th scope="col">done</th>
              <th scope="col">cancelled</th>
              <th scope="col">open</th>
              <th scope="col">overdue</th>
            </tr>
          </thead>
          <tbody>
            {[...stats.days].reverse().map((d) => (
              <tr key={d.date}>
                <th scope="row">{d.date}</th>
                <td>{d.created}</td>
                <td>{d.done}</td>
                <td>{d.cancelled}</td>
                <td>{d.open}</td>
                <td>{d.overdue}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="tk-charts">
          <BurnDown days={stats.days} />
          <DonePerDay days={stats.days} />
        </div>
      )}
      <div className="tk-breakdowns">
        <Breakdown title="By project" rows={stats.byProject} />
        <Breakdown title="By tag" rows={stats.byTag} />
        <Breakdown title="By priority" rows={stats.byPriority} />
      </div>
    </section>
  );
}
