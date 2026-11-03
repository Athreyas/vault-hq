import { noteUrl } from "../api.ts";
import type { ActionDraft } from "../api/search.ts";
import AskAction from "./AskAction.tsx";
import { RichText } from "./Rich.tsx";
import type { UiTurn } from "./AskState.ts";
import { scopeLabel } from "./AskState.ts";

/** One question with its answer (streaming or finished) and the notes it read. */
export interface ActionHandlers {
  change: (d: ActionDraft) => void;
  add: () => void;
  cancel: () => void;
  ask: () => void;
}

export default function AskTurn({ turn: t, vaultNames, onAction }: { turn: UiTurn; vaultNames: Map<string, string>; onAction: ActionHandlers }) {
  const waiting = t.streaming && !t.answer;
  if (t.mode === "action" && t.action) {
    return (
      <li className="ask-turn is-action">
        <div className="ask-q">
          <p>
            <span aria-hidden="true">❯ </span>
            {t.question}
          </p>
          <span className="ask-meta">task</span>
        </div>
        <AskAction
          draft={t.action}
          added={t.added}
          busy={t.streaming}
          error={t.error}
          onChange={onAction.change}
          onAdd={onAction.add}
          onCancel={onAction.cancel}
          onAsk={t.added ? undefined : onAction.ask}
        />
      </li>
    );
  }
  return (
    <li className={`ask-turn ${t.streaming ? "is-streaming" : ""}`}>
      <div className="ask-q">
        <p>
          <span aria-hidden="true">❯ </span>
          {t.question}
        </p>
        <span className="ask-meta">
          {t.mode === "search" ? "search" : "ask"}
          {t.at ? ` · ${t.at}` : ` · ${scopeLabel(t.scope, vaultNames)}`}
          {t.semantic === false && " · keywords only"}
        </span>
      </div>

      {t.mode === "ask" && (
        <div className="ask-a" aria-live={t.streaming ? "polite" : "off"} aria-busy={t.streaming}>
          {waiting && <p className="ask-wait">{t.sources.length ? `reading ${t.sources.length} sections…` : "searching the notes…"}</p>}
          {t.answer && (t.streaming ? <p className="ask-stream">{t.answer}</p> : <RichText text={t.answer} />)}
          {t.stopped && <p className="ask-wait">stopped.</p>}
          {t.error && <p className="is-error">{t.error}</p>}
        </div>
      )}
      {t.mode === "search" && t.streaming && <p className="ask-wait">searching…</p>}
      {t.mode === "search" && t.error && <p className="is-error">{t.error}</p>}
      {t.mode === "search" && !t.streaming && !t.error && t.sources.length === 0 && <p className="ask-wait">no note matches that in this scope. Widen the scope or try other words.</p>}

      {t.sources.length > 0 && (
        <details className="ask-sources" open={t.mode === "search"}>
          <summary>
            <span className="label">{t.mode === "ask" ? `read from ${t.sources.length}` : `${t.sources.length} matches`}</span>
          </summary>
          <ol>
            {t.sources.map((s, j) => (
              <li key={`${s.id}-${j}`}>
                <a className="note-link" href={noteUrl(s.rel)} title={s.id}>
                  {s.stem}
                  {s.heading && s.heading !== s.title ? ` / ${s.heading.split(" › ").slice(1).join(" › ") || s.heading}` : ""}
                </a>
                {s.kind === "bookmark" && <span className="ask-badge">bookmark</span>}
                {s.date && <span className="ask-src-date">{s.date}</span>}
                <span className="ask-score" title="match strength">
                  {s.score.toFixed(2)}
                </span>
                <p>{s.excerpt}</p>
              </li>
            ))}
          </ol>
        </details>
      )}
      {t.sources.length === 0 && t.cited.length > 0 && (
        <p className="ask-cited">
          <span className="label">sources </span>
          {t.cited.map((c, i) => (
            <span key={c}>
              {i > 0 && " · "}
              <a className="note-link" href={noteUrl(c)}>
                {c}
              </a>
            </span>
          ))}
        </p>
      )}
    </li>
  );
}
