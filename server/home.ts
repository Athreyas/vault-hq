// Home panels: the same cards all day, reordered by time of day.
// Morning leads with what to do, midday with work in motion, evening with what
// still needs you, night with how the day went and what tomorrow holds.
import { noteHref } from "./config.ts";
import { addDays, phase, today } from "./dates.ts";
import { needs } from "./needs.ts";
import { extension } from "./extensions.ts";
import type { Card, Home, Phase } from "./types.ts";
import { byUrgency, day, findings, inbox, projects, tasks } from "./vault.ts";

const ORDER: Record<Phase, string[]> = extension?.order ?? {
  dawn: ["focus", "needs", "project", "inbox", "findings", "tomorrow", "done"],
  day: ["focus", "project", "needs", "inbox", "findings", "done", "tomorrow"],
  dusk: ["needs", "done", "inbox", "project", "focus", "findings", "tomorrow"],
  night: ["done", "tomorrow", "needs", "project", "inbox", "findings", "focus"],
};

const GREETING: Record<Phase, string> = {
  dawn: "Early start",
  day: "Today",
  dusk: "Winding down",
  night: "The day so far",
};

// A stable hue per card so a project keeps its colour across visits.
function hue(key: string): number {
  let h = 0;
  for (const ch of key) h = (h * 31 + ch.charCodeAt(0)) % 360;
  return h;
}

export function activeProjects(limit: number) {
  return projects()
    .filter((x) => x.status === "active")
    .sort((a, b) => b.updated.localeCompare(a.updated) || b.openTasks.length - a.openTasks.length)
    .slice(0, limit);
}

export async function home(date?: string): Promise<Home> {
  const t = date ?? today();
  const isToday = t === today();
  const p = phase();
  const open = tasks().filter((x) => x.status === "pending");
  const cards: Card[] = [];

  const focus = open.filter((x) => (x.due && x.due <= addDays(t, 2)) || x.prio === "H").sort(byUrgency).slice(0, 5);
  cards.push({
    id: "focus",
    kind: "focus",
    kicker: `${open.length} open tasks`,
    title: focus.length ? "Worth doing today" : "Nothing pressing",
    lines: focus.length ? focus.map((x) => x.desc) : ["No task is due soon or marked high priority."],
    progress: null,
    hue: 36,
    action: { label: "Open tasks", href: "#/tasks" },
  });

  const n = await needs();
  cards.push({
    id: "needs",
    kind: "needs",
    kicker: `${n.length} waiting`,
    title: n.length ? (n[0]?.title ?? "") : "Nothing needs a decision",
    lines: n.slice(1, 4).map((x) => `${x.label}: ${x.title}`),
    progress: null,
    hue: 12,
    action: n.length ? { label: "Decide", href: "#/needs" } : null,
  });

  const box = inbox(t);
  cards.push({
    id: "inbox",
    kind: "inbox",
    kicker: `${box.length} in the inbox`,
    title: box.length ? "Captures to curate" : "Inbox is clear",
    lines: box.slice(0, 4).map((c) => `${c.title} (${c.ageDays}d)`),
    progress: null,
    hue: 260,
    action: { label: "Open inbox", href: "#/inbox" },
  });

  const d = day(t);
  cards.push({
    id: "done",
    kind: "done",
    kicker: `${d.agentLog.length} agent actions, ${d.completed.length} tasks done`,
    title: d.completed.length ? (isToday ? "Done today" : "Done that day") : isToday ? "Today's trail" : "That day's trail",
    lines: [...d.completed.map((x) => `✓ ${x.desc}`), ...d.agentLog.map((a) => `${a.time ?? ""} ${a.text}`)].slice(0, 5),
    progress: null,
    hue: 200,
    action: { label: "Open day", href: `#/days/${t}` },
  });

  const tomorrow = d.human.find((s) => s.heading === "Tomorrow")?.lines ?? [];
  const dueTomorrow = open.filter((x) => x.due === addDays(t, 1)).map((x) => x.desc);
  cards.push({
    id: "tomorrow",
    kind: "tomorrow",
    kicker: "Tomorrow",
    title: tomorrow[0] ?? (dueTomorrow.length ? `${dueTomorrow.length} due tomorrow` : "Nothing planned yet"),
    lines: [...tomorrow.slice(1), ...dueTomorrow].slice(0, 4),
    progress: null,
    hue: 230,
    action: { label: "Open tomorrow", href: `#/days/${addDays(t, 1)}` },
  });

  if (extension) cards.push(...(await extension.cards(t)));

  const active = activeProjects(6);
  for (const pr of active) {
    cards.push({
      id: `project-${pr.stem}`,
      kind: "project",
      kicker: `${pr.openTasks.length} open, updated ${pr.updated}`,
      title: pr.title,
      lines: pr.openTasks.length ? pr.openTasks.slice(0, 3).map((x) => x.desc) : [pr.summary],
      progress: pr.progress,
      hue: hue(pr.stem),
      action: { label: "Open note", href: noteHref(pr.rel) },
    });
  }

  for (const f of findings()) {
    cards.push({
      id: `findings-${f.rel}`,
      kind: "findings",
      kicker: "Findings",
      title: f.title,
      lines: f.latest ? [`Last run: ${f.latest}`] : ["No runs logged yet."],
      progress: null,
      hue: hue(f.rel),
      action: { label: "Open note", href: noteHref(f.rel) },
    });
  }

  const order = ORDER[p];
  cards.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind));
  return { phase: p, greeting: isToday ? GREETING[p] : t, cards };
}

