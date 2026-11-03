// Vault HQ server: JSON API over the vault plus the built web app, on 127.0.0.1 only.
// Writes need `X-HQ: 1` and a same-origin `Origin`, so no other site open in the
// browser can drive it (a cross-site form or fetch can't set that header without CORS).
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { z } from "zod";
import { config } from "./config.ts";
import { addDays, today } from "./dates.ts";
import { activeProjects, home } from "./home.ts";
import { needs } from "./needs.ts";
import { noteList, noteView } from "./notes.ts";
import { chooseVault, reveal, RevealInput, saveSettings, settings, SettingsInput, vaultCandidates } from "./settings.ts";
import { extension } from "./extensions.ts";
import { lastTick, openErrors } from "./signals.ts";
import * as tw from "./taskwarrior.ts";
import type { Status } from "./types.ts";
import { day, inbox, notes, tasks, unreadable } from "./vault.ts";
import { ask, ollamaStatus, search } from "./search.ts";
import { addToDay, archiveCapture, capture, decide, noteSource, saveNote, SECTIONS, taskAction } from "./writes.ts";

const DATE = z.string().regex(/^\d{4}-\d{2}-\d{2}$/);
const UUID = z.string().uuid();

const Decision = z.object({ id: z.string().min(1), outcome: z.string().min(1), why: z.string().max(4000).default("") });
const TaskAdd = z.object({
  description: z.string().trim().min(1).max(500),
  project: z.string().regex(/^[a-z0-9][a-z0-9-]*$/).optional(),
  due: DATE.optional(),
  priority: z.enum(["H", "M", "L"]).optional(),
});
const TaskModify = z.object({ due: DATE.nullable().optional(), priority: z.enum(["H", "M", "L"]).nullable().optional() });
const SPAN = z.coerce.number().int().min(1).max(366);
const Ask = z.object({ question: z.string().trim().min(3).max(2000), date: DATE.optional(), span: SPAN.optional() });
// Daily-note properties every vault records (daily-metrics.py); extensions add their own.
const CORE_METRICS = [
  { key: "tasks_pending", label: "Open tasks" },
  { key: "tasks_done_today", label: "Done that day" },
  { key: "vault_inbox", label: "Inbox" },
];

const Archive = z.object({ rel: z.string().startsWith("00-Inbox/") });
const DayAdd = z.object({ date: DATE, section: z.enum(SECTIONS), text: z.string().trim().min(1).max(2000) });
const NoteSave = z.object({ rel: z.string().min(4).max(500).endsWith(".md"), text: z.string().max(2_000_000), base: z.string().regex(/^[0-9a-f]{64}$/) });
const Capture = z.object({ text: z.string().trim().min(1).max(4000) });

class HttpError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

function send(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" });
  res.end(JSON.stringify(body));
}

async function body(req: http.IncomingMessage, limit = 64_000): Promise<unknown> {
  let raw = "";
  for await (const chunk of req) {
    raw += chunk;
    if (raw.length > limit) throw new HttpError(413, "Request too large.");
  }
  return raw ? JSON.parse(raw) : {};
}

function parse<T>(schema: z.ZodType<T>, value: unknown): T {
  const r = schema.safeParse(value);
  if (!r.success) throw new HttpError(400, r.error.issues.map((i) => `${i.path.join(".") || "body"}: ${i.message}`).join("; "));
  return r.data;
}

function guardWrite(req: http.IncomingMessage): void {
  const origin = req.headers.origin;
  const allowed = [`http://localhost:${config.port}`, `http://127.0.0.1:${config.port}`, "http://localhost:5173"];
  if (req.headers["x-hq"] !== "1" || (origin && !allowed.includes(origin))) {
    throw new HttpError(403, "Writes are only accepted from Vault HQ itself.");
  }
}

// Notes the user wrote or edited lately, for suggested questions: anything with a title and
// an updated date outside the inbox, planning, memory, archive and tooling folders.
const SKIP = /^(00-Inbox|06-Memory|08-Archive|09-Planning|_templates|_tools)\/|(^|\/)(AGENTS|README)\.md$/;
function recentNotes(limit: number) {
  return notes()
    .filter((x) => x.props.title && x.props.updated && !SKIP.test(x.rel))
    .sort((a, b) => (b.props.updated ?? "").localeCompare(a.props.updated ?? ""))
    .slice(0, limit);
}

async function status(): Promise<Status> {
  const t = today();
  const [n, external] = await Promise.all([needs(), extension?.status() ?? null]);
  const open = tasks().filter((x) => x.status === "pending");
  const d = day(t);
  const suggestions = [
    ...activeProjects(3).map((p) => `What is left on ${p.title}?`),
    ...(d.agentLog.length ? ["What did the agents do today?"] : []),
    ...(n[0] ? [`Why is "${(n[0].title.split(/\s[—-]\s/)[0] ?? "").slice(0, 70).trim()}" waiting on me?`] : []),
    ...recentNotes(2).map((x) => `What came out of ${x.props.title}?`),
  ];
  return {
    vault: config.vaultName,
    needs: n.length,
    lastTick: lastTick(),
    errors24h: openErrors(24).length,
    unreadable: unreadable(),
    external,
    tips: {
      needs: n.slice(0, 3).map((x) => x.title),
      tasksOpen: open.length,
      tasksOverdue: open.filter((x) => x.due !== null && x.due < t).length,
      inbox: inbox(t).length,
      today: d.agentLog.length + d.completed.length,
      notes: notes().length,
      suggestions,
    },
  };
}

async function api(req: http.IncomingMessage, url: URL): Promise<unknown> {
  const p = url.pathname;
  const m = req.method ?? "GET";

  if (m === "GET") {
    if (p === "/api/status") return status();
    if (p === "/api/home") return home(url.searchParams.has("date") ? parse(DATE, url.searchParams.get("date")) : undefined);
    if (p === "/api/needs") return needs();
    if (p === "/api/notes") return noteList();
    if (p === "/api/settings") return settings();
    if (p === "/api/settings/vaults") return vaultCandidates();
    if (p === "/api/note/source") {
      try {
        return noteSource(parse(z.string().min(4).max(500), url.searchParams.get("rel") ?? ""));
      } catch (e) {
        throw new HttpError(404, (e as Error).message);
      }
    }
    if (p === "/api/note") {
      const ref = parse(z.string().trim().min(1).max(500), url.searchParams.get("ref") ?? "");
      const n = noteView(ref);
      if (!n) throw new HttpError(404, `No note matches "${ref}".`);
      return n;
    }
    if (p === "/api/inbox") return inbox(today());
    if (p === "/api/tasks") return tw.pending();
    if (p === "/api/metrics") return [...CORE_METRICS, ...(extension?.metrics ?? [])];
    if (p === "/api/ollama") return ollamaStatus();
    if (p === "/api/search") {
      const q = parse(z.string().trim().min(2).max(500), url.searchParams.get("q") ?? "");
      const scope = url.searchParams.has("span")
        ? { date: parse(DATE, url.searchParams.get("date") ?? today()), span: parse(SPAN, url.searchParams.get("span")) }
        : undefined;
      return search(q, 8, scope);
    }
    if (p === "/api/days") {
      const from = parse(DATE, url.searchParams.get("from") ?? today());
      const span = parse(z.coerce.number().int().min(1).max(31), url.searchParams.get("span") ?? "1");
      return Array.from({ length: span }, (_, i) => day(addDays(from, i)));
    }
    throw new HttpError(404, `No such endpoint: ${p}`);
  }

  if (m !== "POST") throw new HttpError(405, "Use GET or POST.");
  guardWrite(req);
  const input = await body(req, p === "/api/note/save" ? 4_000_000 : 64_000); // whole notes can be large

  if (p === "/api/needs/decide") {
    const d = parse(Decision, input);
    const need = (await needs()).find((x) => x.id === d.id);
    if (!need) throw new HttpError(404, "That item is no longer in the queue. Refresh to see the current list.");
    if (!need.outcomes.some((o) => o.key === d.outcome)) throw new HttpError(400, `Unknown outcome for this item: ${d.outcome}`);
    return { result: await decide(need, d.outcome, d.why) };
  }
  if (p === "/api/tasks/add") {
    const t = parse(TaskAdd, input);
    await taskAction(() => tw.add(t), `Added task: ${t.description}`);
    return { result: "Added" };
  }
  const taskOp = /^\/api\/tasks\/([^/]+)\/(done|modify|delete)$/.exec(p);
  if (taskOp) {
    const uuid = parse(UUID, taskOp[1]);
    const op = taskOp[2];
    if (op === "done") await taskAction(() => tw.done(uuid), `Completed a task from HQ`);
    else if (op === "delete") await taskAction(() => tw.remove(uuid), `Dropped a task from HQ`);
    else {
      const change = parse(TaskModify, input);
      await taskAction(() => tw.modify(uuid, change), `Changed a task from HQ`);
    }
    return { result: "Saved" };
  }
  if (p === "/api/inbox/archive") {
    archiveCapture(parse(Archive, input).rel);
    return { result: "Archived" };
  }
  if (p === "/api/ask") {
    const { question, date, span } = parse(Ask, input);
    return ask(question, span ? { date: date ?? today(), span } : undefined);
  }
  if (p === "/api/day/add") {
    const d = parse(DayAdd, input);
    addToDay(d.date, d.section, d.text);
    return { result: `Added to ${d.section}` };
  }
  if (p === "/api/settings") {
    saveSettings(parse(SettingsInput, input));
    return settings();
  }
  if (p === "/api/settings/vault") {
    const { path: dir } = parse(z.object({ path: z.string().min(1).max(1000).nullable() }), input);
    try {
      return chooseVault(dir);
    } catch (e) {
      throw new HttpError(400, (e as Error).message);
    }
  }
  if (p === "/api/settings/reveal") {
    reveal(parse(RevealInput, input).what);
    return { result: "Shown in Finder" };
  }
  if (p === "/api/note/save") {
    const s = parse(NoteSave, input);
    let r;
    try {
      r = await saveNote(s.rel, s.text, s.base);
    } catch (e) {
      throw new HttpError(/No note at/.test((e as Error).message) ? 404 : 500, (e as Error).message);
    }
    if (r.conflict) {
      const err = new HttpError(409, "This note changed after you started editing.");
      Object.assign(err, { current: { text: r.text, hash: r.hash } });
      throw err;
    }
    return { result: "Saved", rel: s.rel, text: r.text, hash: r.hash };
  }
  if (p === "/api/capture") {
    return { result: "Captured", rel: capture(parse(Capture, input).text) };
  }
  throw new HttpError(404, `No such endpoint: ${p}`);
}

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

function serveStatic(url: URL, res: http.ServerResponse): void {
  const safe = path.normalize(url.pathname).replace(/^(\.\.[/\\])+/, "");
  let file = path.join(config.webDist, safe);
  if (!file.startsWith(config.webDist) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    file = path.join(config.webDist, "index.html");
  }
  if (!fs.existsSync(file)) {
    res.writeHead(503, { "content-type": "text/plain" });
    res.end("The web app is not built yet. Run `npm run build`, or `npm start` to build and serve.");
    return;
  }
  res.writeHead(200, { "content-type": TYPES[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://${config.host}`);
  if (!url.pathname.startsWith("/api/")) return serveStatic(url, res);
  try {
    send(res, 200, await api(req, url));
  } catch (e) {
    const status = e instanceof HttpError ? e.status : 500;
    send(res, status, { error: e instanceof Error ? e.message : String(e), ...(e instanceof HttpError && "current" in e ? { current: (e as HttpError & { current: unknown }).current } : {}) });
  }
});

server.on("error", (e: NodeJS.ErrnoException) => {
  if (e.code === "EADDRINUSE") {
    console.error(`Port ${config.port} is taken, probably by another Vault HQ. Stop it with:  lsof -ti tcp:${config.port} -sTCP:LISTEN | xargs kill`);
    console.error(`or run this one elsewhere:  HQ_PORT=4321 npm start`);
    process.exit(1);
  }
  throw e;
});

server.listen(config.port, config.host, () => {
  console.log(`Vault HQ on http://localhost:${config.port}  (vault: ${config.vault})`);
  // read the vault once now, so the first page load doesn't wait on a cold iCloud scan
  void home().catch(() => undefined);
});
