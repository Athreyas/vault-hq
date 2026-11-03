// HQ's own small state (snoozes). Lives beside the automation state, not in the vault:
// a snooze is a UI preference, not knowledge.
import fs from "node:fs";
import path from "node:path";
import { config } from "./config.ts";

const FILE = path.join(config.state, "hq.json");

interface State {
  snoozed: Record<string, string>; // need id -> ISO date it comes back
}

function load(): State {
  try {
    return JSON.parse(fs.readFileSync(FILE, "utf8")) as State;
  } catch {
    return { snoozed: {} };
  }
}

export function snoozedUntil(id: string, todayIso: string): string | null {
  const until = load().snoozed[id];
  return until && until > todayIso ? until : null;
}

export function snooze(id: string, until: string): void {
  const s = load();
  s.snoozed[id] = until;
  fs.mkdirSync(path.dirname(FILE), { recursive: true });
  fs.writeFileSync(FILE, JSON.stringify(s, null, 1));
}
