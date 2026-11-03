// Shapes the server returns. The web app imports these as types only.

export type Phase = "dawn" | "day" | "dusk" | "night";

export interface VaultTask {
  id: string | null;          // Tasks-plugin 🆔, null if not yet synced
  desc: string;
  prio: "H" | "M" | "L" | null;
  due: string | null;         // YYYY-MM-DD
  done: string | null;        // ✅ date
  status: "pending" | "completed" | "deleted";
  note: string;               // vault-relative path
}

export interface TwTask {
  uuid: string;
  description: string;
  project: string | null;
  priority: "H" | "M" | "L" | null;
  due: string | null;         // YYYY-MM-DD (local)
  note: string | null;        // vaultnote UDA
  urgency: number;
}

export interface AgentLogLine {
  time: string | null;
  text: string;
  links: string[];
}

export interface Day {
  date: string;
  exists: boolean;
  metrics: Record<string, number | string>;
  human: { heading: string; lines: string[] }[];
  agentLog: AgentLogLine[];
  completed: VaultTask[];
  captures: { rel: string; title: string }[];
  dueTasks: number;           // pending tasks due that day
}

export interface Project {
  rel: string;
  stem: string;
  title: string;
  status: string;
  progress: number | null;
  updated: string;
  summary: string;
  openTasks: VaultTask[];
}

export interface Card {
  id: string;
  kind: string; // core: focus, needs, inbox, done, tomorrow, project, findings; extensions add their own
  kicker: string;
  title: string;
  lines: string[];
  progress: number | null;
  hue: number;
  action: { label: string; href: string } | null;
}

export interface Home {
  phase: Phase;
  greeting: string;
  cards: Card[];
}

export type NeedKind = "overdue" | "due-soon" | "capture" | "proposal" | "alert" | "automation" | "stale-project";

export interface Need {
  id: string;
  kind: NeedKind;
  label: string;              // short kind label shown in the list
  title: string;
  detail: string;
  evidence: { rel: string | null; excerpt: string; when: string | null };
  outcomes: { key: string; label: string; hint: string }[];
  rank: number;               // lower = more urgent
  taskUuid: string | null;
}

export interface Status {
  vault: string;
  needs: number;
  lastTick: string | null;
  errors24h: number;
  unreadable: string[]; // notes the server could not read, usually iCloud refusing a background read
  external: { name: string; state: string; up: number; total: number } | null; // from an extension, if any
  tips: {
    needs: string[];
    tasksOpen: number;
    tasksOverdue: number;
    inbox: number;
    today: number;
    notes: number;
    suggestions: string[];
  };
}

export interface InboxItem {
  rel: string;
  title: string;
  kind: "compact" | "session" | "proposal" | "dump";
  created: string;
  ageDays: number;
  author: string;
}

export interface NoteSummary {
  rel: string;
  title: string;
  folder: string;
  updated: string;
}

export interface NoteView extends NoteSummary {
  props: [string, string][];
  body: string;
  backlinks: NoteSummary[];
  obsidian: string; // obsidian:// link, for editing until HQ can edit notes itself
}
