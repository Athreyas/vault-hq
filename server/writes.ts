// Every vault write HQ makes. Kept in one file so the full write surface is visible.
import { execFile, spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { config } from "./config.ts";
import { snoozeDate } from "./needs.ts";
import { snooze } from "./state.ts";
import * as tw from "./taskwarrior.ts";
import type { Need } from "./types.ts";
import { resolve } from "./notes.ts";
import { invalidate } from "./vault.ts";
import { addDays, today } from "./dates.ts";

const run = promisify(execFile);
const BIN = path.join(config.automation, "bin");
const DECISIONS = "09-Planning/decision-log.md";

// Reuse vaultlib.daily_log so the daily note gets the same locking and layout as
// every other writer (hooks, task-sync, metrics).
async function dailyLog(text: string): Promise<void> {
  const now = new Date();
  const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  const script = `import sys; sys.path.insert(0, ${JSON.stringify(BIN)}); from vaultlib import daily_log, machine; daily_log(f"${hhmm} hq@{machine()} · " + sys.argv[1])`;
  await run("/usr/bin/python3", ["-c", script, text]);
}

function appendDecision(need: Need, outcome: string, why: string): void {
  const file = path.join(config.vault, DECISIONS);
  const t = today();
  if (!fs.existsSync(file)) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(
      file,
      `---\ntitle: Decision log\ntype: index\ncreated: ${t}\nupdated: ${t}\nauthor: human\ntags: [planning, decisions]\n---\n\n# Decision log\n\nDecisions made in Vault HQ's Needs You queue, newest last. The reasoning is the user's own words.\n`,
    );
  }
  const now = new Date();
  const stamp = `${t} ${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  const label = need.outcomes.find((o) => o.key === outcome)?.label ?? outcome;
  const evidence = need.evidence.rel ? `[[${path.basename(need.evidence.rel, ".md")}]]` : "none";
  let text = fs.readFileSync(file, "utf8").replace(/^updated: .*$/m, `updated: ${t}`);
  text += `\n## ${stamp}: ${need.title}\n- Kind: ${need.label}\n- Outcome: ${label}\n- Why: ${why.trim() || "(not given)"}\n- Evidence: ${evidence}\n`;
  fs.writeFileSync(file, text);
}

export async function decide(need: Need, outcome: string, why: string): Promise<string> {
  let result: string;
  switch (outcome) {
    case "done":
      if (!need.taskUuid) throw new Error("This task is not in Taskwarrior yet; wait for the next sync and retry.");
      await tw.done(need.taskUuid);
      result = "Marked done";
      break;
    case "redate":
      if (!need.taskUuid) throw new Error("This task is not in Taskwarrior yet; wait for the next sync and retry.");
      await tw.modify(need.taskUuid, { due: addDays(today(), 7) });
      result = "Moved to next week";
      break;
    case "drop":
      if (!need.taskUuid) throw new Error("This task is not in Taskwarrior yet; wait for the next sync and retry.");
      await tw.remove(need.taskUuid);
      result = "Dropped";
      break;
    case "archive":
      if (!need.evidence.rel) throw new Error("Nothing to archive.");
      archiveCapture(need.evidence.rel);
      result = "Archived";
      break;
    case "decline":
      if (need.evidence.rel) archiveCapture(need.evidence.rel);
      result = "Declined";
      break;
    case "keep":
      snooze(need.id, addDays(today(), 14));
      result = "Kept active";
      break;
    case "snooze":
      snooze(need.id, snoozeDate(3));
      result = "Snoozed for 3 days";
      break;
    default:
      snooze(need.id, addDays(today(), 1));
      result = need.outcomes.find((o) => o.key === outcome)?.label ?? "Recorded";
  }
  if (outcome !== "snooze") appendDecision(need, outcome, why);
  await dailyLog(`${result}: ${need.title} → [[decision-log]]`).catch(() => undefined);
  invalidate();
  return result;
}

export function archiveCapture(rel: string): void {
  if (!rel.startsWith("00-Inbox/") || rel.includes("..")) throw new Error("Only inbox captures can be archived.");
  const from = path.join(config.vault, rel);
  const to = path.join(config.vault, "08-Archive/inbox", path.basename(rel));
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.renameSync(from, to);
  invalidate();
}

export async function taskAction(fn: () => Promise<void>, note: string): Promise<void> {
  await fn();
  await dailyLog(note).catch(() => undefined);
  invalidate();
}

// ---- the user's own words ------------------------------------------------------------
// HQ writes into the human zone only on the user's behalf: lines typed into a daily-note
// section, or a quick capture to the inbox. The layout matches vaultlib.new_daily.

export const SECTIONS = ["Quick Wins", "Work", "Learning", "Tomorrow"] as const;

function newDaily(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  const title = `${d.toLocaleDateString("en-US", { weekday: "long" })}, ${d.toLocaleDateString("en-US", { month: "long" })} ${d.getDate()}`;
  return (
    `---\ntitle: "${title}"\ntype: daily\ncreated: ${date}\nupdated: ${date}\nauthor: human\ntags: [daily]\n---\n\n` +
    `# ${title}\n\n## Quick Wins\n-\n\n## Work\n-\n\n## Learning\n-\n\n## Tomorrow\n-\n\n## Agent log\n\n` +
    "_Machine zone: agents and scripts append one line per action. Do not edit above this heading._\n"
  );
}

export function addToDay(date: string, section: (typeof SECTIONS)[number], text: string): void {
  const file = path.join(config.vault, `09-Planning/Daily/${date}.md`);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const lines = (fs.existsSync(file) ? fs.readFileSync(file, "utf8") : newDaily(date)).split("\n");
  let start = lines.findIndex((l) => l.trim() === `## ${section}`);
  if (start < 0) {
    // section missing (older note): add it just above the agent log
    const log = lines.findIndex((l) => l.trim() === "## Agent log");
    const at = log < 0 ? lines.length : log;
    lines.splice(at, 0, `## ${section}`, "-", "");
    start = at;
  }
  let end = start + 1;
  while (end < lines.length && !lines[end]!.startsWith("## ")) end++;
  // drop the template's empty "-" placeholder, then append after the last bullet
  for (let i = end - 1; i > start; i--) if (lines[i]!.trim() === "-") { lines.splice(i, 1); end--; }
  let at = end;
  while (at > start + 1 && lines[at - 1]!.trim() === "") at--;
  lines.splice(at, 0, `- ${text.trim()}`);
  let out = lines.join("\n").replace(/^updated: .*$/m, `updated: ${today()}`);
  if (!/\n## (Agent log)/.test(out)) out += "\n## Agent log\n";
  fs.writeFileSync(file, out);
  invalidate();
}

export function capture(text: string): string {
  const t = today();
  const rel = `00-Inbox/${t}-capture.md`;
  const file = path.join(config.vault, rel);
  const now = new Date();
  const stamp = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  if (!fs.existsSync(file)) {
    fs.writeFileSync(
      file,
      `---\ntitle: ${t} capture\ntype: capture\ncreated: ${t}\nupdated: ${t}\nauthor: human\ntags: [inbox]\n---\n\n# ${t} capture\n\nQuick thoughts captured in Vault HQ. Sort them into a project or note on the weekly pass.\n`,
    );
  }
  fs.appendFileSync(file, `\n- ${stamp} ${text.trim().replace(/\n+/g, " ")}`);
  invalidate();
  return rel;
}

// ---- editing a whole note -------------------------------------------------------------------
// The editor starts from the file's exact text and its SHA-256. A save goes through the same
// lock every vault writer uses (flock on the note itself, as vaultlib.locked does) and is
// refused if the file changed since the editor read it, so an agent's, a sync's or
// Obsidian's change is never overwritten blind. It writes in place, not by rename: another
// writer may hold the lock on this very file, and a rename would swap its file away.

const sha256 = (text: string) => crypto.createHash("sha256").update(text, "utf8").digest("hex");

function notePath(rel: string): string {
  const n = resolve(rel);
  if (!n || n.rel !== rel) throw new Error(`No note at ${rel}. Only existing notes can be edited.`);
  return path.join(config.vault, n.rel); // a path the vault scan found, never one built from the request
}

export function noteSource(rel: string): { rel: string; text: string; hash: string } {
  const text = fs.readFileSync(notePath(rel), "utf8");
  return { rel, text, hash: sha256(text) };
}

const SAVE_PY = `
import fcntl, hashlib, json, re, sys, datetime
path, base = sys.argv[1], sys.argv[2]
new = sys.stdin.read()
with open(path, "r+", encoding="utf-8", newline="") as f:
    fcntl.flock(f, fcntl.LOCK_EX)
    try:
        cur = f.read()
        h = hashlib.sha256(cur.encode("utf-8")).hexdigest()
        if h != base:
            print(json.dumps({"conflict": True, "text": cur, "hash": h}))
            sys.exit(0)
        if new.startswith("---\\n"):
            new = re.sub(r"^updated: .*$", "updated: " + datetime.date.today().isoformat(), new, count=1, flags=re.M)
        f.seek(0); f.truncate(); f.write(new); f.flush()
        print(json.dumps({"conflict": False, "text": new, "hash": hashlib.sha256(new.encode("utf-8")).hexdigest()}))
    finally:
        fcntl.flock(f, fcntl.LOCK_UN)
`;

export type SaveResult = { conflict: false; text: string; hash: string } | { conflict: true; text: string; hash: string };

export function saveNote(rel: string, text: string, base: string): Promise<SaveResult> {
  const file = notePath(rel);
  return new Promise((ok, fail) => {
    const py = spawn("/usr/bin/python3", ["-I", "-c", SAVE_PY, file, base], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    py.stdout.setEncoding("utf8").on("data", (c: string) => (out += c));
    py.stderr.setEncoding("utf8").on("data", (c: string) => (err += c));
    py.on("error", fail);
    py.on("close", (code) => {
      if (code !== 0) return fail(new Error(`Saving ${rel} failed: ${err.trim().split("\n").pop() ?? `exit ${code}`}`));
      try {
        const r = JSON.parse(out) as SaveResult;
        if (!r.conflict) invalidate();
        ok(r);
      } catch {
        fail(new Error(`Saving ${rel} gave no result.`));
      }
    });
    py.stdin.end(text, "utf8");
  });
}
