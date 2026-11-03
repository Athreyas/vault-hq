// Steps 4–6, which drive other streams' commands: Taskwarrior import (B), Ollama (C),
// browser bookmarks (F). Each one copes with its commands not being in this build.

import { useEffect, useState } from "react";
import {
  bookmarksEnrich,
  bookmarksImport,
  type BrowserSource,
  browserAllow,
  browsersDetect,
  countsLine,
  type ImportCounts,
  message,
  ollamaRecommend,
  ollamaStatus,
  type OllamaStatus,
  type Recommendation,
  settingsUpdate,
  tasksImport,
  unavailable,
} from "../../api/onboarding.ts";
import { Copyable, NotBuilt, Status } from "./parts.tsx";

// ---- 4 tasks ---------------------------------------------------------------------------------

export function StepTasks() {
  const [mode, setMode] = useState<"fresh" | "taskwarrior">("fresh");
  const [dry, setDry] = useState<ImportCounts | null>(null);
  const [done, setDone] = useState<ImportCounts | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);

  const run = async (dryRun: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const r = await tasksImport(dryRun);
      if (dryRun) setDry(r);
      else setDone(r);
    } catch (e) {
      if (unavailable(e)) setMissing(true);
      else setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ob-stack">
      <p className="ob-lede">
        Tasks are checkbox lines in your notes (Obsidian Tasks style), so agents and editors can read and add them too.
      </p>
      <fieldset className="ob-box">
        <label className="ob-row">
          <input type="radio" name="ob-tasks" checked={mode === "fresh"} onChange={() => setMode("fresh")} />
          <span className="ob-row-main">
            <b>Start fresh</b>
            <br />
            <span className="ob-muted">Add tasks as you go; loose ones land in 09-Planning/Tasks.md.</span>
          </span>
        </label>
        <label className="ob-row">
          <input type="radio" name="ob-tasks" checked={mode === "taskwarrior"} onChange={() => setMode("taskwarrior")} />
          <span className="ob-row-main">
            <b>Import from Taskwarrior</b>
            <br />
            <span className="ob-muted">
              Reads <code>task export</code>, writes each task into its project's note. Taskwarrior's own data is never
              touched.
            </span>
          </span>
        </label>
      </fieldset>
      {mode === "taskwarrior" &&
        (missing ? (
          <NotBuilt what="Taskwarrior import" />
        ) : (
          <div className="ob-stack">
            {!dry && (
              <div>
                <button type="button" className="ob-button" disabled={busy} onClick={() => void run(true)}>
                  {busy ? "reading…" : "dry run: show what would come in"}
                </button>
              </div>
            )}
            {dry && !done && (
              <>
                <p>Would import: {countsLine(dry)}</p>
                {Array.isArray(dry.sample) && dry.sample.length > 0 && (
                  <pre className="ob-pre">{dry.sample.slice(0, 8).join("\n")}</pre>
                )}
                <div className="ob-inline">
                  <button type="button" className="ob-button is-primary" disabled={busy} onClick={() => void run(false)}>
                    {busy ? "importing…" : "import them"}
                  </button>
                  <button type="button" className="ob-link" onClick={() => setDry(null)}>
                    cancel
                  </button>
                </div>
              </>
            )}
            {done && <Status ok={`Imported: ${countsLine(done)}`} />}
          </div>
        ))}
      <Status error={error} />
    </div>
  );
}

// ---- 5 local AI ------------------------------------------------------------------------------

/** Installed models split into embedding and chat ones, by name. */
export function splitModels(models: string[]): { embed: string[]; chat: string[] } {
  const isEmbed = (m: string) => /embed|bge|minilm|e5-|gte-/i.test(m);
  return { embed: models.filter(isEmbed), chat: models.filter((m) => !isEmbed(m)) };
}

export function StepAI() {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [rec, setRec] = useState<Recommendation | null>(null);
  const [missing, setMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [embed, setEmbed] = useState("");
  const [chat, setChat] = useState("");
  const [saved, setSaved] = useState<string | null>(null);

  const detect = () => {
    setError(null);
    ollamaStatus().then(
      (s) => {
        setStatus(s);
        const { embed: e, chat: c } = splitModels(s.models);
        setEmbed(s.embedModel && s.models.includes(s.embedModel) ? s.embedModel : (e[0] ?? ""));
        setChat(s.chatModel && s.models.includes(s.chatModel) ? s.chatModel : (c[0] ?? ""));
      },
      (e: unknown) => (unavailable(e) ? setMissing(true) : setError(message(e))),
    );
    ollamaRecommend().then(setRec, () => setRec(null));
  };
  useEffect(detect, []);

  if (missing) return <NotBuilt what="Local AI detection" />;

  const save = () =>
    settingsUpdate({ ollama: { chatModel: chat, embedModel: embed } }).then(
      () => setSaved(`Ask will use ${chat || "no chat model"} with ${embed || "no embeddings"}.`),
      (e: unknown) => setError(unavailable(e) ? "Saving settings isn't in this build yet." : message(e)),
    );

  const models = splitModels(status?.models ?? []);
  const pulls = rec?.commands ?? rec?.pulls ?? [];

  return (
    <div className="ob-stack">
      <p className="ob-lede">
        Ask answers from your notes with a model running on this Mac (Ollama). Optional: without one, Ask is search only.
      </p>
      {!status && !error && <p className="ob-muted">Looking for Ollama…</p>}
      {status && (
        <p className={status.up ? "ob-status is-ok" : "ob-status"}>
          {status.up
            ? `Ollama is running at ${status.url} with ${status.models.length} models.`
            : (status.problem ?? `Ollama isn't answering at ${status.url}.`)}{" "}
          <button type="button" className="ob-link" onClick={detect}>
            check again
          </button>
        </p>
      )}
      {status?.up && status.models.length > 0 && (
        <div className="ob-box ob-stack">
          <label className="ob-field">
            <span className="ob-label">embeddings (search)</span>
            <select value={embed} onChange={(e) => setEmbed(e.target.value)}>
              <option value="">none</option>
              {[...models.embed, ...models.chat].map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </label>
          <label className="ob-field">
            <span className="ob-label">chat (answers)</span>
            <select value={chat} onChange={(e) => setChat(e.target.value)}>
              <option value="">none</option>
              {[...models.chat, ...models.embed].map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </label>
          <div>
            <button type="button" className="ob-button is-primary" onClick={() => void save()}>
              use these
            </button>
          </div>
          <Status ok={saved} />
        </div>
      )}
      {rec && (
        <div className="ob-stack">
          <p>
            For this Mac ({rec.memoryGb} GB): <b>{rec.embed}</b> for search
            {rec.chat.length > 0 && (
              <>
                , <b>{rec.chat.join(" or ")}</b> for answers
              </>
            )}
            . Vault HQ never installs anything itself; run these in a terminal if you want them:
          </p>
          {pulls.length > 0 && <Copyable text={pulls.join("\n")} title="ollama pull" />}
          {!status?.binary && (
            <p className="ob-muted">
              Ollama itself comes from <code>ollama.com</code> or <code>brew install ollama</code>.
            </p>
          )}
        </div>
      )}
      <Status error={error} />
    </div>
  );
}

// ---- 6 bookmarks ----------------------------------------------------------------------------

export function StepBookmarks() {
  const [sources, setSources] = useState<BrowserSource[] | null>(null);
  const [missing, setMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number; lines: string[] } | null>(null);
  const [enrich, setEnrich] = useState(false);

  const detect = () =>
    browsersDetect().then(setSources, (e: unknown) => (unavailable(e) ? setMissing(true) : setError(message(e))));
  useEffect(() => void detect(), []);

  if (missing) return <NotBuilt what="Browser bookmarks" />;

  const allow = (s: BrowserSource, on: boolean) =>
    browserAllow(s.id, on).then(
      () => detect(),
      (e: unknown) => setError(message(e)),
    );

  const allowed = (sources ?? []).filter((s) => s.allowed);

  const importAll = async () => {
    setError(null);
    const lines: string[] = [];
    setProgress({ done: 0, total: allowed.length, lines });
    for (const [i, s] of allowed.entries()) {
      try {
        const r = await bookmarksImport(s.id, false);
        lines.push(`${s.label}: ${countsLine(r)}`);
      } catch (e) {
        lines.push(`${s.label}: ${message(e)}`);
      }
      setProgress({ done: i + 1, total: allowed.length, lines: [...lines] });
    }
    if (enrich) {
      try {
        await settingsUpdate({ enrichBookmarks: true });
        await bookmarksEnrich();
        lines.push("Enrichment started: tags and summaries fill in in the background.");
      } catch (e) {
        lines.push(`Enrichment: ${message(e)}`);
      }
      setProgress({ done: allowed.length, total: allowed.length, lines: [...lines] });
    }
  };

  const safariDenied = (sources ?? []).some((s) => (s.browser === "safari" || s.id === "safari") && (s.denied || s.problem));

  return (
    <div className="ob-stack">
      <p className="ob-lede">
        Bring your browser bookmarks in as notes (one per link, in 10-Bookmarks/), so search and agents can find them.
        Nothing is read until you tick a browser.
      </p>
      {!sources && !error && <p className="ob-muted">Looking for browsers…</p>}
      {sources && sources.length === 0 && <p className="ob-muted">No browser bookmarks found on this Mac.</p>}
      {sources && sources.length > 0 && (
        <fieldset className="ob-box">
          <legend className="ob-label">read bookmarks from</legend>
          {sources.map((s) => (
            <label key={s.id} className="ob-row">
              <input type="checkbox" checked={s.allowed} onChange={(e) => void allow(s, e.target.checked)} />
              <span className="ob-row-main">
                <b>{s.label}</b>{" "}
                {typeof s.count === "number" && <span className="ob-muted">{s.count} bookmarks</span>}
                {s.problem && (
                  <>
                    <br />
                    <span className="ob-muted">{s.problem}</span>
                  </>
                )}
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {safariDenied && (
        <p className="ob-status">
          Safari's bookmarks need Full Disk Access: System Settings → Privacy &amp; Security → Full Disk Access → turn on
          Vault HQ. Or, in Safari, File → Export → Bookmarks…, and import that HTML file from Settings → Bookmarks.
        </p>
      )}
      {allowed.length > 0 && (
        <div className="ob-stack">
          <label className="ob-row">
            <input type="checkbox" checked={enrich} onChange={(e) => setEnrich(e.target.checked)} />
            <span className="ob-row-main">
              Also add tags and a one-line summary with the local model <span className="ob-muted">(fetches each page; can be stopped)</span>
            </span>
          </label>
          <div>
            <button
              type="button"
              className="ob-button is-primary"
              disabled={progress !== null && progress.done < progress.total}
              onClick={() => void importAll()}
            >
              import from {allowed.length} {allowed.length === 1 ? "browser" : "browsers"}
            </button>
          </div>
        </div>
      )}
      {progress && (
        <div className="ob-stack">
          <div className="ob-progress" role="progressbar" aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.done}>
            <div style={{ width: `${progress.total ? (progress.done / progress.total) * 100 : 100}%` }} />
          </div>
          <ul className="ob-list">
            {progress.lines.map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        </div>
      )}
      <Status error={error} />
    </div>
  );
}
