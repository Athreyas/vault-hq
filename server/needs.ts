// Needs You: things waiting on a human decision, derived from vault and signals by
// fixed rules (no model in the ranking), so the queue is explainable and stable.
import crypto from "node:crypto";
import { addDays, daysBetween, today } from "./dates.ts";
import { extension } from "./extensions.ts";
import { openErrors } from "./signals.ts";
import { snoozedUntil } from "./state.ts";
import * as tw from "./taskwarrior.ts";
import type { Need, TwTask } from "./types.ts";
import { excerpt, inbox, projects, tasks } from "./vault.ts";

const id = (...parts: string[]) => crypto.createHash("sha1").update(parts.join("|")).digest("hex").slice(0, 12);

const TASK_OUTCOMES = [
  { key: "done", label: "Done", hint: "Mark it complete in Taskwarrior and the note." },
  { key: "redate", label: "Move to next week", hint: "Set the due date seven days out." },
  { key: "drop", label: "Drop it", hint: "Cancel the task; the note keeps it struck through." },
  { key: "snooze", label: "Snooze 3 days", hint: "Hide it here until then; nothing else changes." },
];

export async function needs(): Promise<Need[]> {
  const t = today();
  const out: Need[] = [];
  let twTasks: TwTask[] = [];
  try {
    twTasks = await tw.pending();
  } catch {
    twTasks = [];
  }
  const uuidFor = (desc: string) => twTasks.find((x) => x.description === desc)?.uuid ?? null;

  for (const task of tasks().filter((x) => x.status === "pending" && x.due)) {
    const late = daysBetween(task.due!, t);
    const soon = -late;
    if (late > 0) {
      out.push({
        id: id("overdue", task.id ?? task.desc),
        kind: "overdue",
        label: "Overdue",
        title: task.desc,
        detail: `Due ${task.due}, ${late} day${late === 1 ? "" : "s"} ago.`,
        evidence: { rel: task.note, excerpt: excerpt(task.note, 400), when: task.due },
        outcomes: TASK_OUTCOMES,
        rank: 10 - Math.min(late, 9),
        taskUuid: uuidFor(task.desc),
      });
    } else if (task.prio === "H" && soon <= 3) {
      out.push({
        id: id("due-soon", task.id ?? task.desc),
        kind: "due-soon",
        label: soon === 0 ? "Due today" : `Due in ${soon} day${soon === 1 ? "" : "s"}`,
        title: task.desc,
        detail: `High priority, due ${task.due}.`,
        evidence: { rel: task.note, excerpt: excerpt(task.note, 400), when: task.due },
        outcomes: TASK_OUTCOMES,
        rank: 20 + soon,
        taskUuid: uuidFor(task.desc),
      });
    }
  }

  if (extension) out.push(...(await extension.needs(t)));

  const errors = openErrors(24);
  if (errors.length) {
    const last = errors.at(-1)!;
    out.push({
      id: id("automation", last.at.slice(0, 13)),
      kind: "automation",
      label: "Automation",
      title: `${errors.length} automation error${errors.length === 1 ? "" : "s"} not yet resolved`,
      detail: last.text.slice(0, 240),
      evidence: { rel: "07-Agents/claude-code.md", excerpt: errors.slice(-5).map((e) => `${e.at}  ${e.text.slice(0, 160)}`).join("\n"), when: last.at },
      outcomes: [
        { key: "ack", label: "Seen it", hint: "Record that you looked; it returns if errors continue past this hour." },
        { key: "snooze", label: "Snooze 3 days", hint: "Hide it here until then." },
      ],
      rank: 30,
      taskUuid: null,
    });
  }

  for (const c of inbox(t)) {
    if (c.kind === "proposal") {
      out.push({
        id: id("proposal", c.rel),
        kind: "proposal",
        label: "Proposal",
        title: c.title,
        detail: "An agent proposes a change to a note in your zone. Decide whether it should be applied.",
        evidence: { rel: c.rel, excerpt: excerpt(c.rel, 900), when: c.created },
        outcomes: [
          { key: "accept", label: "Accept", hint: "Record the go; ask an agent to apply it." },
          { key: "decline", label: "Decline", hint: "Record the no and archive the proposal." },
          { key: "snooze", label: "Decide later", hint: "Hide it for 3 days." },
        ],
        rank: 15,
        taskUuid: null,
      });
    } else if (c.ageDays >= 2) {
      out.push({
        id: id("capture", c.rel),
        kind: "capture",
        label: "Inbox",
        title: c.title,
        detail: `Sitting in the inbox for ${c.ageDays} days. Curate what matters into a project or memory note, then archive it.`,
        evidence: { rel: c.rel, excerpt: excerpt(c.rel, 700), when: c.created },
        outcomes: [
          { key: "archive", label: "Done with it", hint: "Move it to 08-Archive/inbox/." },
          { key: "snooze", label: "Snooze 3 days", hint: "Hide it here until then." },
        ],
        rank: 40 + Math.min(c.ageDays, 30) * -0.1,
        taskUuid: null,
      });
    }
  }

  for (const p of projects().filter((x) => x.status === "active" && x.updated)) {
    const idle = daysBetween(p.updated, t);
    if (idle >= 14) {
      out.push({
        id: id("stale", p.rel, String(Math.floor(idle / 14))),
        kind: "stale-project",
        label: "Quiet project",
        title: p.title,
        detail: `Marked active, but untouched for ${idle} days. Keep it, or pause it so it stops competing for attention.`,
        evidence: { rel: p.rel, excerpt: p.summary, when: p.updated },
        outcomes: [
          { key: "keep", label: "Still active", hint: "Record it; it comes back in two weeks if still quiet." },
          { key: "pause", label: "Pause it", hint: "Record the pause; ask an agent to set status: paused." },
          { key: "snooze", label: "Snooze 3 days", hint: "Hide it here until then." },
        ],
        rank: 50,
        taskUuid: null,
      });
    }
  }

  return out.filter((n) => !snoozedUntil(n.id, t)).sort((a, b) => a.rank - b.rank);
}

export function snoozeDate(days = 3): string {
  return addDays(today(), days);
}
