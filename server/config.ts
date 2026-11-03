import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const home = os.homedir();

const STATE = process.env.VAULT_STATE ?? path.join(home, ".local/share/vault-automation/state");
export const SETTINGS_FILE = path.join(STATE, "hq-settings.json");
export const VAULT_PATH_FILE = path.join(home, ".config/vault-automation/vault-path");

// Overrides saved from Settings (server/settings.ts writes them; read here at startup).
function saved(): { vault?: string; task?: string } {
  try {
    return JSON.parse(fs.readFileSync(SETTINGS_FILE, "utf8")) as { vault?: string; task?: string };
  } catch {
    return {};
  }
}

// One Mac serves one vault. Normally that is the file the vault automation's install.sh
// writes, shared with the background sync; Settings can point HQ alone somewhere else.
// Each machine runs its own HQ over its own vault; HQ never sees two at once.
function vaultPath(): { path: string; from: "env" | "settings" | "automation" } {
  if (process.env.VAULT_PATH) return { path: process.env.VAULT_PATH, from: "env" };
  const own = saved().vault;
  if (own && fs.existsSync(own)) return { path: own, from: "settings" };
  try {
    return { path: fs.readFileSync(VAULT_PATH_FILE, "utf8").trim(), from: "automation" };
  } catch {
    throw new Error(`No vault configured. Run the vault's _tools/automation/install.sh, set VAULT_PATH, or choose one in Settings. (${VAULT_PATH_FILE} is missing)`);
  }
}

// Which optional sources this vault allows. Read lazily and kept once read; a failed read
// (iCloud can refuse a background process for a moment) is retried after 30 s instead of
// switching sources off until the next restart.
function collectors(vault: string): { has(name: string): boolean; list(): string[] } {
  let known: Set<string> | null = null;
  let triedAt = 0;
  return {
    has(name: string) {
      if (!known && Date.now() - triedAt > 30_000) {
        triedAt = Date.now();
        // the copy install.sh puts on local disk first, then the vault's own
        // install.sh's local copy belongs to the automation's vault: use it only when HQ shows that vault
        let automationVault = "";
        try {
          automationVault = fs.readFileSync(VAULT_PATH_FILE, "utf8").trim();
        } catch {
          /* no automation installed */
        }
        const local = automationVault === vault ? [path.join(home, ".local/share/vault-automation/bin/collectors.json")] : [];
        const files = [...local, path.join(vault, "_tools/automation/collectors.json")];
        let missing = 0;
        for (const f of files) {
          try {
            known = new Set((JSON.parse(fs.readFileSync(f, "utf8")) as { collectors: string[] }).collectors);
            break;
          } catch (e) {
            if ((e as NodeJS.ErrnoException).code === "ENOENT") missing++;
          }
        }
        if (!known && missing === files.length) known = new Set(["vault", "taskwarrior"]); // no file anywhere: core only
      }
      return known?.has(name) ?? false;
    },
    list() {
      this.has(""); // read the file if it hasn't been yet
      return known ? [...known] : [];
    },
  };
}

const chosen = vaultPath();
const vault = chosen.path;

export const config = {
  port: Number(process.env.HQ_PORT ?? 4320),
  host: "127.0.0.1",
  vault,
  vaultName: path.basename(vault),
  vaultFrom: chosen.from,
  // optional sources (server/extensions.ts) run only where the vault allows them
  collectors: collectors(vault),
  automation: path.join(home, ".local/share/vault-automation"),
  state: STATE,
  task: process.env.TASK_BIN ?? saved().task ?? "/opt/homebrew/bin/task",
  // Ask runs on local Ollama only; nothing leaves the machine
  ollama: {
    url: process.env.OLLAMA_URL ?? "http://127.0.0.1:11434",
    chatModel: process.env.OLLAMA_CHAT_MODEL ?? "llama3.1:8b",
    embedModel: process.env.OLLAMA_EMBED_MODEL ?? "nomic-embed-text",
  },
  webDist: path.join(import.meta.dirname, "..", "web", "dist"),
} as const;

// Link that opens a note in this vault in Obsidian.
export const obsidian = (rel: string) =>
  `obsidian://open?vault=${encodeURIComponent(config.vaultName)}&file=${encodeURIComponent(rel.replace(/\.md$/, ""))}`;

// Link that opens a note in HQ's own reader.
export const noteHref = (rel: string) => `#/notes/${encodeURIComponent(rel)}`;

// Folders whose checkboxes are tasks; mirrors task-sync.py's EXCLUDE.
export const TASK_SCOPE_EXCLUDE = ["00-Inbox/", "06-Memory/", "08-Archive/", "_templates/", "_tools/"];
