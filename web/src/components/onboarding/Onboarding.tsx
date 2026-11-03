// The first-run wizard (SPEC 11): seven steps, each skippable, re-runnable from Settings.
// Keyboard: Alt+→ next, Alt+← back, 1–7 jump to a step (when not typing).
// The shell shows it while `onboarding_state().done` is false and calls `onFinish` after.

import { useCallback, useEffect, useRef, useState } from "react";
import { message, onboardingState, setStep, STEPS, type Step, type VaultInfo, vaultsList } from "../../api/onboarding.ts";
import "./onboarding.css";
import StepAgents from "./StepAgents.tsx";
import StepBrain from "./StepBrain.tsx";
import { StepAI, StepBookmarks, StepTasks } from "./StepMore.tsx";

export const STEP_TITLES: Record<Step, string> = {
  welcome: "Welcome",
  brain: "Your brain",
  agents: "Your agents",
  tasks: "Tasks",
  ai: "Local AI",
  bookmarks: "Bookmarks",
  done: "Done",
};

/** The step after (or before) `step`, clamped to the ends. */
export function neighbour(step: Step, by: 1 | -1): Step {
  const i = STEPS.indexOf(step) + by;
  return STEPS[Math.max(0, Math.min(STEPS.length - 1, i))] ?? step;
}

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && Boolean(t.closest("input, textarea, select, [contenteditable]"));

/** What the steps share: the vaults added so far, and which one prompts are written for. */
export interface WizardVaults {
  list: VaultInfo[];
  /** Vaults this run added, kept even if `vaults_list` isn't in this build. */
  add: (v: VaultInfo) => void;
  reload: () => void;
  primary: VaultInfo | null;
}

export default function Onboarding({ onFinish }: { onFinish: () => void }) {
  const [step, setLocal] = useState<Step>("welcome");
  const [visited, setVisited] = useState<Set<Step>>(() => new Set(["welcome"]));
  const [vaults, setVaults] = useState<VaultInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const main = useRef<HTMLElement>(null);

  useEffect(() => {
    onboardingState().then(
      (s) => {
        const st = STEPS.find((x) => x === s.step);
        if (st && st !== "done") setLocal(st);
      },
      () => {
        /* no saved state: start at the beginning */
      },
    );
  }, []);

  const reload = useCallback(() => {
    vaultsList().then(
      (list) =>
        setVaults((mine) => {
          const ids = new Set(list.map((v) => v.id));
          return [...list, ...mine.filter((v) => !ids.has(v.id))];
        }),
      () => {
        /* vaults_list not in this build: keep what this run added */
      },
    );
  }, []);
  useEffect(reload, [reload]);

  const add = useCallback((v: VaultInfo) => {
    setVaults((list) => {
      const rest = list.filter((x) => x.id !== v.id);
      const next = [...rest, v];
      return v.primary ? next.map((x) => ({ ...x, primary: x.id === v.id })) : next;
    });
  }, []);

  const go = useCallback((to: Step) => {
    setLocal(to);
    setVisited((s) => new Set(s).add(to));
    setError(null);
    setStep(to).catch((e: unknown) => {
      if (to === "done") setError(message(e));
    });
    requestAnimationFrame(() => main.current?.focus());
  }, []);

  const finish = useCallback(() => {
    setStep("done").then(onFinish, (e: unknown) => {
      // not able to save that we finished: still let the user in
      setError(message(e));
      onFinish();
    });
  }, [onFinish]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        go(neighbour(step, 1));
      } else if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        go(neighbour(step, -1));
      } else if (!typing(e.target) && !e.metaKey && !e.ctrlKey && !e.altKey && /^[1-7]$/.test(e.key)) {
        const s = STEPS[Number(e.key) - 1];
        if (s) go(s);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, step]);

  const primary = vaults.find((v) => v.primary) ?? vaults[0] ?? null;
  const wv: WizardVaults = { list: vaults, add, reload, primary };
  const index = STEPS.indexOf(step);

  return (
    <div className="ob">
      <header className="ob-head">
        <div className="ob-brand">vault-hq · setup</div>
        <nav aria-label="Setup steps">
          <ol className="ob-steps">
            {STEPS.map((s, i) => (
              <li key={s}>
                <button
                  type="button"
                  className={`ob-step${s === step ? " is-current" : ""}${visited.has(s) && s !== step ? " is-visited" : ""}`}
                  aria-current={s === step ? "step" : undefined}
                  onClick={() => go(s)}
                >
                  <kbd>{i + 1}</kbd> {STEP_TITLES[s]}
                </button>
              </li>
            ))}
          </ol>
        </nav>
        <div className="ob-progress" role="progressbar" aria-valuemin={1} aria-valuemax={STEPS.length} aria-valuenow={index + 1}>
          <div style={{ width: `${((index + 1) / STEPS.length) * 100}%` }} />
        </div>
      </header>

      <main className="ob-main" ref={main} tabIndex={-1} aria-labelledby="ob-title">
        <h1 id="ob-title" className="ob-title">
          <span className="ob-muted">
            {index + 1}/{STEPS.length}
          </span>{" "}
          {STEP_TITLES[step]}
        </h1>
        {step === "welcome" && <Welcome />}
        {step === "brain" && <StepBrain vaults={wv} />}
        {step === "agents" && <StepAgents vaults={wv} />}
        {step === "tasks" && <StepTasks />}
        {step === "ai" && <StepAI />}
        {step === "bookmarks" && <StepBookmarks />}
        {step === "done" && <Done vaults={vaults} />}
        {error && (
          <p className="ob-status is-error" role="alert">
            {error}
          </p>
        )}
      </main>

      <footer className="ob-foot">
        <button type="button" className="ob-button" disabled={index === 0} onClick={() => go(neighbour(step, -1))}>
          ← back
        </button>
        <span className="ob-hint">
          <kbd>⌥←</kbd> <kbd>⌥→</kbd> move · <kbd>1</kbd>–<kbd>7</kbd> jump · every step can be skipped
        </span>
        {step === "done" ? (
          <button type="button" className="ob-button is-primary" onClick={finish}>
            open home →
          </button>
        ) : (
          <span className="ob-foot-right">
            {step !== "welcome" && (
              <button type="button" className="ob-link" onClick={() => go(neighbour(step, 1))}>
                skip
              </button>
            )}
            <button type="button" className="ob-button is-primary" onClick={() => go(neighbour(step, 1))}>
              {step === "welcome" ? "start →" : "next →"}
            </button>
          </span>
        )}
      </footer>
    </div>
  );
}

function Welcome() {
  return (
    <div className="ob-prose">
      <p className="ob-lede">
        Vault HQ is the front end to your second brain: one folder of plain markdown that you, your editors and your AI
        agents all write into.
      </p>
      <ul className="ob-list">
        <li>
          <b>Local only.</b> Notes stay on this Mac. Nothing is sent anywhere except to a model server you choose on this
          Mac or your home network.
        </li>
        <li>
          <b>Plain files.</b> Every note, task and bookmark is a <code>.md</code> file any tool can read. Delete the app
          and your brain is still there.
        </li>
        <li>
          <b>Your agents, one brain.</b> Claude, Cursor, Codex and others get the same rules for where to write and how,
          and connect through Vault HQ's MCP server when they can.
        </li>
        <li>
          <b>Nothing without a yes.</b> Every file outside your vault that setup would change is shown to you first, and
          backed up.
        </li>
      </ul>
      <p className="ob-muted">Takes about five minutes. Every step can be skipped and re-run from Settings.</p>
    </div>
  );
}

function Done({ vaults }: { vaults: VaultInfo[] }) {
  return (
    <div className="ob-prose">
      <p className="ob-lede">You're set up.</p>
      {vaults.length ? (
        <ul className="ob-list">
          {vaults.map((v) => (
            <li key={v.id}>
              <b>{v.name}</b> <span className="ob-muted">{v.path}</span> {v.primary && <span className="ob-chip">primary</span>}
            </li>
          ))}
        </ul>
      ) : (
        <p className="ob-muted">No vault yet. Add one from Settings → Brain whenever you're ready.</p>
      )}
      <p>
        Capture anything with <kbd>⌥⌘Space</kbd>, open the palette with <kbd>⌃⌥Space</kbd>. Run this setup again from
        Settings.
      </p>
    </div>
  );
}
