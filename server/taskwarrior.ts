// Every task write goes through the `task` CLI with hooks on, so Taskwarrior's
// on-exit hook runs task-sync.py and the change lands in the vault note. HQ never
// edits checkboxes itself: one write path for tasks.
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { config } from "./config.ts";
import type { TwTask } from "./types.ts";

const run = promisify(execFile);
const QUIET = ["rc.verbose=nothing", "rc.confirmation=off"];

function localDate(stamp: string | undefined): string | null {
  if (!stamp) return null;
  const m = /^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z$/.exec(stamp);
  if (!m) return null;
  const d = new Date(Date.UTC(+m[1]!, +m[2]! - 1, +m[3]!, +m[4]!, +m[5]!, +m[6]!));
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

interface Raw {
  uuid: string;
  description: string;
  project?: string;
  priority?: "H" | "M" | "L";
  due?: string;
  vaultnote?: string;
  urgency?: number;
}

export async function pending(): Promise<TwTask[]> {
  const { stdout } = await run(config.task, ["rc.hooks=off", ...QUIET, "status:pending", "export"], {
    maxBuffer: 16 * 1024 * 1024,
  });
  return (JSON.parse(stdout || "[]") as Raw[])
    .map((t) => ({
      uuid: t.uuid,
      description: t.description,
      project: t.project ?? null,
      priority: t.priority ?? null,
      due: localDate(t.due),
      note: t.vaultnote ?? null,
      urgency: t.urgency ?? 0,
    }))
    .sort((a, b) => b.urgency - a.urgency);
}

export async function done(uuid: string): Promise<void> {
  await run(config.task, [...QUIET, uuid, "done"]);
}

export async function remove(uuid: string): Promise<void> {
  await run(config.task, [...QUIET, uuid, "delete"]);
}

export async function modify(uuid: string, change: { due?: string | null; priority?: string | null }): Promise<void> {
  const args: string[] = [];
  if (change.due !== undefined) args.push(`due:${change.due ?? ""}`);
  if (change.priority !== undefined) args.push(`priority:${change.priority ?? ""}`);
  if (args.length) await run(config.task, [...QUIET, uuid, "modify", ...args]);
}

export async function add(t: { description: string; project?: string; due?: string; priority?: string }): Promise<void> {
  const args = ["add"];
  if (t.project) args.push(`project:${t.project}`);
  if (t.due) args.push(`due:${t.due}`);
  if (t.priority) args.push(`priority:${t.priority}`);
  // `--` stops Taskwarrior from parsing words in the description as attributes
  await run(config.task, [...QUIET, ...args, "--", t.description]);
}
