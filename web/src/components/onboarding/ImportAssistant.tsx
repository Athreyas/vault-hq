// The restructuring assistant for an existing (Obsidian) vault: what the analysis found,
// the plan as groups of tickable changes, apply only what is ticked, then undo if wanted.

import { useEffect, useMemo, useState } from "react";
import {
  type ImportAnalysis,
  importAnalyze,
  importApply,
  importUndo,
  message,
  type RestructureReport,
  unavailable,
} from "../../api/onboarding.ts";
import type { PlanItem } from "../../bindings/PlanItem.ts";
import { fmtBytes, NotBuilt, Status } from "./parts.tsx";

const GROUPS: { id: string; title: string; hint: string }[] = [
  { id: "folders", title: "Folders", hint: "the standard folders, and familiar ones moved into place" },
  { id: "frontmatter", title: "Frontmatter", hint: "title, type, dates and author for notes missing them" },
  { id: "loose", title: "Loose files at the top", hint: "moved into the inbox to be filed" },
  { id: "tags", title: "Tag suggestions", hint: "guesses; tick the ones you agree with" },
];

/** The plan with the user's ticks applied. */
export function withTicks(items: PlanItem[], ticked: Set<string>): PlanItem[] {
  return items.map((i) => ({ ...i, ticked: ticked.has(i.id) }));
}

export default function ImportAssistant({ path, onAdd }: { path: string; onAdd: () => void }) {
  const [a, setA] = useState<ImportAnalysis | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<RestructureReport | null>(null);
  const [undone, setUndone] = useState<RestructureReport | null>(null);

  const analyse = () => {
    setA(null);
    setError(null);
    importAnalyze(path).then(
      (r) => {
        setA(r);
        setTicked(new Set(r.plan.items.filter((i) => i.ticked).map((i) => i.id)));
      },
      (e: unknown) => (unavailable(e) ? setMissing(true) : setError(message(e))),
    );
  };
  useEffect(analyse, [path]);

  const byGroup = useMemo(() => {
    const m = new Map<string, PlanItem[]>();
    for (const i of a?.plan.items ?? []) m.set(i.group, [...(m.get(i.group) ?? []), i]);
    return m;
  }, [a]);

  if (missing) return <NotBuilt what="The import assistant" />;
  if (error) return <Status error={error} />;
  if (!a) return <p className="ob-muted">Reading {path}…</p>;

  const toggle = (ids: string[], on: boolean) =>
    setTicked((s) => {
      const n = new Set(s);
      for (const id of ids) (on ? n.add(id) : n.delete(id));
      return n;
    });

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await importApply({ root: a.plan.root, items: withTicks(a.plan.items, ticked) }));
      setUndone(null);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const undo = async () => {
    if (!report?.journal) return;
    setBusy(true);
    try {
      setUndone(await importUndo(a.root, report.journal));
      setReport(null);
      analyse();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="ob-import" aria-label="Import assistant">
      <dl className="ob-facts">
        <Fact k="notes" v={a.notes} />
        <Fact k="no frontmatter" v={a.noFrontmatter} warn />
        <Fact k="incomplete frontmatter" v={a.incompleteFrontmatter} warn />
        <Fact k="loose at the top" v={a.looseRoot.length} warn />
        <Fact k="unknown folders" v={a.unknownFolders.length} warn />
        <Fact k="broken links" v={a.brokenLinkCount} warn />
        <Fact k="untagged" v={a.untagged} />
        <Fact
          k="attachments"
          v={`${a.attachments} · ${fmtBytes(Number(a.attachmentBytes))}`}
          title={a.attachmentKinds.map((k) => `${k.kind} ${k.count}`).join(", ")}
        />
      </dl>
      {a.unreadable.length > 0 && (
        <p className="ob-status">
          {a.unreadable.length} files couldn't be read (iCloud may not have downloaded them yet): {a.unreadable.slice(0, 5).join(", ")}
        </p>
      )}

      {report ? (
        <div className="ob-box ob-stack">
          <Status
            ok={`Applied ${report.applied.length}: ${report.edited.length} notes edited, ${report.moved.length} moved, ${report.created.length} created.`}
          />
          {report.skipped.length > 0 && (
            <details open>
              <summary>{report.skipped.length} skipped</summary>
              <ul className="ob-list">
                {report.skipped.map((s, i) => (
                  <li key={i}>
                    <code>{s.id}</code> {s.reason}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {report.moved.length > 0 && (
            <details>
              <summary>what moved</summary>
              <ul className="ob-list">
                {report.moved.map((m) => (
                  <li key={m.from}>
                    <code>{m.from}</code> → <code>{m.to}</code>
                  </li>
                ))}
              </ul>
            </details>
          )}
          <div className="ob-inline">
            <button type="button" className="ob-button is-primary" onClick={onAdd}>
              add this vault
            </button>
            {report.journal && (
              <button type="button" className="ob-button" disabled={busy} onClick={() => void undo()}>
                undo these changes
              </button>
            )}
          </div>
        </div>
      ) : (
        <>
          {undone && <Status ok={`Undone: ${undone.moved.length} moved back, ${undone.edited.length} notes restored.`} />}
          {a.plan.items.length === 0 && <p className="ob-muted">Nothing to change: this vault already fits.</p>}
          {GROUPS.filter((g) => byGroup.has(g.id)).map((g) => {
            const items = byGroup.get(g.id) ?? [];
            const on = items.filter((i) => ticked.has(i.id)).length;
            return (
              <fieldset key={g.id} className="ob-box">
                <legend className="ob-label">
                  {g.title} · {on}/{items.length} ticked
                </legend>
                <p className="ob-muted">{g.hint}</p>
                <div className="ob-inline">
                  <button type="button" className="ob-link" onClick={() => toggle(items.map((i) => i.id), true)}>
                    tick all
                  </button>
                  <button type="button" className="ob-link" onClick={() => toggle(items.map((i) => i.id), false)}>
                    none
                  </button>
                </div>
                <ul className="ob-plan">
                  {items.slice(0, 400).map((i) => (
                    <li key={i.id}>
                      <label className="ob-row">
                        <input type="checkbox" checked={ticked.has(i.id)} onChange={(e) => toggle([i.id], e.target.checked)} />
                        <span className="ob-row-main">{i.summary}</span>
                      </label>
                    </li>
                  ))}
                </ul>
                {items.length > 400 && <p className="ob-muted">…and {items.length - 400} more like these.</p>}
              </fieldset>
            );
          })}
          {a.brokenLinks.length > 0 && (
            <details className="ob-box">
              <summary>
                {a.brokenLinkCount} broken links (shown, not changed)
              </summary>
              <ul className="ob-list">
                {a.brokenLinks.map((b, i) => (
                  <li key={i}>
                    <code>{b.from}</code> → <code>[[{b.target}]]</code>
                  </li>
                ))}
              </ul>
            </details>
          )}
          <div className="ob-inline">
            <button type="button" className="ob-button is-primary" disabled={busy || ticked.size === 0} onClick={() => void apply()}>
              {busy ? "applying…" : `apply ${ticked.size} ticked`}
            </button>
            <button type="button" className="ob-button" onClick={onAdd}>
              add without changes
            </button>
          </div>
        </>
      )}
      <Status error={error} />
    </section>
  );
}

function Fact({ k, v, warn, title }: { k: string; v: number | string; warn?: boolean; title?: string }) {
  return (
    <div className={`ob-fact${warn && v !== 0 ? " is-warn" : ""}`} title={title}>
      <dt>{k}</dt>
      <dd>{v}</dd>
    </div>
  );
}
