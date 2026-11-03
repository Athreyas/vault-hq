// Interim Ask view, moved off the v1 HTTP API onto api.ts (search, ask_start + the
// ask://event stream) so the app builds. Stream C owns Ask* and replaces this with the
// chat, conversations and history.
import { useEffect, useRef, useState } from "react";
import type { AskRequest } from "../App.tsx";
import { api, iso, noteUrl, notBuilt, on, useCall, type SearchHit, type SearchScope } from "../api.ts";
import { RichText } from "./Rich.tsx";

interface Exchange {
  id: number;
  question: string;
  mode: "ask" | "search";
  answer: string;
  sources: SearchHit[];
  error: string | null;
  done: boolean;
}

const SCOPES = [
  { days: 0, label: "all notes" },
  { days: 1, label: "one day" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
] as const;

export default function Ask({ req }: { req: AskRequest | null }) {
  const [question, setQuestion] = useState("");
  const [items, setItems] = useState<Exchange[]>([]); // newest first
  const [days, setDays] = useState<number>(0);
  const ollama = useCall(api.ollamaStatus, "ollama", { refreshMs: 30_000, live: false });
  const busy = items.some((x) => !x.done);
  const input = useRef<HTMLInputElement>(null);
  const handled = useRef<number | null>(null);
  const streams = useRef(new Map<string, number>()); // ask request id → exchange id

  useEffect(() => {
    if (!req) input.current?.focus();
  }, [req]);

  // one listener for every streamed answer, routed by request id
  useEffect(
    () =>
      on("ask://event", (ev) => {
        const xid = streams.current.get(ev.id);
        if (xid === undefined) return;
        setItems((l) =>
          l.map((x) => {
            if (x.id !== xid) return x;
            if (ev.kind === "sources") return { ...x, sources: ev.sources ?? [] };
            if (ev.kind === "token") return { ...x, answer: x.answer + (ev.token ?? "") };
            if (ev.kind === "error") return { ...x, error: ev.message ?? "The model stopped.", done: true };
            return { ...x, done: true };
          }),
        );
        if (ev.kind === "done" || ev.kind === "error") streams.current.delete(ev.id);
      }),
    [],
  );

  async function run(q: string, mode: "ask" | "search") {
    q = q.trim();
    if (q.length < 3 || busy) return;
    const scope: SearchScope = days ? { days, until: iso(new Date()) } : {};
    const id = Date.now();
    setQuestion("");
    setItems((l) => [{ id, question: q, mode, answer: "", sources: [], error: null, done: false }, ...l].slice(0, 20));
    const fail = (e: unknown) => setItems((l) => l.map((x) => (x.id === id ? { ...x, error: notBuilt(e) ? `${mode === "ask" ? "Ask" : "Search"} is not in this build yet.` : (e as Error).message, done: true } : x)));
    try {
      if (mode === "search") {
        const sources = await api.search(q, scope);
        setItems((l) => l.map((x) => (x.id === id ? { ...x, sources, done: true } : x)));
      } else {
        streams.current.set(await api.askStart(q, scope), id);
      }
    } catch (e) {
      fail(e);
    }
  }

  // a question sent from the palette or the Home prompt
  useEffect(() => {
    if (!req || handled.current === req.id) return;
    handled.current = req.id;
    void run(req.q, req.mode);
  });

  const o = ollama.data;
  const line = ollama.error ? (notBuilt(ollama.error) ? "local AI status not in this build yet" : ollama.error.message) : !o ? "checking the model server…" : !o.up ? "model server offline: run ollama serve" : !o.chatReady ? "answer model missing" : o.embedReady ? "ready, semantic search on" : "ready, keyword search only";

  return (
    <section className="ask">
      <header className="ask-head">
        <h1>ask</h1>
        <span className={`ollama-state ${o?.up && o.chatReady ? "is-ok" : o || ollama.error ? "is-error" : ""}`}>{line}</span>
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
        <select id="ask-span" value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {SCOPES.map((s) => (
            <option key={s.days} value={s.days}>
              {s.label}
            </option>
          ))}
        </select>
      </div>

      <ol className="exchanges">
        {items.map((x) => (
          <li key={x.id} className="exchange">
            <div className="ex-head">
              <p className="ex-q">
                <span aria-hidden="true">❯ </span>
                {x.question}
              </p>
              <span className="ex-meta">{x.mode}</span>
            </div>
            {x.mode === "ask" && <div className="ex-a">{x.error ? <p className="is-error">{x.error}</p> : x.answer ? <RichText text={x.answer} /> : <p className="ask-wait">searching the notes, then reading the best matches…</p>}</div>}
            {x.mode === "search" && x.error && <p className="is-error">{x.error}</p>}
            {x.mode === "search" && !x.done && <p className="ask-wait">searching…</p>}
            {x.sources.length > 0 && (
              <div className="sources">
                <h3>{x.mode === "ask" ? "read from" : `${x.sources.length} matching notes`}</h3>
                <ol>
                  {x.sources.map((s, j) => (
                    <li key={j}>
                      <a className="note-link" href={noteUrl(s.note)} title={s.note}>
                        {s.title}
                        {s.heading && s.heading !== s.title ? ` / ${s.heading}` : ""}
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
            {x.mode === "search" && x.done && !x.error && x.sources.length === 0 && <p className="ask-wait">no note matches that in this scope.</p>}
          </li>
        ))}
      </ol>
    </section>
  );
}
