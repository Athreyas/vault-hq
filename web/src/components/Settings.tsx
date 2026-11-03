import { useEffect, useRef, useState } from "react";
import type { Notify } from "../App.tsx";
import { api, notBuilt, setTheme, useCall, type AppStatus, type Settings as SettingsData, type Theme } from "../api.ts";

const SECTIONS = [
  ["vaults", "Vaults"],
  ["ai", "Local AI"],
  ["keys", "Hotkeys"],
  ["look", "Appearance"],
  ["app", "App & agents"],
  ["privacy", "Privacy"],
] as const;
type Section = (typeof SECTIONS)[number][0];

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

const errText = (e: unknown, what: string) => (notBuilt(e) ? `${what} is not in this build yet.` : (e as Error).message);

// ⌘, : the vaults HQ reads, the local model, hotkeys, theme, and where HQ keeps its files.
export default function Settings({ status, onClose, notify, onChange }: { status: AppStatus | null; onClose: () => void; notify: Notify; onChange: () => void }) {
  const [section, setSection] = useState<Section>("vaults");
  const [s, setS] = useState<SettingsData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.settings().then(setS, (e: Error) => setError(e.message));
    box.current?.focus();
  }, []);

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

  /** Save a patch; returns whether it was taken. */
  async function save(patch: Record<string, unknown>, ok = "Saved"): Promise<boolean> {
    try {
      setS(await api.updateSettings(patch));
      notify(ok);
      return true;
    } catch (e) {
      notify(errText(e, "Saving settings"), "error");
      return false;
    }
  }

  async function act(fn: () => Promise<unknown>, ok: string, what: string) {
    try {
      await fn();
      notify(ok);
      setS(await api.settings());
      onChange();
    } catch (e) {
      notify(errText(e, what), "error");
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

  const reveal = (what: string) => void api.reveal(what).catch((e: Error) => notify(e.message, "error"));

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
          {!s && !error && <p className="empty">Reading settings…</p>}
          {s && section === "vaults" && <Vaults s={s} status={status} act={act} save={save} reveal={reveal} />}
          {s && section === "ai" && <LocalAi s={s} save={save} copy={copy} />}
          {s && section === "keys" && <Keys s={s} save={save} />}
          {s && section === "look" && <Look s={s} notify={notify} />}
          {s && section === "app" && (
            <dl className="set-list">
              <Row k="version">Vault HQ {status?.version ?? "…"}</Row>
              <Row k="agents (MCP)">
                <code className="set-path">{status?.mcpCommand ?? "…"}</code>{" "}
                {status && (
                  <button type="button" className="link-button" onClick={() => void copy(status.mcpCommand)}>
                    copy
                  </button>
                )}
                <p className="set-note">Agents start this as a stdio MCP server. It uses the same vaults and the same file locks as the app.</p>
              </Row>
              <Row k="setup">
                <button type="button" className="link-button" onClick={() => window.dispatchEvent(new Event("hq:setup"))}>
                  run setup again
                </button>
                <p className="set-note">The first-run wizard: vaults, agents and their MCP connections, tasks, local AI, bookmarks. Nothing is changed without a yes.</p>
              </Row>
              <Row k="settings">
                <button type="button" className="link-button" onClick={() => reveal("settings")}>
                  show settings.json
                </button>
              </Row>
              <Row k="app files">
                <button type="button" className="link-button" onClick={() => reveal("support")}>
                  show support folder
                </button>{" "}
                ·{" "}
                <button type="button" className="link-button" onClick={() => reveal("cache")}>
                  show cache (search index, task index)
                </button>
              </Row>
              <Row k="keys">⌘K palette · ⌘, settings · 1–7 views · / ask · n new · e edit a note · ⌘S save</Row>
            </dl>
          )}
          {s && section === "privacy" && (
            <>
              <p className="lede">There is no account. HQ runs on this Mac, for the person logged in, and has nothing to sign in to.</p>
              <dl className="set-list">
                <Row k="your notes">plain markdown in the vault folders above; HQ keeps no copy of its own</Row>
                <Row k="talks to">the local model server you set (this Mac or the home network), a page you save as a bookmark (its title), and nothing else. No analytics, no telemetry, no cloud model.</Row>
                <Row k="stores">settings, and rebuildable indexes in the cache folder</Row>
                <Row k="refuses">vault folders named in the blocked list, before reading anything inside them</Row>
              </dl>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

type Save = (patch: Record<string, unknown>, ok?: string) => Promise<boolean>;
type Act = (fn: () => Promise<unknown>, ok: string, what: string) => Promise<void>;

function Vaults({ s, status, act, save, reveal }: { s: SettingsData; status: AppStatus | null; act: Act; save: Save; reveal: (what: string) => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [blocked, setBlocked] = useState("");
  const candidates = useCall(api.vaultCandidates, "candidates", { live: false });
  const known = new Set(s.vaults.map((v) => v.path));
  const infos = status?.vaults ?? [];

  return (
    <>
      <p className="lede">A vault is a folder of markdown. Add as many as you like (home, team); captures and new notes go to the primary one.</p>
      {status && status.problems.length > 0 && (
        <ul className="set-problems">
          {status.problems.map((p) => (
            <li key={p} className="set-bad">
              {p}
            </li>
          ))}
        </ul>
      )}
      <ul className="set-vaults">
        {s.vaults.map((v) => {
          const info = infos.find((i) => i.id === v.id);
          const primary = s.primaryVault === v.id;
          return (
            <li key={v.id} className={`set-vault-row ${primary ? "is-current" : ""}`}>
              <div>
                <b>{v.name}</b> {primary && <span className="chip">primary</span>}
                <code className="set-path">{v.path}</code>
                <small>{info ? `${info.notes} notes${info.unreadable ? `, ${info.unreadable} unreadable` : ""}` : "not open"}</small>
              </div>
              <div className="set-actions">
                {!primary && (
                  <button type="button" className="button" onClick={() => void act(() => api.vaultSetPrimary(v.id), `${v.name} is now primary`, "Choosing the primary vault")}>
                    make primary
                  </button>
                )}
                <button type="button" className="button" onClick={() => reveal(`vault:${v.id}`)}>
                  show
                </button>
                <button
                  type="button"
                  className="button is-danger"
                  onClick={() => {
                    if (window.confirm(`Stop showing ${v.name} in HQ? Nothing in the folder is deleted.`)) void act(() => api.vaultRemove(v.id), `Removed ${v.name} from HQ`, "Removing vaults");
                  }}
                >
                  remove
                </button>
              </div>
            </li>
          );
        })}
        {s.vaults.length === 0 && <li className="set-note">No vaults yet. Add a folder you already have, or create a new one.</li>}
      </ul>

      {candidates.data && candidates.data.some((c) => !known.has(c.path)) && (
        <>
          <h2 className="set-h">Found on this Mac</h2>
          <ul className="set-vaults">
            {candidates.data
              .filter((c) => !known.has(c.path))
              .map((c) => (
                <li key={c.path}>
                  <button type="button" className="set-vault" title={c.path} onClick={() => void act(() => api.vaultAdd(c.path), `Added ${c.name}`, "Adding vaults")}>
                    <b>{c.name}</b>
                    <small>{c.path}</small>
                  </button>
                </li>
              ))}
          </ul>
        </>
      )}

      <h2 className="set-h">Add or create</h2>
      <div className="set-form">
        <label className="set-field">
          <span>folder</span>
          <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="~/Documents/home-vault" spellCheck={false} />
          <small>An existing folder is added as it is. A new folder gets the standard layout (00-Inbox … 10-Bookmarks, AGENTS.md).</small>
        </label>
        <label className="set-field">
          <span>name (new vault)</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="home" spellCheck={false} />
        </label>
        <div className="set-actions">
          <button type="button" className="button is-primary" disabled={!path.trim()} onClick={() => void act(() => api.vaultAdd(path.trim()), "Vault added", "Adding vaults").then(() => setPath(""))}>
            add existing
          </button>
          <button type="button" className="button" disabled={!path.trim() || !name.trim()} onClick={() => void act(() => api.vaultCreate(path.trim(), name.trim()), "Vault created", "Creating vaults").then(() => setPath(""))}>
            create new
          </button>
        </div>
      </div>

      <h2 className="set-h">Blocked folder names</h2>
      <p className="set-note">HQ refuses a vault whose folder has one of these names, before it reads anything inside.</p>
      <ul className="set-chips">
        {s.blockedVaultNames.map((b) => (
          <li key={b}>
            <code>{b}</code>{" "}
            <button type="button" className="link-button" aria-label={`Unblock ${b}`} onClick={() => void save({ blockedVaultNames: s.blockedVaultNames.filter((x) => x !== b) }, `Unblocked ${b}`)}>
              ×
            </button>
          </li>
        ))}
        {s.blockedVaultNames.length === 0 && <li className="set-note">none</li>}
      </ul>
      <form
        className="set-inline"
        onSubmit={(e) => {
          e.preventDefault();
          if (blocked.trim()) void save({ blockedVaultNames: [...s.blockedVaultNames, blocked.trim()] }, `Blocked ${blocked.trim()}`).then((ok) => ok && setBlocked(""));
        }}
      >
        <input aria-label="Folder name to block" placeholder="work-vault" value={blocked} onChange={(e) => setBlocked(e.target.value)} spellCheck={false} />
        <button type="submit" className="button" disabled={!blocked.trim()}>
          block
        </button>
      </form>
    </>
  );
}

function LocalAi({ s, save, copy }: { s: SettingsData; save: Save; copy: (t: string) => Promise<void> }) {
  const [ai, setAi] = useState(s.ollama);
  const [busy, setBusy] = useState(false);
  const status = useCall(api.ollamaStatus, "ollama", { live: false });
  const rec = useCall(api.ollamaRecommend, "recommend", { live: false });
  const dirty = ai.url !== s.ollama.url || ai.chatModel !== s.ollama.chatModel || ai.embedModel !== s.ollama.embedModel;
  const o = status.data;
  const models = o?.models ?? [];

  async function submit() {
    setBusy(true);
    if (await save({ ollama: ai })) status.reload();
    setBusy(false);
  }

  const pick = (k: "chatModel" | "embedModel", label: string, hint: string) => (
    <label className="set-field">
      <span>{label}</span>
      <input list={`models-${k}`} value={ai[k]} onChange={(e) => setAi({ ...ai, [k]: e.target.value })} spellCheck={false} />
      <datalist id={`models-${k}`}>
        {models.map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <small>{hint}</small>
    </label>
  );

  return (
    <>
      <p className="lede">Ask and semantic search run on a local model server (Ollama). Questions and notes never leave this Mac or your home network.</p>
      <dl className="set-list">
        <Row k="status">
          {status.error ? (
            <span className="set-bad">{errText(status.error, "Checking the model server")}</span>
          ) : !o ? (
            "checking…"
          ) : (
            <>
              <Ok on={o.up} yes="reachable" no="not reachable" /> · <Ok on={o.chatReady} yes="answer model ready" no="answer model missing" /> · <Ok on={o.embedReady} yes="search model ready" no="search model missing" />
              {!o.up && !o.binary && <p className="set-note">Ollama is not installed. Get it from ollama.com, then come back.</p>}
            </>
          )}
        </Row>
      </dl>
      <div className="set-form">
        <label className="set-field">
          <span>server</span>
          <input value={ai.url} onChange={(e) => setAi({ ...ai, url: e.target.value })} spellCheck={false} />
          <small>This Mac (http://127.0.0.1:11434) or a home-network address. Anything else is refused.</small>
        </label>
        {pick("chatModel", "answer model", "Writes answers. Bigger is better if the Mac has the memory.")}
        {pick("embedModel", "search model", "Finds passages by meaning. Changing it rebuilds the search index.")}
        <div className="set-actions">
          <button type="button" className="button is-primary" disabled={!dirty || busy} onClick={() => void submit()}>
            {busy ? "saving…" : "save"}
          </button>
          <button type="button" className="button" disabled={busy} onClick={status.reload}>
            test connection
          </button>
          {models.length > 0 && <small className="set-note">Installed: {models.join(", ")}</small>}
        </div>
      </div>
      {rec.data && (
        <>
          <h2 className="set-h">Recommended for this Mac ({rec.data.memoryGb} GB)</h2>
          <ul className="set-pulls">
            {rec.data.pulls.map((p) => (
              <li key={p}>
                <code>{p}</code>{" "}
                <button type="button" className="link-button" onClick={() => void copy(p)}>
                  copy
                </button>
              </li>
            ))}
          </ul>
          <p className="set-note">HQ never installs models itself. Run these in Terminal.</p>
        </>
      )}
    </>
  );
}

function Keys({ s, save }: { s: SettingsData; save: Save }) {
  const [keys, setKeys] = useState(s.hotkeys);
  const dirty = keys.palette !== s.hotkeys.palette || keys.capture !== s.hotkeys.capture;
  return (
    <>
      <p className="lede">Global hotkeys work from any app. Write them like Alt+Super+Space (Super is ⌘); leave one empty to turn it off.</p>
      <div className="set-form">
        <label className="set-field">
          <span>palette</span>
          <input value={keys.palette} onChange={(e) => setKeys({ ...keys, palette: e.target.value })} spellCheck={false} />
          <small>Brings HQ forward with the palette open, or puts it away.</small>
        </label>
        <label className="set-field">
          <span>quick capture</span>
          <input value={keys.capture} onChange={(e) => setKeys({ ...keys, capture: e.target.value })} spellCheck={false} />
          <small>Opens the small capture box: ⏎ note, ⌘⏎ task, a lone link saves a bookmark.</small>
        </label>
        <div className="set-actions">
          <button type="button" className="button is-primary" disabled={!dirty} onClick={() => void save({ hotkeys: keys }, "Hotkeys saved")}>
            save
          </button>
          <button type="button" className="button" disabled={!dirty} onClick={() => setKeys(s.hotkeys)}>
            revert
          </button>
        </div>
      </div>
    </>
  );
}

function Look({ s, notify }: { s: SettingsData; notify: Notify }) {
  const [theme, setLocal] = useState<Theme>(s.theme);
  return (
    <>
      <p className="lede">Dark by default. Saved in settings, so every window and the next launch agree.</p>
      <div className="segmented" role="group" aria-label="Theme">
        {(["dark", "light"] as const).map((t) => (
          <button
            key={t}
            type="button"
            className={theme === t ? "is-active" : ""}
            onClick={() => {
              setLocal(t);
              setTheme(t).catch((e: Error) => notify(e.message, "error"));
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
  );
}
