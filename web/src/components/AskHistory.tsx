import type { ConversationSummary } from "../bindings/ConversationSummary.ts";

/** Saved conversations, newest first; each is a note in 06-Memory/conversations/. */
export default function AskHistory({
  items,
  current,
  error,
  onOpen,
  onNew,
}: {
  items: ConversationSummary[];
  current: string | null;
  error: string | null;
  onOpen: (id: string) => void;
  onNew: () => void;
}) {
  return (
    <nav className="ask-history" aria-label="Conversations">
      <div className="ask-history-head">
        <span className="label">conversations</span>
        <button type="button" className="link-button" onClick={onNew} title="Start a new conversation">
          + new
        </button>
      </div>
      {error && <p className="is-error ask-history-note">{error}</p>}
      {!error && items.length === 0 && <p className="ask-history-note">none yet. Each conversation is saved as a note you can search later.</p>}
      <ol>
        {items.map((c) => (
          <li key={c.id}>
            <button type="button" className={`ask-history-item ${c.id === current ? "is-current" : ""}`} aria-current={c.id === current ? "true" : undefined} onClick={() => onOpen(c.id)} title={c.title}>
              <span className="ask-history-title">{c.title}</span>
              <span className="ask-history-meta">
                {c.updated} · {c.turns} {c.turns === 1 ? "turn" : "turns"}
              </span>
            </button>
          </li>
        ))}
      </ol>
    </nav>
  );
}
