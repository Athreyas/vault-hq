import { useCallback, useEffect, useState } from "react";
import {
  bookmarksImport,
  browserAllow,
  browsersDetect,
  errorMessage,
  htmlSource,
  onProgress,
  type BookmarkImportReport,
  type BrowserSource,
  type JobProgress,
} from "../api/bookmarks";

type Say = (message: string, kind?: "error") => void;

/** Live progress of the running import, from `jobs://progress`. */
function useImportProgress(): JobProgress | null {
  const [p, setP] = useState<JobProgress | null>(null);
  useEffect(() => {
    const off = onProgress("bookmarks-import", setP);
    return () => {
      off.then((f) => f()).catch(() => {});
    };
  }, []);
  return p;
}

function Progress({ p }: { p: JobProgress | null }) {
  if (!p || p.total === 0) return <p className="bm-progress-line label">reading…</p>;
  const pct = Math.round((p.done / p.total) * 100);
  return (
    <div className="bm-progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
      <div className="bm-progress-bar" style={{ inlineSize: `${pct}%` }} />
      <span className="bm-progress-line">
        {p.done}/{p.total} · {p.message}
      </span>
    </div>
  );
}

function Report({ r }: { r: BookmarkImportReport }) {
  return (
    <div className="bm-report">
      <p>
        {r.dryRun ? "Would add" : "Added"} <b>{r.added}</b> of {r.found}
        {r.updated > 0 && <>, {r.updated} now also listed here</>}
        {r.unchanged > 0 && <>, {r.unchanged} already saved</>}
        {r.duplicates > 0 && <>, {r.duplicates} duplicates skipped</>}
        {r.removed > 0 && <>, {r.removed} gone from the browser (kept, marked)</>}.
      </p>
      {r.dryRun && r.sample.length > 0 && (
        <ul className="bm-sample">
          {r.sample.map((s) => (
            <li key={s.rel}>
              <span>{s.title}</span> <span className="bm-faint">→ {s.rel}</span>
            </li>
          ))}
        </ul>
      )}
      {r.errors.length > 0 && (
        <ul className="bm-errors is-error">
          {r.errors.slice(0, 5).map((e) => (
            <li key={e}>{e}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** One source's run: dry run first, then the real import, so nothing lands unseen. */
function useRun(onDone: () => void, say: Say) {
  const [busy, setBusy] = useState<string | null>(null);
  const [reports, setReports] = useState<Record<string, BookmarkImportReport>>({});
  const run = useCallback(
    async (id: string, dryRun: boolean) => {
      setBusy(id);
      try {
        const r = await bookmarksImport(id, dryRun);
        setReports((m) => ({ ...m, [id]: r }));
        if (!dryRun) {
          say(`Imported ${r.added} new bookmark${r.added === 1 ? "" : "s"}.`);
          onDone();
        }
      } catch (e) {
        say(errorMessage(e), "error");
      } finally {
        setBusy(null);
      }
    },
    [onDone, say],
  );
  return { busy, reports, run };
}

/**
 * The browser-permission panel: every detected browser profile with its own checkbox.
 * Nothing is read until it is ticked; Safari explains Full Disk Access or HTML export.
 */
export default function BookmarksSources({ onImported, say }: { onImported: () => void; say: Say }) {
  const [sources, setSources] = useState<BrowserSource[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [htmlPath, setHtmlPath] = useState("");
  const progress = useImportProgress();

  const load = useCallback(() => {
    browsersDetect()
      .then((s) => (setSources(s), setError(null)))
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);

  const done = useCallback(() => {
    load();
    onImported();
  }, [load, onImported]);
  const { busy, reports, run } = useRun(done, say);

  async function toggle(s: BrowserSource, allowed: boolean) {
    try {
      await browserAllow(s.id, allowed);
      load();
    } catch (e) {
      say(errorMessage(e), "error");
    }
  }

  if (error) return <p className="empty is-error">Could not look for browsers: {error}</p>;
  if (!sources) return <p className="empty">Looking for browsers…</p>;

  const html = htmlPath.trim() ? htmlSource(htmlPath.trim()) : "";

  return (
    <section className="bm-sources panel" aria-label="Browsers">
      <header className="panel-head">
        <span className="panel-kicker">browsers</span>
        <p className="bm-faint">
          Vault HQ reads each browser's own bookmark file, only for the profiles you tick. No plugin, no login.
        </p>
      </header>
      {sources.length === 0 && <p className="empty">No browsers with bookmarks found on this Mac.</p>}
      <ul className="bm-source-list">
        {sources.map((s) => (
          <li key={s.id} className={`bm-source${s.allowed ? " is-allowed" : ""}`}>
            <label className="bm-check">
              <input type="checkbox" checked={s.allowed} onChange={(e) => toggle(s, e.target.checked)} />
              <span className="bm-source-label">{s.label}</span>
              {s.allowed && s.count !== null && <span className="count">{s.count}</span>}
            </label>
            {s.allowed && !s.needsAccess && (
              <span className="bm-source-actions">
                <button className="button" disabled={busy !== null} onClick={() => run(s.id, true)}>
                  Preview
                </button>
                <button className="button is-primary" disabled={busy !== null} onClick={() => run(s.id, false)}>
                  Import
                </button>
              </span>
            )}
            {s.guidance && (s.needsAccess || (s.browser === "safari" && !s.allowed)) && (
              <p className={`bm-guidance${s.needsAccess ? " is-attention" : ""}`}>{s.guidance}</p>
            )}
            {s.guidance && s.allowed && !s.needsAccess && s.count === null && (
              <p className="bm-guidance is-error">{s.guidance}</p>
            )}
            {busy === s.id && <Progress p={progress} />}
            {reports[s.id] && busy !== s.id && <Report r={reports[s.id]!} />}
          </li>
        ))}
        <li className="bm-source">
          <label className="bm-html">
            <span className="bm-source-label">HTML export</span>
            <input
              type="text"
              placeholder="/path/to/bookmarks.html"
              value={htmlPath}
              onChange={(e) => setHtmlPath(e.target.value)}
              spellCheck={false}
            />
          </label>
          <span className="bm-source-actions">
            <button className="button" disabled={!html || busy !== null} onClick={() => run(html, true)}>
              Preview
            </button>
            <button className="button is-primary" disabled={!html || busy !== null} onClick={() => run(html, false)}>
              Import
            </button>
          </span>
          <p className="bm-faint">Any browser's "Export bookmarks" file works, Safari's included (File → Export → Bookmarks…).</p>
          {busy === html && html && <Progress p={progress} />}
          {html && reports[html] && busy !== html && <Report r={reports[html]!} />}
        </li>
      </ul>
    </section>
  );
}
