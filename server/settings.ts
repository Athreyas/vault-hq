// Settings: what HQ is connected to, and what the user can change from the app: which
// vault HQ shows, the Taskwarrior binary, and the local model server and models.
// Saved beside the automation state (never in the vault); environment variables still win.
// The background automation keeps following its own vault-path file; moving it is the
// target vault's install.sh, which Settings shows rather than runs.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { z } from "zod";
import { spawn } from "node:child_process";
import { config, SETTINGS_FILE, VAULT_PATH_FILE } from "./config.ts";
import { extension } from "./extensions.ts";
import { ollamaStatus, resetVectors, vectorCount } from "./search.ts";
import { lastTick } from "./signals.ts";
import { notes, unreadable } from "./vault.ts";

const FILE = SETTINGS_FILE;
const STARTED = new Date().toISOString();
const VERSION = (JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "..", "package.json"), "utf8")) as { version: string }).version;

// Ask must stay on this Mac or the home network: loopback, private ranges, .local names.
const LOCAL_HOST = /^(localhost|127(\.\d{1,3}){3}|\[::1\]|10(\.\d{1,3}){3}|192\.168(\.\d{1,3}){2}|172\.(1[6-9]|2\d|3[01])(\.\d{1,3}){2}|[a-z0-9-]+\.local)$/i;
const localUrl = z
  .string()
  .trim()
  .url()
  .refine((u) => {
    const x = new URL(u);
    return x.protocol === "http:" && LOCAL_HOST.test(x.hostname) && x.pathname === "/" && !x.search;
  }, "Use http:// with this Mac or a home-network address (localhost, a private IP, name.local), no path.")
  .transform((u) => u.replace(/\/$/, ""));
const model = z.string().trim().min(1).max(120).regex(/^[\w.:/-]+$/, "A model name like llama3.1:8b");

const taskBin = z
  .string()
  .trim()
  .refine((p) => path.isAbsolute(p) && /^task/.test(path.basename(p)), "The full path to Taskwarrior's task program, e.g. /opt/homebrew/bin/task.")
  .refine((p) => {
    try {
      fs.accessSync(p, fs.constants.X_OK);
      return fs.statSync(p).isFile();
    } catch {
      return false;
    }
  }, "No runnable program at that path.");

export const SettingsInput = z.object({ ollama: z.object({ url: localUrl, chatModel: model, embedModel: model }).partial().optional(), task: taskBin.optional() });

type Saved = { ollama?: { url?: string; chatModel?: string; embedModel?: string }; vault?: string; task?: string };

function write(s: Saved): void {
  fs.mkdirSync(path.dirname(FILE), { recursive: true });
  fs.writeFileSync(FILE, JSON.stringify(s, null, 1));
}

const ENV = { url: "OLLAMA_URL", chatModel: "OLLAMA_CHAT_MODEL", embedModel: "OLLAMA_EMBED_MODEL" } as const;
const locked = (k: keyof typeof ENV) => Boolean(process.env[ENV[k]]);

function load(): Saved {
  try {
    return JSON.parse(fs.readFileSync(FILE, "utf8")) as Saved;
  } catch {
    return {};
  }
}

function apply(s: Saved): void {
  const o = config.ollama as { url: string; chatModel: string; embedModel: string };
  for (const k of ["url", "chatModel", "embedModel"] as const) {
    const v = s.ollama?.[k];
    if (v && !locked(k)) o[k] = v;
  }
}

apply(load()); // at startup

export function saveSettings(input: z.infer<typeof SettingsInput>): void {
  const before = config.ollama.embedModel;
  const s = load();
  if (input.ollama) s.ollama = { ...s.ollama, ...input.ollama };
  if (input.task && !process.env.TASK_BIN) {
    s.task = input.task;
    (config as { task: string }).task = input.task;
  }
  write(s);
  apply(s);
  if (config.ollama.embedModel !== before) resetVectors(); // vectors from another model don't compare
}

function vaultPathSource(): string {
  if (config.vaultFrom === "env") return "the VAULT_PATH environment variable";
  if (config.vaultFrom === "settings") return "Settings (HQ only; the background sync follows its own vault)";
  return "the vault automation (~/.config/vault-automation/vault-path, written by install.sh)";
}

function automationVault(): string | null {
  try {
    return fs.readFileSync(VAULT_PATH_FILE, "utf8").trim() || null;
  } catch {
    return null;
  }
}

// ---- choosing the vault --------------------------------------------------------------------

const expand = (p: string) => (p.startsWith("~/") ? path.join(os.homedir(), p.slice(2)) : p);

/** Why HQ can't serve `dir`, or null if it can. Checks the veto before looking inside. */
export function vaultProblem(dir: string): string | null {
  if (!path.isAbsolute(dir)) return "Use a full path, e.g. ~/Documents/my-vault.";
  const veto = extension?.vaultAllowed?.(dir);
  if (veto) return veto;
  let real: string;
  try {
    real = fs.realpathSync(dir);
    if (!fs.statSync(real).isDirectory()) return "That is a file, not a folder.";
  } catch {
    return "No folder at that path.";
  }
  if (real === os.homedir() || real === "/" || real.split("/").length < 4) return "Pick the vault folder itself, not a top-level folder.";
  const vetoReal = extension?.vaultAllowed?.(real);
  if (vetoReal) return vetoReal;
  try {
    const top = fs.readdirSync(real, { withFileTypes: true });
    const md = top.some((e) => e.isFile() && e.name.endsWith(".md")) || top.some((e) => e.isDirectory() && !e.name.startsWith(".") && fs.readdirSync(path.join(real, e.name)).some((f) => f.endsWith(".md")));
    if (!md && !top.some((e) => e.name === ".obsidian")) return "No markdown notes in that folder.";
  } catch {
    return "That folder can't be read.";
  }
  return null;
}

/** Obsidian vaults in the usual places, minus any this Mac refuses. */
export function vaultCandidates(): { path: string; name: string; automation: boolean; current: boolean }[] {
  const roots = [path.join(os.homedir(), "Library/Mobile Documents/iCloud~md~obsidian/Documents"), path.join(os.homedir(), "Documents"), os.homedir()];
  const out = new Map<string, { path: string; name: string; automation: boolean; current: boolean }>();
  for (const root of roots) {
    let entries: fs.Dirent[] = [];
    try {
      entries = fs.readdirSync(root, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      if (!e.isDirectory() || e.name.startsWith(".")) continue;
      const dir = path.join(root, e.name);
      if (extension?.vaultAllowed?.(dir)) continue; // never look inside a refused vault
      if (!fs.existsSync(path.join(dir, ".obsidian"))) continue;
      out.set(dir, { path: dir, name: e.name, automation: fs.existsSync(path.join(dir, "_tools/automation/install.sh")), current: dir === config.vault });
    }
  }
  return [...out.values()];
}

/** Point HQ at `dir` (or back at the automation's vault with null). Returns whether HQ restarts itself. */
export function chooseVault(dir: string | null): { restarting: boolean; vault: string } {
  if (process.env.VAULT_PATH) throw new Error("VAULT_PATH is set in the environment; change it there.");
  const s = load();
  let target: string;
  if (dir === null) {
    delete s.vault;
    target = automationVault() ?? config.vault;
  } else {
    const full = expand(dir.trim());
    const problem = vaultProblem(full);
    if (problem) throw new Error(problem);
    target = fs.realpathSync(full);
    if (target === automationVault()) delete s.vault; // same as the automation: no override needed
    else s.vault = target;
  }
  write(s);
  return { restarting: restartSoon(), vault: target };
}

/** Under launchd (the local.vault-hq service), exit and let KeepAlive start a fresh server. */
function restartSoon(): boolean {
  if (process.env.XPC_SERVICE_NAME !== "local.vault-hq") return false;
  setTimeout(() => process.exit(0), 400);
  return true;
}

const REVEAL = { vault: () => config.vault, automation: () => path.join(config.automation, "bin"), log: () => path.join(os.homedir(), "Library/Logs/vault-hq.log"), settings: () => FILE, state: () => config.state } as const;
export const RevealInput = z.object({ what: z.enum(Object.keys(REVEAL) as [keyof typeof REVEAL, ...(keyof typeof REVEAL)[]]) });

/** Show one of HQ's own locations in Finder. Only these fixed locations, never a path from the request. */
export function reveal(what: keyof typeof REVEAL): void {
  const p = REVEAL[what]();
  if (fs.existsSync(p)) spawn("/usr/bin/open", ["-R", p], { stdio: "ignore", detached: true }).unref();
}

export async function settings() {
  const ai = await ollamaStatus();
  const ns = notes();
  return {
    vault: {
      name: config.vaultName,
      path: config.vault,
      pathFrom: vaultPathSource(),
      from: config.vaultFrom,
      automationVault: automationVault(),
      hasAutomation: fs.existsSync(path.join(config.vault, "_tools/automation/install.sh")),
      icloud: config.vault.includes("/Mobile Documents/"),
      notes: ns.length,
      unreadable: unreadable().length,
      folders: [...new Set(ns.flatMap((n) => (n.rel.includes("/") ? [n.rel.slice(0, n.rel.indexOf("/"))] : [])))].sort(),
      automation: path.join(config.automation, "bin"),
      lastSync: lastTick(),
      collectors: config.collectors.list(),
    },
    ai: {
      url: config.ollama.url,
      chatModel: config.ollama.chatModel,
      embedModel: config.ollama.embedModel,
      locked: { url: locked("url"), chatModel: locked("chatModel"), embedModel: locked("embedModel") },
      up: ai.up,
      chatReady: ai.chat,
      embedReady: ai.embed,
      models: ai.models,
      vectors: vectorCount(),
    },
    tasks: { bin: config.task, installed: fs.existsSync(config.task), locked: Boolean(process.env.TASK_BIN) },
    locations: { state: config.state, settingsFile: FILE },
    app: {
      version: VERSION,
      port: config.port,
      pid: process.pid,
      node: process.version,
      startedAt: STARTED,
      service: fs.existsSync(path.join(os.homedir(), "Library/LaunchAgents/local.vault-hq.plist")),
      log: path.join(os.homedir(), "Library/Logs/vault-hq.log"),
      macApp: fs.existsSync(path.join(os.homedir(), "Applications/Vault HQ.app")),
      settingsFile: FILE,
      extension: Boolean(extension),
    },
    privacy: {
      user: os.userInfo().username,
      host: os.hostname(),
      bind: `${config.host}:${config.port}`,
    },
  };
}
