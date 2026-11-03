// Step 2: where the brain lives. Create a vault (iCloud Drive suggested for several Macs),
// add existing folders, import an Obsidian vault through the restructuring assistant.
// Several vaults are fine (home, team, …); one is primary and takes captures.

import { useEffect, useState } from "react";
import {
  type LocationSuggestion,
  locations,
  message,
  scaffoldVault,
  unavailable,
  vaultAdd,
  vaultCreate,
  vaultSetPrimary,
} from "../../api/onboarding.ts";
import ImportAssistant from "./ImportAssistant.tsx";
import type { WizardVaults } from "./Onboarding.tsx";
import { Status } from "./parts.tsx";

/** `base/name`, with the name made folder-safe. */
export function joinPath(base: string, name: string): string {
  const clean = name.trim().replace(/[/:]/g, "-");
  return `${base.replace(/\/+$/, "")}/${clean}`;
}

export default function StepBrain({ vaults }: { vaults: WizardVaults }) {
  const [mode, setMode] = useState<"new" | "existing">("new");
  return (
    <div className="ob-stack">
      <p className="ob-lede">
        A vault is a folder of notes. Make a new one, or bring the folder you already use (an Obsidian vault works as it
        is). Add as many as you like: a home brain, a team one.
      </p>

      <VaultList vaults={vaults} />

      <div className="ob-segmented" role="tablist" aria-label="Add a vault">
        <button type="button" role="tab" aria-selected={mode === "new"} className={mode === "new" ? "is-active" : ""} onClick={() => setMode("new")}>
          new vault
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === "existing"}
          className={mode === "existing" ? "is-active" : ""}
          onClick={() => setMode("existing")}
        >
          existing folder / Obsidian
        </button>
      </div>
      {mode === "new" ? <NewVault vaults={vaults} /> : <ExistingVault vaults={vaults} />}
    </div>
  );
}

function VaultList({ vaults }: { vaults: WizardVaults }) {
  const [error, setError] = useState<string | null>(null);
  if (!vaults.list.length) return <p className="ob-muted">No vaults yet.</p>;
  return (
    <fieldset className="ob-box">
      <legend className="ob-label">your vaults · primary takes captures</legend>
      {vaults.list.map((v) => (
        <label key={v.id} className="ob-row">
          <input
            type="radio"
            name="ob-primary"
            checked={v.primary}
            onChange={() =>
              vaultSetPrimary(v.id).then(
                () => {
                  vaults.add({ ...v, primary: true });
                  vaults.reload();
                },
                (e: unknown) => setError(message(e)),
              )
            }
          />
          <span className="ob-row-main">
            <b>{v.name}</b> <span className="ob-muted">{v.path}</span>
          </span>
          <span className="ob-muted">{v.notes} notes</span>
        </label>
      ))}
      <Status error={error} />
    </fieldset>
  );
}

function NewVault({ vaults }: { vaults: WizardVaults }) {
  const [spots, setSpots] = useState<LocationSuggestion[]>([]);
  const [base, setBase] = useState("");
  const [name, setName] = useState(vaults.list.length ? "team" : "home");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  useEffect(() => {
    locations().then(
      (l) => {
        setSpots(l);
        if (l[0]) setBase(l[0].path);
      },
      () => setSpots([]),
    );
  }, []);

  const path = base ? joinPath(base, name) : "";

  const create = async () => {
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const v = await vaultCreate(path, name.trim());
      vaults.add(v);
      let extra = "";
      try {
        const r = await scaffoldVault(v.id, true);
        extra = ` ${r.created.length} files and folders written: AGENTS.md, templates, a welcome note.`;
      } catch (e) {
        extra = unavailable(e) ? "" : ` The agent contract wasn't written: ${message(e)}`;
      }
      setOk(`Created ${v.name} at ${v.path}.${extra}`);
      vaults.reload();
    } catch (e) {
      setError(unavailable(e) ? "Creating vaults isn't in this build yet." : message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="ob-box ob-stack"
      onSubmit={(e) => {
        e.preventDefault();
        if (path && name.trim() && !busy) void create();
      }}
    >
      <label className="ob-field">
        <span className="ob-label">name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="home" required />
      </label>
      <fieldset className="ob-field">
        <legend className="ob-label">where</legend>
        {spots.map((s) => (
          <label key={s.path} className="ob-row">
            <input type="radio" name="ob-where" checked={base === s.path} onChange={() => setBase(s.path)} />
            <span className="ob-row-main">
              <b>{s.label}</b> {s.synced && <span className="ob-chip">syncs between Macs</span>}
              <br />
              <span className="ob-muted">{s.note}</span>
            </span>
          </label>
        ))}
        <label className="ob-row">
          <input type="radio" name="ob-where" checked={!spots.some((s) => s.path === base)} onChange={() => setBase("")} />
          <span className="ob-row-main">
            <b>Somewhere else</b>
          </span>
        </label>
        {!spots.some((s) => s.path === base) && (
          <input
            className="ob-path"
            value={base}
            onChange={(e) => setBase(e.target.value)}
            placeholder="/full/path/to/a/parent/folder"
            aria-label="Parent folder"
          />
        )}
      </fieldset>
      {path && (
        <p className="ob-muted">
          The vault will be <code>{path}</code>
        </p>
      )}
      <div>
        <button type="submit" className="ob-button is-primary" disabled={busy || !path || !name.trim()}>
          {busy ? "creating…" : "create vault"}
        </button>
      </div>
      <Status error={error} ok={ok} />
    </form>
  );
}

function ExistingVault({ vaults }: { vaults: WizardVaults }) {
  const [path, setPath] = useState("");
  const [analysing, setAnalysing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  const addIt = async (p: string) => {
    setError(null);
    try {
      const v = await vaultAdd(p);
      vaults.add(v);
      vaults.reload();
      setOk(`Added ${v.name} (${v.notes} notes).`);
      setAnalysing(null);
    } catch (e) {
      setError(unavailable(e) ? "Adding vaults isn't in this build yet." : message(e));
    }
  };

  return (
    <div className="ob-box ob-stack">
      <form
        className="ob-inline"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) setAnalysing(path.trim());
        }}
      >
        <label className="ob-field ob-grow">
          <span className="ob-label">folder</span>
          <input
            className="ob-path"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/full/path/to/your/vault"
          />
        </label>
        <button type="submit" className="ob-button is-primary" disabled={!path.trim()}>
          analyse
        </button>
        <button type="button" className="ob-button" disabled={!path.trim()} onClick={() => void addIt(path.trim())}>
          add as it is
        </button>
      </form>
      <p className="ob-muted">
        Analysing only reads. It lists what doesn't fit Vault HQ's layout and proposes changes; nothing happens until you
        tick and apply them, and an applied plan can be undone.
      </p>
      {analysing && <ImportAssistant key={analysing} path={analysing} onAdd={() => void addIt(analysing)} />}
      <Status error={error} ok={ok} />
    </div>
  );
}
