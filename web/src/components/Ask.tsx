import { useEffect, useRef, useState } from "react";
import type { AskRequest } from "../App.tsx";
import { ASK_KEEP, get, iso, loadAsks, noteUrl, post, saveAsks, scopeLabel, useData } from "../api.ts";
import type { AskSource, Exchange } from "../api.ts";
import { RichText } from "./Rich.tsx";

interface OllamaStatus {
  up: boolean;
  chat: boolean;
  embed: boolean;
}

const SCOPES = [
  { span: 0, label: "all notes" },
  { span: 1, label: "one day" },
  { span: 3, label: "3 days" },
  { span: 7, label: "7 days" },
  { span: 30, label: "30 days" },
] as const;

const sameScope = (a: Exchange["scope"], b: Exchange["scope"]) => a.span === b.span && (a.span === 0 || a.date === b.date);

export default function Ask({ req, suggestions }: { req: AskRequest | null; suggestions: string[] }) {
  const [question, setQuestion] = useState("");
  const [items, setItems] = useState<Exchange[]>(loadAsks); // newest first
  const [span, setSpan] = useState<number>(0);
  const [date, setDate] = useState(() => iso(new Date()));
  const ollama = useData<OllamaStatus>("/api/ollama", 30_000);
  const busy = items.some((x) => !x.done);
  const input = useRef<HTMLInputElement>(null);
  const handled = useRef<number | null>(null);

  useEffect(() => saveAsks(items), [items]);
  useEffect(() => {
    if (!req) input.current?.focus();
  }, [req]);

  async function run(q: string, mode: "ask" | "search", force = false) {
    q = q.trim();
    if (q.length < 3 || busy) return;
    const scope = { span, date };
    const cached = items.find((x) => x.question === q && x.mode === mode && sameScope(x.scope, scope) && x.done && !x.error);
    if (cached && !force) {
      setItems((l) => [cached, ...l.filter((x) => x.id !== cached.id)]);
      return;
    }
    const id = Date.now();
    setQuestion("");
    setItems((l) => [{ id, question: q, mode, scope, answer: null, sources: [], error: null, done: false }, ...l.filter((x) => !(force && x.question === q && x.mode === mode && sameScope(x.scope, scope)))].slice(0, ASK_KEEP));
    const finish = (patch: Partial<Exchange>) => setItems((l) => l.map((x) => (x.id === id ? { ...x, ...patch, done: true } : x)));
    const range = span ? `&date=${date}&span=${span}` : "";
    try {
      if (mode === "search") {
        const r = await get<{ sources: AskSource[] }>(`/api/search?q=${encodeURIComponent(q)}${range}`);
        finish({ sources: r.sources });
      } else {
        const r = await post<{ answer: string; sources: AskSource[] }>("/api/ask", span ? { question: q, date, span } : { question: q });
        finish({ answer: r.answer, sources: r.sources });
      }
    } catch (e) {
      finish({ error: (e as Error).message });
    }
  }

  // a question sent from the palette or the Home prompt
  useEffect(() => {
    if (!req || handled.current === req.id) return;
    handled.current = req.id;
    void run(req.q, req.mode);
  });

  const o = ollama.data;
  const ollamaLine = !o ? "checking ollama…" : !o.up ? "ollama offline: run ollama serve" : !o.chat ? "chat model missing in ollama" : o.embed ? "ollama ready, semantic search on" : "ollama ready, keyword search only";
  const ok = Boolean(o?.up && o.chat);

  return (
    <section className="ask">
      <header className="ask-head">
        <h1>ask</h1>
        <span className={`ollama-state ${ok ? "is-ok" : o ? "is-error" : ""}`}>{ollamaLine}</span>
      </header>

      <form
        className="prompt is-big"
        onSubmit={(e) => {
          e.preventDefault();
          void run(question, "ask");
        }}
      >
        <span aria-hidden="true">❯</span>
        <label className="sr-only" htmlFor="ask-q">
          Question
        </label>
        <input
          id="ask-q"
          ref={input}
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && e.shiftKey) {
              e.preventDefault();
              void run(question, "search");
            }
          }}
          placeholder="ask about a project, a decision or a day"
          autoComplete="off"
          spellCheck={false}
        />
        <button type="button" className="button" disabled={busy || question.trim().length < 3} onClick={() => void run(question, "search")}>
          search
        </button>
        <button className="button is-primary" disabled={busy || question.trim().length < 3}>
          {busy ? "reading…" : "ask"}
        </button>
      </form>

      <div className="scope">
        <label htmlFor="ask-span">scope</label>
        <select id="ask-span" value={span} onChange={(e) => setSpan(Number(e.target.value))}>
          {SCOPES.map((s) => (
            <option key={s.span} value={s.span}>
              {s.label}
            </option>
          ))}
        </select>
        {span > 0 && (
          <>
            <label htmlFor="ask-date">ending</label>
            <input id="ask-date" type="date" value={date} onChange={(e) => e.target.value && setDate(e.target.value)} />
          </>
        )}
        <span className="scope-note">{span ? "notes created or updated in that window" : "every note in the vault"}</span>
      </div>

      {items.length === 0 && suggestions.length > 0 && (
        <div className="suggest">
          <p className="label">try</p>
          <ul>
            {suggestions.map((s) => (
              <li key={s}>
                <button className="link-button" onClick={() => void run(s, "ask")}>
                  ▸ {s}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <ol className="exchanges">
        {items.map((x) => (
          <li key={x.id} className="exchange">
            <div className="ex-head">
              <p className="ex-q">
                <span aria-hidden="true">❯ </span>
                {x.question}
              </p>
              <span className="ex-meta">
                {x.mode === "ask" ? "ask" : "search"} · {scopeLabel(x.scope)}
              </span>
              {x.done && (
                <button className="link-button" onClick={() => void run(x.question, x.mode, true)} disabled={busy} title="Run this again against the current notes">
                  rerun
                </button>
              )}
            </div>
            {x.mode === "ask" && (
              <div className="ex-a">
                {x.answer !== null ? <RichText text={x.answer} /> : x.error ? <p className="is-error">{x.error}</p> : <p className="ask-wait">searching the notes, then reading the best matches. The first question after a change can take a minute.</p>}
              </div>
            )}
            {x.mode === "search" && !x.done && <p className="ask-wait">searching…</p>}
            {x.mode === "search" && x.error && <p className="is-error">{x.error}</p>}
            {x.sources.length > 0 && (
              <div className="sources">
                <h3>{x.mode === "ask" ? "read from" : `${x.sources.length} matching notes`}</h3>
                <ol>
                  {x.sources.map((s, j) => (
                    <li key={j}>
                      <a className="note-link" href={noteUrl(s.rel)} title={s.rel}>
                        {s.stem}
                        {s.heading && s.heading !== s.stem ? ` / ${s.heading}` : ""}
                      </a>
                      <span className="score" title="match strength">
                        {s.score.toFixed(2)}
                      </span>
                      <p>{s.excerpt}</p>
                    </li>
                  ))}
                </ol>
              </div>
            )}
            {x.mode === "search" && x.done && !x.error && x.sources.length === 0 && <p className="ask-wait">no note matches that in this scope. Widen the scope or try other words.</p>}
          </li>
        ))}
      </ol>

      {items.some((x) => x.done) && (
        <nav className="recent" aria-label="Recent questions">
          <span className="label">recent</span>
          <ul>
            {items
              .filter((x) => x.done && !x.error)
              .map((x) => (
                <li key={x.id}>
                  <button
                    className="recent-item"
                    title={`${x.question}\n${x.mode} · ${scopeLabel(x.scope)}${x.sources.length ? ` · ${x.sources.length} sources` : ""}`}
                    onClick={() => {
                      setItems((l) => [x, ...l.filter((y) => y.id !== x.id)]);
                      document.getElementById("main")?.scrollTo({ top: 0 });
                    }}
                  >
                    {x.mode === "search" ? "/ " : "? "}
                    {x.question}
                  </button>
                </li>
              ))}
          </ul>
          <button className="link-button" onClick={() => setItems([])} title="Forget recent questions on this browser">
            clear
          </button>
        </nav>
      )}
    </section>
  );
}
