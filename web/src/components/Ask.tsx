import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./Ask.css";
import type { AskRequest } from "../App.tsx";
import { askCancel, askStart, conversationGet, conversationsList, errorText, ollamaStatus, search, vaultsList } from "../api/search.ts";
import type { ConversationSummary, OllamaStatus, SearchScope } from "../api/search.ts";
import type { VaultInfo } from "../bindings/VaultInfo.ts";
import AskHistory from "./AskHistory.tsx";
import AskScope from "./AskScope.tsx";
import { applyEvent, fromConversation, newTurn, type UiTurn } from "./AskState.ts";
import AskTurn from "./AskTurn.tsx";

const ALL: SearchScope = { vaults: [], kinds: "both" };

function ollamaLine(o: OllamaStatus | null, err: string | null): { text: string; ok: boolean } {
  if (err) return { text: err, ok: false };
  if (!o) return { text: "checking ollama…", ok: false };
  if (!o.up || !o.hasChat) return { text: o.problem ?? "ollama is not ready", ok: false };
  return { text: o.hasEmbed ? `ollama ready · ${o.chatModel}, semantic search on` : `ollama ready · ${o.chatModel}, keyword search only`, ok: true };
}

/** The Ask tab: one long chat over the vault, saved as conversation notes. */
export default function Ask({ req, suggestions }: { req: AskRequest | null; suggestions: string[] }) {
  const [question, setQuestion] = useState("");
  const [turns, setTurns] = useState<UiTurn[]>([]); // oldest first, like a chat
  const [conversation, setConversation] = useState<string | null>(null);
  const [scope, setScope] = useState<SearchScope>(ALL);
  const [history, setHistory] = useState<ConversationSummary[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [vaults, setVaults] = useState<VaultInfo[]>([]);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [ollamaError, setOllamaError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const handled = useRef<number | null>(null);
  const busy = turns.some((t) => t.streaming);
  const vaultNames = useMemo(() => new Map(vaults.map((v) => [v.id, v.name])), [vaults]);

  const loadHistory = useCallback(() => {
    conversationsList()
      .then((l) => (setHistory(l), setHistoryError(null)))
      .catch((e: unknown) => setHistoryError(errorText(e)));
  }, []);

  useEffect(() => {
    loadHistory();
    void vaultsList().then(setVaults);
    const check = () =>
      ollamaStatus()
        .then((s) => (setOllama(s), setOllamaError(null)))
        .catch((e: unknown) => setOllamaError(errorText(e)));
    void check();
    const timer = setInterval(check, 30_000);
    return () => clearInterval(timer);
  }, [loadHistory]);

  useEffect(() => {
    if (!req) input.current?.focus();
  }, [req]);

  // keep the newest words in view while an answer streams
  const last = turns[turns.length - 1];
  useEffect(() => {
    if (last?.streaming) end.current?.scrollIntoView({ block: "nearest" });
  }, [last?.answer, last?.streaming, last?.sources.length]);

  const patch = (key: string, f: (t: UiTurn) => UiTurn) => setTurns((l) => l.map((t) => (t.key === key ? f(t) : t)));

  async function run(q: string, mode: UiTurn["mode"]) {
    q = q.trim();
    if (q.length < 2 || busy) return;
    const turn = newTurn(mode, q, scope);
    setQuestion("");
    setTurns((l) => [...l, turn]);
    if (mode === "search") {
      try {
        const r = await search(q, scope);
        patch(turn.key, (t) => ({ ...t, sources: r.hits, semantic: r.semantic, streaming: false }));
      } catch (e) {
        patch(turn.key, (t) => ({ ...t, error: errorText(e), streaming: false }));
      }
      return;
    }
    try {
      const id = await askStart(q, scope, conversation, (e) => {
        patch(turn.key, (t) => applyEvent(t, e));
        if (e.kind === "done") {
          if (e.conversation) setConversation(e.conversation);
          loadHistory();
        }
      });
      // the answer may already be over (an error before any token): only set a live id
      patch(turn.key, (t) => (t.streaming ? { ...t, reqId: id } : t));
    } catch (e) {
      patch(turn.key, (t) => ({ ...t, error: errorText(e), streaming: false }));
    }
  }

  function stop(t: UiTurn) {
    if (t.reqId) void askCancel(t.reqId);
  }

  async function open(id: string) {
    if (busy) return;
    try {
      const c = await conversationGet(id);
      setConversation(c.id);
      setTurns(fromConversation(c));
      input.current?.focus();
    } catch (e) {
      setHistoryError(errorText(e));
    }
  }

  function fresh() {
    if (busy) return;
    setConversation(null);
    setTurns([]);
    input.current?.focus();
  }

  // a question sent from the palette or the Home prompt
  useEffect(() => {
    if (!req || handled.current === req.id) return;
    handled.current = req.id;
    void run(req.q, req.mode);
  });

  const line = ollamaLine(ollama, ollamaError);
  const title = conversation ? history.find((h) => h.id === conversation)?.title : null;

  return (
    <section className="ask-chat">
      <AskHistory items={history} current={conversation} error={historyError} onOpen={(id) => void open(id)} onNew={fresh} />

      <div className="ask-main">
        <header className="ask-head">
          <h1>ask</h1>
          {title && <span className="ask-title">{title}</span>}
          <span className={`ask-ollama ${line.ok ? "is-ok" : ollama || ollamaError ? "is-error" : ""}`}>{line.text}</span>
        </header>

        {turns.length === 0 && (
          <div className="ask-empty">
            <p className="lede">Ask about a project, a decision or a day. Answers come from your notes through the local model, with the notes they used. Follow-up questions keep the conversation going.</p>
            {suggestions.length > 0 && (
              <ul className="ask-suggest">
                {suggestions.map((s) => (
                  <li key={s}>
                    <button type="button" className="link-button" onClick={() => void run(s, "ask")}>
                      ▸ {s}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        <ol className="ask-thread">
          {turns.map((t) => (
            <AskTurn key={t.key} turn={t} vaultNames={vaultNames} />
          ))}
        </ol>
        <div ref={end} />

        <div className="ask-compose">
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
                } else if (e.key === "Escape" && last?.streaming) {
                  stop(last);
                }
              }}
              placeholder={conversation ? "follow up" : "ask about a project, a decision or a day"}
              autoComplete="off"
              spellCheck={false}
            />
            <button type="button" className="button" disabled={busy || question.trim().length < 2} onClick={() => void run(question, "search")} title="Search only (⇧Enter)">
              search
            </button>
            {busy && last ? (
              <button type="button" className="button" onClick={() => stop(last)} title="Stop (Esc)">
                ■ stop
              </button>
            ) : (
              <button className="button is-primary" disabled={question.trim().length < 2}>
                {conversation ? "follow up" : "ask"}
              </button>
            )}
          </form>
          <AskScope scope={scope} vaults={vaults} onChange={setScope} />
        </div>
      </div>
    </section>
  );
}
