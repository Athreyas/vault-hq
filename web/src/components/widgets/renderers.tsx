// How each widget kind draws its data: compact, glanceable, terminal-dense. Other streams
// can take over a kind (the memories widget, say) with `registerWidgetRenderer`.
import { useEffect, useState, type ComponentType } from "react";
import type { WidgetData } from "../../bindings/WidgetData.ts";
import type { WidgetInstance } from "../../bindings/WidgetInstance.ts";
import type { WidgetKind } from "../../bindings/WidgetKind.ts";
import type { WidgetTask } from "../../bindings/WidgetTask.ts";
import Memories from "../BookmarksMemories.tsx";
import { Bars, Meter, Spark } from "./charts.tsx";
import { ago, bytes, dueLabel, perDay, uptime, zoneTime } from "./layout.ts";

export interface RendererProps {
  instance: WidgetInstance;
  /** Null until the first answer arrives. */
  data: WidgetData | null;
}

const custom = new Map<WidgetKind, ComponentType<RendererProps>>();

/** Draw `kind` with your own component instead of the built-in one. */
export function registerWidgetRenderer(kind: WidgetKind, component: ComponentType<RendererProps>): void {
  custom.set(kind, component);
}

export function WidgetBody({ instance, data }: RendererProps) {
  const Custom = custom.get(instance.kind);
  if (Custom) return <Custom instance={instance} data={data} />;
  if (instance.kind === "clock") return <Clock instance={instance} data={data} />;
  // stream F's card: it fetches its own memories and handles dismissals
  if (instance.kind === "memories") return <Memories />;
  if (!data) return <p className="w-muted">…</p>;
  const b = data.body;
  switch (b.type) {
    case "system":
      return <System body={b} />;
    case "tasksBurn":
      return <Burn body={b} />;
    case "taskList":
      return <TaskList tasks={b.tasks} total={b.total} />;
    case "inbox":
      return (
        <div className="w-stack">
          <p className="w-big">{b.count}</p>
          {b.titles.length > 0 && (
            <ul className="w-lines">
              {b.titles.map((t, i) => (
                <li key={i} title={t}>
                  {t}
                </li>
              ))}
            </ul>
          )}
        </div>
      );
    case "jsonStatus":
      return <JsonStatus body={b} />;
    case "unavailable":
      return <p className="w-muted">{b.message}</p>;
    default:
      return null;
  }
}

type Body<T extends WidgetData["body"]["type"]> = Extract<WidgetData["body"], { type: T }>;

function useNow(everyMs: number): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), everyMs);
    return () => clearInterval(t);
  }, [everyMs]);
  return now;
}

function Clock({ instance, data }: RendererProps) {
  const c = instance.config;
  const seconds = c.seconds === true;
  const hour12 = c.hour12 === true;
  const now = useNow(seconds ? 1000 : 15_000);
  const zones = Array.isArray(c.zones) ? (c.zones as { label?: string; zone?: string }[]) : [];
  const time = now.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", second: seconds ? "2-digit" : undefined, hour12 });
  const body = data?.body.type === "clock" ? data.body : null;
  return (
    <div className="w-stack">
      <p className="w-big w-clock">
        <time dateTime={now.toISOString()}>{time}</time>
      </p>
      <p className="w-row">
        <span>{now.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" })}</span>
        {body && <span className="w-muted">wk {body.isoWeek} · day {body.dayOfYear}</span>}
      </p>
      {body && (
        <p className="w-row">
          <Meter pct={body.yearPct} label={`${Math.round(body.yearPct)}% of the year gone`} />
          <span className="w-muted w-num">{Math.round(body.yearPct)}%</span>
        </p>
      )}
      {zones.length > 0 && (
        <ul className="w-lines">
          {zones.map((z, i) => (
            <li key={i} className="w-row">
              <span>{z.label || z.zone}</span>
              <span className="w-num">{(z.zone && zoneTime(z.zone, now, hour12)) ?? "?"}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function System({ body }: { body: Body<"system"> }) {
  const s = body.now;
  const memPct = s.memTotal ? (s.memUsed / s.memTotal) * 100 : 0;
  const diskPct = s.diskTotal && s.diskFree !== null ? ((s.diskTotal - s.diskFree) / s.diskTotal) * 100 : null;
  const pressure = s.memPressure === "critical" ? "bad" : s.memPressure === "warn" ? "warn" : undefined;
  return (
    <div className="w-stack">
      <div className="w-metric">
        <span className="w-key">cpu</span>
        <Spark values={body.cpuHistory} max={100} label="CPU" unit="%" />
        <span className="w-num">{Math.round(s.cpuPct)}%</span>
      </div>
      <div className="w-metric">
        <span className="w-key">mem</span>
        <Spark values={body.memHistory} max={100} label="Memory used" unit="%" />
        <span className="w-num" title={s.memPressure ? `pressure ${s.memPressure}` : undefined}>
          {Math.round(memPct)}%{pressure ? " !" : ""}
        </span>
      </div>
      {diskPct !== null && s.diskFree !== null && (
        <div className="w-metric">
          <span className="w-key">disk</span>
          <Meter pct={diskPct} label={`Disk ${Math.round(diskPct)}% used`} />
          <span className="w-num">{bytes(s.diskFree)} free</span>
        </div>
      )}
      {s.battery && (
        <div className="w-metric">
          <span className="w-key">bat</span>
          <Meter pct={s.battery.pct} label={`Battery ${s.battery.pct}%`} tone={!s.battery.plugged && s.battery.pct <= 20 ? "warn" : undefined} />
          <span className="w-num">
            {s.battery.pct}%{s.battery.charging ? " ↑" : s.battery.plugged ? " =" : ""}
          </span>
        </div>
      )}
      <p className="w-row w-muted">
        <span>up {uptime(s.uptimeSecs)}</span>
        {s.cpuCount > 0 && <span>{s.cpuCount} cores</span>}
      </p>
      {s.top.length > 0 && (
        <ul className="w-lines" aria-label="Busiest processes">
          {s.top.map((p, i) => (
            <li key={i} className="w-row">
              <span title={p.name}>{p.name}</span>
              <span className="w-num">{p.cpuPct.toFixed(0)}%</span>
            </li>
          ))}
        </ul>
      )}
      {body.notes.length > 0 && <p className="w-note">{body.notes.join(" · ")}</p>}
    </div>
  );
}

function Burn({ body }: { body: Body<"tasksBurn"> }) {
  const open = body.days.map((d) => d.open);
  const done = body.days.map((d) => d.done);
  return (
    <div className="w-stack">
      <div className="w-metric">
        <span className="w-key">open</span>
        <Spark values={open} label="Open tasks" />
        <span className="w-num">{body.openNow}</span>
      </div>
      <div className="w-metric">
        <span className="w-key">done</span>
        <Bars values={done} label="Done per day" />
        <span className="w-num" title={`average over ${body.days.length} days`}>
          {perDay(body.donePerDay)}/d
        </span>
      </div>
      <p className="w-row w-muted">
        <span>{body.days.length} days</span>
        <span title={`created minus done this week: ${body.net7d > 0 ? "+" : ""}${body.net7d}`}>week {body.done7d} done</span>
      </p>
    </div>
  );
}

function TaskList({ tasks, total }: { tasks: WidgetTask[]; total: number }) {
  if (tasks.length === 0) return <p className="w-muted">Nothing here. Good.</p>;
  return (
    <ul className="w-lines">
      {tasks.map((t) => (
        <li key={t.id} className="w-row">
          <span title={t.project ? `${t.title} (${t.project})` : t.title}>
            {t.started ? "▸ " : ""}
            {t.title}
          </span>
          <span className={`w-num${t.dueIn !== null && t.dueIn < 0 ? " is-bad" : t.dueIn === 0 ? " is-warn" : ""}`}>
            {t.dueIn !== null ? dueLabel(t.dueIn) : t.urgency.toFixed(1)}
          </span>
        </li>
      ))}
      {total > tasks.length && <li className="w-muted">+{total - tasks.length} more</li>}
    </ul>
  );
}

function JsonStatus({ body }: { body: Body<"jsonStatus"> }) {
  const now = useNow(30_000).getTime();
  return (
    <div className="w-stack">
      {body.fields.length > 0 && (
        <ul className="w-lines">
          {body.fields.map((f, i) => (
            <li key={i} className="w-row">
              <span title={f.label}>
                <span className={`w-mark${f.ok === false ? " is-bad" : f.ok ? " is-ok" : ""}`} aria-hidden="true">
                  {f.ok === null ? "·" : f.ok ? "✓" : "✗"}
                </span>{" "}
                {f.label}
                {f.ok !== null && <span className="sr-only">{f.ok ? " ok" : " failing"}</span>}
              </span>
              <span className="w-num" title={f.value ?? "not found"}>
                {f.value ?? "–"}
                {f.value !== null && f.unit ? ` ${f.unit}` : ""}
              </span>
            </li>
          ))}
        </ul>
      )}
      {body.error && <p className="w-note is-bad">{body.error}</p>}
      <p className="w-row w-muted">
        <span title={body.url}>{hostOf(body.url)}</span>
        <span>{body.fetchedAt ? ago(body.fetchedAt, now) : "not fetched"}</span>
      </p>
    </div>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
