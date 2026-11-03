// Step 3: the agents. Tick the ones you use; for each, copy its prompt/skill text (where
// the brain is and how to write into it) and, where it speaks MCP, connect it: HQ shows
// the exact file and diff (or the command to run) and writes only on "yes".

import { useEffect, useState } from "react";
import {
  type Agent,
  agentPrompt,
  agentsCatalog,
  type ConfigApplied,
  type ConfigDiff,
  configApply,
  configPreview,
  message,
  OnboardingError,
  unavailable,
} from "../../api/onboarding.ts";
import type { WizardVaults } from "./Onboarding.tsx";
import { Copyable, Diff, NotBuilt, Status } from "./parts.tsx";

export default function StepAgents({ vaults }: { vaults: WizardVaults }) {
  const [catalog, setCatalog] = useState<Agent[] | null>(null);
  const [missing, setMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [vaultId, setVaultId] = useState<string>("");

  useEffect(() => {
    agentsCatalog().then(
      (c) => {
        setCatalog(c);
        setChosen(new Set(c.filter((a) => a.detected).map((a) => a.id)));
      },
      (e: unknown) => (unavailable(e) ? setMissing(true) : setError(message(e))),
    );
  }, []);
  useEffect(() => {
    if (!vaultId && vaults.primary) setVaultId(vaults.primary.id);
  }, [vaults.primary, vaultId]);

  if (missing) return <NotBuilt what="Agent setup" />;
  if (error) return <Status error={error} />;
  if (!catalog) return <p className="ob-muted">Looking for agents…</p>;

  const toggle = (id: string, on: boolean) =>
    setChosen((s) => {
      const n = new Set(s);
      if (on) n.add(id);
      else n.delete(id);
      return n;
    });

  return (
    <div className="ob-stack">
      <p className="ob-lede">
        Every agent gets the same rules: where the brain is, what each folder is for, frontmatter with its own name as
        author, inbox first, never delete. Agents that speak MCP also get Vault HQ's tools, which write with the same
        locks as the app.
      </p>

      {vaults.list.length > 1 && (
        <label className="ob-field">
          <span className="ob-label">prompts point at</span>
          <select value={vaultId} onChange={(e) => setVaultId(e.target.value)}>
            {vaults.list.map((v) => (
              <option key={v.id} value={v.id}>
                {v.name} · {v.path}
              </option>
            ))}
          </select>
        </label>
      )}
      {!vaults.list.length && (
        <p className="ob-status">Add a vault in the previous step first: the prompts need to say where it is.</p>
      )}

      <fieldset className="ob-box">
        <legend className="ob-label">which agents do you use?</legend>
        <div className="ob-agent-picks">
          {catalog.map((a) => (
            <label key={a.id} className="ob-row">
              <input type="checkbox" checked={chosen.has(a.id)} onChange={(e) => toggle(a.id, e.target.checked)} />
              <span className="ob-row-main">
                <b>{a.name}</b> {a.detected && <span className="ob-chip">found on this Mac</span>}{" "}
                {a.mcp && <span className="ob-chip">MCP</span>}
                <br />
                <span className="ob-muted">{a.blurb}</span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>

      {catalog
        .filter((a) => chosen.has(a.id))
        .map((a) => (
          <AgentCard key={a.id} agent={a} vaultId={vaultId} />
        ))}
    </div>
  );
}

function AgentCard({ agent, vaultId }: { agent: Agent; vaultId: string }) {
  const [prompt, setPrompt] = useState<string | null>(null);
  const [promptErr, setPromptErr] = useState<string | null>(null);
  const [showPrompt, setShowPrompt] = useState(false);

  useEffect(() => {
    if (!showPrompt || !vaultId) return;
    setPrompt(null);
    agentPrompt(agent.id, vaultId).then(setPrompt, (e: unknown) => setPromptErr(message(e)));
  }, [agent.id, vaultId, showPrompt]);

  return (
    <section className="ob-box ob-stack ob-agent" aria-label={agent.name}>
      <h2 className="ob-h2">{agent.name}</h2>

      <div>
        <p>
          <span className="ob-label">{kindLabel(agent.promptKind)}</span> {agent.promptTarget}
        </p>
        <button type="button" className="ob-button" disabled={!vaultId} onClick={() => setShowPrompt((s) => !s)} aria-expanded={showPrompt}>
          {showPrompt ? "hide" : "show"} {kindLabel(agent.promptKind)}
        </button>
        {showPrompt && prompt && <Copyable text={prompt} title={`${agent.name} · ${kindLabel(agent.promptKind)}`} />}
        {showPrompt && !prompt && !promptErr && <p className="ob-muted">Writing it…</p>}
        <Status error={promptErr} />
      </div>

      {agent.connect !== "none" ? (
        <Connect agent={agent} />
      ) : (
        <p className="ob-muted">
          {agent.name} can't run a local MCP server, so it works from the text above alone.
        </p>
      )}
    </section>
  );
}

function kindLabel(kind: string): string {
  switch (kind) {
    case "skill":
      return "skill";
    case "rule":
      return "rule (.mdc)";
    case "agents-md":
      return "AGENTS.md snippet";
    default:
      return "instructions";
  }
}

function Connect({ agent }: { agent: Agent }) {
  const [diff, setDiff] = useState<ConfigDiff | null>(null);
  const [applied, setApplied] = useState<ConfigApplied | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const preview = () => {
    setError(null);
    setApplied(null);
    configPreview(agent.id).then(setDiff, (e: unknown) => setError(message(e)));
  };

  const apply = async () => {
    if (!diff) return;
    setBusy(true);
    setError(null);
    try {
      setApplied(await configApply(agent.id, diff.baseHash));
      setDiff(null);
    } catch (e) {
      if (e instanceof OnboardingError && e.kind === "Conflict") {
        setError("The file changed after the preview, so nothing was written. Preview again.");
        setDiff(null);
      } else {
        setError(message(e));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ob-stack">
      <p>
        <span className="ob-label">connect via MCP</span>{" "}
        {agent.connect === "file" ? (
          <>
            adds a <code>vault-hq</code> server to <code>{agent.configPath}</code>; everything else in it is kept, and the
            old file is backed up.
          </>
        ) : (
          <>{agent.name} keeps its own MCP list; you run one command.</>
        )}
      </p>
      {!diff && !applied && (
        <div>
          <button type="button" className="ob-button" onClick={preview}>
            {agent.connect === "file" ? "preview the change" : "show the command"}
          </button>
        </div>
      )}

      {diff && diff.kind === "command" && diff.command && (
        <>
          <Copyable text={diff.command} title="run in a terminal" />
          {diff.note && <p className="ob-muted">{diff.note}</p>}
        </>
      )}

      {diff && diff.kind === "file" && (
        <div className="ob-stack">
          <p className="ob-muted">
            {diff.exists ? "Changes to" : "Creates"} <code>{diff.path}</code>
          </p>
          <Diff diff={diff.diff} />
          {diff.note && <p className="ob-status">{diff.note}</p>}
          {diff.changed ? (
            <div className="ob-inline">
              <button type="button" className="ob-button is-primary" disabled={busy} onClick={() => void apply()}>
                {busy ? "writing…" : "yes, write it"}
              </button>
              <button type="button" className="ob-link" onClick={() => setDiff(null)}>
                cancel
              </button>
            </div>
          ) : (
            <Status ok={`${agent.name} is already connected.`} />
          )}
        </div>
      )}

      {applied && (
        <Status
          ok={
            applied.changed
              ? `Written to ${applied.path}${applied.backup ? `; the old file is at ${applied.backup}` : ""}. Restart ${agent.name} to pick it up.`
              : `${agent.name} was already connected; nothing written.`
          }
        />
      )}
      <Status error={error} />
    </div>
  );
}
