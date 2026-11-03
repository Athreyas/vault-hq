import { useEffect, useRef, useState } from "react";
import type { settings } from "../../../server/settings.ts";
import type { Notify } from "../App.tsx";
import { applyTheme, get, post, savedTheme, type Theme } from "../api.ts";

type Info = Awaited<ReturnType<typeof settings>>;
type Candidate = { path: string; name: string; automation: boolean; current: boolean };

/** Wait for a restarted server, then reload the page onto the new vault. */
async function reloadWhenBack(): Promise<void> {
  await new Promise((r) => setTimeout(r, 1200));
  for (let i = 0; i < 60; i++) {
    try {
      const r = await fetch("/api/status", { cache: "no-store" });
      if (r.ok) return window.location.reload();
    } catch {
      /* still restarting */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
}

function Reveal({ what }: { what: string }) {
  return (
    <button type="button" className="link-button" onClick={() => void post("/api/settings/reveal", { what })} title="Show in Finder">
      show
    </button>
  );
}

const SECTIONS = [
  ["brain", "Brain"],
  ["ai", "Local AI"],
  ["look", "Appearance"],
  ["app", "App & service"],
  ["privacy", "Privacy & account"],
] as const;
type Section = (typeof SECTIONS)[number][0];

const ago = (iso: string | null) => {
  if (!iso) return "never";
  const min = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  return min < 1 ? "just now" : min < 60 ? `${min} min ago` : min < 1440 ? `${Math.round(min / 60)} h ago` : `${Math.round(min / 1440)} d ago`;
};

function Row({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="set-row">
      <dt>{k}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function Ok({ on, yes, no }: { on: boolean; yes: string; no: string }) {
  return <span className={on ? "set-ok" : "set-bad"}>{on ? `● ${yes}` : `○ ${no}`}</span>;
}

// ⌘, : what HQ is connected to and the few things that can be changed from here.
export default function Settings({ onClose, notify }: { onClose: () => void; notify: Notify }) {
  const [info, setInfo] = useState<Info | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [section, setSection] = useState<Section>("brain");
  const [ai, setAi] = useState({ url: "", chatModel: "", embedModel: "" });
  const [busy, setBusy] = useState(false);
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const [vaults, setVaults] = useState<Candidate[]>([]);
  const [vaultPath, setVaultPath] = useState("");
  const [switching, setSwitching] = useState<string | null>(null);
  const [task, setTask] = useState("");
  const box = useRef<HTMLDivElement>(null);

  const load = () =>
    get<Info>("/api/settings").then(
      (d) => {
        setInfo(d);
        setAi({ url: d.ai.url, chatModel: d.ai.chatModel, embedModel: d.ai.embedModel });
        setTask(d.tasks.bin);
      },
      (e: Error) => setError(e.message),
    );
  useEffect(() => {
    void load();
    get<Candidate[]>("/api/settings/vaults").then(setVaults, () => setVaults([]));
    box.current?.focus();
  }, []);

  async function useVault(dir: string | null) {
    setSwitching(dir ?? "the automation's vault");
    try {
      const r = await post<{ restarting: boolean; vault: string }>("/api/settings/vault", { path: dir });
      if (r.restarting) {
        notify(`Switching to ${r.vault.split("/").pop()}…`);
        await reloadWhenBack();
      } else {
        notify("Saved. Restart HQ to switch (scripts/service.sh restart, or stop and npm start)");
        setSwitching(null);
      }
    } catch (e) {
      notify((e as Error).message, "error");
      setSwitching(null);
    }
  }

  async function saveTask() {
    try {
      setInfo(await post<Info>("/api/settings", { task }));
      notify("Saved");
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      notify("Copied");
    } catch {
      notify("Could not copy", "error");
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function saveAi() {
    if (!info) return;
    setBusy(true);
    try {
      const changed = Object.fromEntries(Object.entries(ai).filter(([k, v]) => v !== info.ai[k as keyof typeof ai]));
      const d = await post<Info>("/api/settings", { ollama: changed });
      setInfo(d);
      notify(d.ai.up ? "Saved. Local AI is reachable" : "Saved, but nothing answers at that address", d.ai.up ? "ok" : "error");
    } catch (e) {
      notify((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  }

  const aiDirty = info !== null && (ai.url !== info.ai.url || ai.chatModel !== info.ai.chatModel || ai.embedModel !== info.ai.embedModel);
  const pick = (k: "chatModel" | "embedModel", label: string, hint: string) => {
    const models = info?.ai.models ?? [];
    const lockedField = info?.ai.locked[k];
    return (
      <label className="set-field">
        <span>{label}</span>
        <input list={`models-${k}`} value={ai[k]} disabled={lockedField} onChange={(e) => setAi({ ...ai, [k]: e.target.value })} spellCheck={false} />
        <datalist id={`models-${k}`}>
          {models.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        <small>{lockedField ? "Set by an environment variable; change it there." : hint}</small>
      </label>
    );
  };

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div className="settings" role="dialog" aria-modal="true" aria-label="Settings" tabIndex={-1} ref={box} onMouseDown={(e) => e.stopPropagation()}>
        <header className="settings-head">
          <h1>settings</h1>
          <button type="button" className="button" onClick={onClose} title="Close (Esc)">
            close
          </button>
        </header>
        <nav className="settings-nav" aria-label="Settings sections">
          {SECTIONS.map(([id, label]) => (
            <button key={id} type="button" className={section === id ? "is-active" : ""} aria-current={section === id ? "true" : undefined} onClick={() => setSection(id)}>
              {label}
            </button>
          ))}
        </nav>
        <div className="settings-body">
          {error && <p className="error">{error}</p>}
          {!info && !error && <p className="empty">Reading settings…</p>}

          {info && section === "brain" && (
            <>
              <p className="lede">The brain is a folder of markdown notes. Agents, editors and HQ all read and write the same files; HQ never keeps a copy of its own.</p>
              <dl className="set-list">
                <Row k="vault">
                  <b>{info.vault.name}</b>
                </Row>
                <Row k="folder">
                  <code className="set-path">{info.vault.path}</code> <Reveal what="vault" />
                </Row>
                <Row k="chosen by">{info.vault.pathFrom}</Row>
                <Row k="storage">{info.vault.icloud ? "iCloud Drive (keep the folder pinned: Finder → Keep Downloaded)" : "a local folder"}</Row>
                <Row k="notes">
                  {info.vault.notes} readable{info.vault.unreadable > 0 && <span className="set-bad">, {info.vault.unreadable} unreadable</span>}
                </Row>
                <Row k="sections">{info.vault.folders.join(" · ")}</Row>
                <Row k="last sync">{ago(info.vault.lastSync)} (Taskwarrior ↔ notes)</Row>
                <Row k="sources on">{info.vault.collectors.join(", ") || "none"}</Row>
                <Row k="automation">
                  <code className="set-path">{info.vault.automation}</code> <Reveal what="automation" />
                </Row>
                {info.vault.from === "settings" && info.vault.automationVault && (
                  <Row k="background sync">
                    still on <code className="set-path">{info.vault.automationVault}</code>
                  </Row>
                )}
              </dl>

              <h2 className="set-h">Change vault</h2>
              <p className="set-note">HQ restarts on the new vault in a few seconds. Notes, tasks and Ask follow it; nothing in either vault changes.</p>
              {vaults.length > 0 && (
                <ul className="set-vaults">
                  {vaults.map((v) => (
                    <li key={v.path}>
                      <button type="button" className={`set-vault ${v.current ? "is-current" : ""}`} disabled={v.current || switching !== null} onClick={() => void useVault(v.path)} title={v.path}>
                        <b>{v.name}</b>
                        <small>{v.current ? "showing now" : v.automation ? "has its own automation" : "plain Obsidian vault"}</small>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <div className="set-inline">
                <input aria-label="Vault folder" placeholder="or a folder path, e.g. ~/Documents/team-vault" value={vaultPath} onChange={(e) => setVaultPath(e.target.value)} spellCheck={false} disabled={info.vault.from === "env"} />
                <button type="button" className="button is-primary" disabled={!vaultPath.trim() || switching !== null} onClick={() => void useVault(vaultPath)}>
                  {switching ? "switching…" : "use folder"}
                </button>
              </div>
              {info.vault.from === "settings" && (
                <p className="set-note">
                  HQ shows a vault chosen here, not the one the background sync serves.{" "}
                  <button type="button" className="link-button" disabled={switching !== null} onClick={() => void useVault(null)}>
                    go back to the sync's vault
                  </button>
                </p>
              )}
              {info.vault.hasAutomation && info.vault.path !== info.vault.automationVault && (
                <p className="set-note">
                  To move the background sync, hooks and metrics to this vault too, run its installer in Terminal:{" "}
                  <code className="set-path">bash "{info.vault.path}/_tools/automation/install.sh"</code>{" "}
                  <button type="button" className="link-button" onClick={() => void copy(`bash "${info.vault.path}/_tools/automation/install.sh"`)}>
                    copy
                  </button>
                </p>
              )}

              <h2 className="set-h">Taskwarrior</h2>
              <div className="set-inline">
                <input aria-label="Taskwarrior program" value={task} onChange={(e) => setTask(e.target.value)} spellCheck={false} disabled={info.tasks.locked} />
                <button type="button" className="button is-primary" disabled={task === info.tasks.bin || info.tasks.locked} onClick={() => void saveTask()}>
                  save
                </button>
              </div>
              <p className="set-note">
                <Ok on={info.tasks.installed} yes="found" no="not found at that path" />
                {info.tasks.locked ? " · set by TASK_BIN; change it there" : " · the program HQ runs for every task change (Homebrew: /opt/homebrew/bin/task)"}
              </p>
            </>
          )}

          {info && section === "ai" && (
            <>
              <p className="lede">Ask and semantic search run on a local model server (Ollama). Questions and notes never leave this Mac or your home network.</p>
              <dl className="set-list">
                <Row k="status">
                  <Ok on={info.ai.up} yes="reachable" no="not reachable" /> · <Ok on={info.ai.chatReady} yes="answer model ready" no="answer model missing" /> · <Ok on={info.ai.embedReady} yes="search model ready" no="search model missing" />
                </Row>
                <Row k="indexed">{info.ai.vectors.toLocaleString("en-US")} note passages embedded</Row>
              </dl>
              <div className="set-form">
                <label className="set-field">
                  <span>server</span>
                  <input value={ai.url} disabled={info.ai.locked.url} onChange={(e) => setAi({ ...ai, url: e.target.value })} spellCheck={false} />
                  <small>{info.ai.locked.url ? "Set by OLLAMA_URL; change it there." : "This Mac (http://127.0.0.1:11434) or a home-network address. Cloud addresses are refused."}</small>
                </label>
                {pick("chatModel", "answer model", "Writes answers. Bigger is better if the Mac has the memory (e.g. qwen2.5:14b).")}
                {pick("embedModel", "search model", "Finds passages by meaning. Changing it re-indexes the vault on the next search (about a minute).")}
                <div className="set-actions">
                  <button type="button" className="button is-primary" disabled={!aiDirty || busy} onClick={() => void saveAi()}>
                    {busy ? "saving…" : "save"}
                  </button>
                  <button type="button" className="button" disabled={busy} onClick={() => void load()}>
                    test connection
                  </button>
                  {info.ai.models.length > 0 && <small className="set-note">Installed: {info.ai.models.join(", ")}</small>}
                </div>
                {!info.ai.chatReady && info.ai.up && (
                  <p className="set-note">
                    Install the answer model with <code>ollama pull {info.ai.chatModel}</code>.
                  </p>
                )}
              </div>
            </>
          )}

          {info && section === "look" && (
            <>
              <p className="lede">Dark by default. The choice is kept in this window only.</p>
              <div className="segmented" role="group" aria-label="Theme">
                {(["dark", "light"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    className={theme === t ? "is-active" : ""}
                    onClick={() => {
                      applyTheme(t);
                      setTheme(t);
                    }}
                  >
                    {t}
                  </button>
                ))}
              </div>
              <dl className="set-list">
                <Row k="your words">IBM Plex Mono</Row>
                <Row k="machine output">JetBrains Mono</Row>
              </dl>
            </>
          )}

          {info && section === "app" && (
            <dl className="set-list">
              <Row k="version">Vault HQ {info.app.version}</Row>
              <Row k="Mac app">
                <Ok on={info.app.macApp} yes="installed in ~/Applications (⌃⌥Space brings it forward)" no="not installed; scripts/app.sh builds it" />
              </Row>
              <Row k="service">
                <Ok on={info.app.service} yes="runs at login and restarts itself (local.vault-hq)" no="not installed; scripts/service.sh install" />
              </Row>
              <Row k="server">
                pid {info.app.pid} on port {info.app.port}, up since {new Date(info.app.startedAt).toLocaleString("en-US")} (Node {info.app.node})
              </Row>
              <Row k="log">
                <code className="set-path">{info.app.log}</code> <Reveal what="log" />
              </Row>
              <Row k="settings file">
                <code className="set-path">{info.app.settingsFile}</code> <Reveal what="settings" />
              </Row>
              <Row k="state folder">
                <code className="set-path">{info.locations.state}</code> <Reveal what="state" />
              </Row>
              <Row k="extras">{info.app.extension ? "this Mac's own extras are loaded (server/personal.ts)" : "core only"}</Row>
              <Row k="keys">⌘, settings · ⌘K palette · 1–7 views · / ask · e edit a note · ⌘S save</Row>
            </dl>
          )}

          {info && section === "privacy" && (
            <>
              <p className="lede">There is no account. HQ runs only on this Mac, for the person logged in, and has nothing to sign in to.</p>
              <dl className="set-list">
                <Row k="user">
                  {info.privacy.user} on {info.privacy.host}
                </Row>
                <Row k="listens on">
                  <code>{info.privacy.bind}</code>: this Mac only, never the network
                </Row>
                <Row k="writes">only from HQ's own pages (a same-origin header check), and only to existing notes, today's inbox and planning notes, and Taskwarrior</Row>
                <Row k="talks to">the local model server above{info.app.extension ? ", plus whatever this Mac's extras fetch" : ""}. No analytics, no telemetry, no cloud model.</Row>
                <Row k="stores">nothing of its own but snoozes, settings and the search index, all in the automation state folder</Row>
              </dl>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
