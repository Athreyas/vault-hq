// Signals from outside the vault: the automation log and the last sync.
import fs from "node:fs";
import path from "node:path";
import { config } from "./config.ts";

export interface LogLine {
  at: string; // "YYYY-MM-DD HH:MM:SS"
  text: string;
  error: boolean;
}

export function automationLog(hours = 24): LogLine[] {
  const file = path.join(config.state, "automation.log");
  if (!fs.existsSync(file)) return [];
  const since = Date.now() - hours * 3_600_000;
  const out: LogLine[] = [];
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    const m = /^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) (.*)$/.exec(line);
    if (!m?.[1] || !m[2]) continue;
    if (Date.parse(m[1].replace(" ", "T")) < since) continue;
    out.push({ at: m[1], text: m[2], error: /\berror\b|exited [1-9]|unavailable/i.test(m[2]) });
  }
  return out;
}

// Which background job a log line is about, so a later success can clear its errors.
function job(text: string): "tasks" | "metrics" | null {
  if (/^tasks:|task-sync\.py/.test(text)) return "tasks";
  if (/^metrics:|daily-metrics\.py/.test(text)) return "metrics";
  return null;
}

/** Errors from the last `hours` that nothing has fixed since: an error from task sync or
 *  metrics stops counting once that job has succeeded after it. Other errors count for the
 *  whole window. */
export function openErrors(hours = 24): LogLine[] {
  const lines = automationLog(hours);
  const stamp = (d: Date) => d.toLocaleString("sv-SE").slice(0, 19); // "YYYY-MM-DD HH:MM:SS", local time like the log
  const okAt: Record<"tasks" | "metrics", string> = { tasks: "", metrics: "" };
  const snap = path.join(config.state, "task-sync.json"); // task-sync writes this only after a full sync
  if (fs.existsSync(snap)) okAt.tasks = stamp(fs.statSync(snap).mtime);
  for (const l of lines) if (!l.error && /^metrics: wrote/.test(l.text) && l.at > okAt.metrics) okAt.metrics = l.at;
  return lines.filter((l) => {
    if (!l.error) return false;
    const j = job(l.text);
    return !j || l.at > okAt[j];
  });
}

export function lastTick(): string | null {
  const stamp = path.join(config.state, "task-sync.json");
  return fs.existsSync(stamp) ? new Date(fs.statSync(stamp).mtimeMs).toISOString() : null;
}
