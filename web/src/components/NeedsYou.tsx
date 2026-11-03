import { useEffect, useMemo, useState } from "react";
import type { Need } from "../../../server/types.ts";
import type { Notify } from "../App.tsx";
import { noteUrl, post, useData } from "../api.ts";

export default function NeedsYou({ notify, onChange }: { notify: Notify; onChange: () => void }) {
  const { data, error, reload } = useData<Need[]>("/api/needs");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<string | null>(null);
  const [why, setWhy] = useState("");
  const [busy, setBusy] = useState(false);

  const items = data ?? [];
  const selected = useMemo(() => items.find((n) => n.id === selectedId) ?? items[0] ?? null, [items, selectedId]);

  useEffect(() => {
    setOutcome(null);
    setWhy("");
  }, [selected?.id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea") || !items.length || !selected) return;
      const i = items.indexOf(selected);
      if (e.key === "ArrowDown") setSelectedId(items[Math.min(i + 1, items.length - 1)]!.id);
      if (e.key === "ArrowUp") setSelectedId(items[Math.max(i - 1, 0)]!.id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items, selected]);

  async function record() {
    if (!selected || !outcome) return;
    setBusy(true);
    try {
      const r = await post<{ result: string }>("/api/needs/decide", { id: selected.id, outcome, why });
      notify(`${r.result}: ${selected.title}`);
      setSelectedId(null);
      reload();
      onChange();
    } catch (e) {
      notify((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  }

  if (error) return <p className="empty">Could not load the queue: {error}</p>;
  if (!data) return <p className="empty">Reading the vault…</p>;
  if (!items.length) {
    return (
      <div className="empty is-large">
        <p>Nothing is waiting on you.</p>
        <p className="empty-hint">Overdue tasks, inbox captures, proposals, alerts and sync errors land here when they need a decision.</p>
      </div>
    );
  }

  const chosen = selected?.outcomes.find((o) => o.key === outcome);
  return (
    <section className="needs">
      <aside className="needs-list" aria-label="Waiting on you">
        <header>
          <h1>
            Needs you <span className="count">{items.length}</span>
          </h1>
          <p>Most urgent first. Use ↑ and ↓ to move.</p>
        </header>
        <ol>
          {items.map((n) => (
            <li key={n.id}>
              <button className={`need-row kind-${n.kind} ${selected?.id === n.id ? "is-selected" : ""}`} onClick={() => setSelectedId(n.id)}>
                <span className="need-label">{n.label}</span>
                <span className="need-title">{n.title}</span>
                <span className="need-detail">{n.detail}</span>
              </button>
            </li>
          ))}
        </ol>
      </aside>

      {selected && (
        <article className="need-detail-pane" aria-live="polite">
          <p className={`chip kind-${selected.kind}`}>{selected.label}</p>
          <h2>{selected.title}</h2>
          <p className="lede">{selected.detail}</p>

          <fieldset className="outcomes">
            <legend>Choose the outcome</legend>
            {selected.outcomes.map((o) => (
              <label key={o.key} className={`outcome ${outcome === o.key ? "is-chosen" : ""}`}>
                <input type="radio" name="outcome" value={o.key} checked={outcome === o.key} onChange={() => setOutcome(o.key)} />
                <span className="outcome-label">{o.label}</span>
                <span className="outcome-hint">{o.hint}</span>
              </label>
            ))}
          </fieldset>

          <label className="why">
            <span>Why this is the right call</span>
            <textarea
              value={why}
              onChange={(e) => setWhy(e.target.value)}
              rows={3}
              placeholder="The reasoning future-you will want. Saved in your decision log."
            />
          </label>

          <button className="button is-primary is-wide" disabled={!outcome || busy} onClick={record}>
            {busy ? "Recording…" : chosen ? `Record: ${chosen.label}` : "Choose an outcome"}
          </button>

          {/* evidence last and collapsed: the decision inputs stay above the fold */}
          <details className="evidence">
            <summary>
              Evidence
              {selected.evidence.rel && <span className="evidence-rel">{selected.evidence.rel.replace(/^.*\//, "").replace(/\.md$/, "")}</span>}
              {selected.evidence.when && <span className="evidence-when">{selected.evidence.when}</span>}
            </summary>
            {selected.evidence.rel && (
              <a className="note-link" href={noteUrl(selected.evidence.rel)}>
                Open {selected.evidence.rel}
              </a>
            )}
            {selected.evidence.excerpt && <pre className="evidence-text">{selected.evidence.excerpt}</pre>}
          </details>
        </article>
      )}
    </section>
  );
}
