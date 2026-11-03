import { useEffect, useRef, useState } from "react";
import { addDays, api, cardHref, iso, parseDate, useCall } from "../api.ts";
import Memories from "./BookmarksMemories.tsx";
import Problem from "./Problem.tsx";
import { RichLine } from "./Rich.tsx";

// Every panel at once instead of one card at a time. Arrow keys move between panels,
// Enter follows the panel's action. The Ask bar widens while it has focus.
export default function Home({ date, onAsk }: { date: string; onAsk: (q: string, mode?: "ask" | "search") => void }) {
  const today = iso(new Date());
  const { data, error } = useCall(() => api.home(date), date, { refreshMs: 120_000 });
  const [q, setQ] = useState("");
  const grid = useRef<HTMLDivElement>(null);
  const to = (d: string) => (window.location.hash = d === today ? "#/home" : `#/home/${d}`);
  const suggestions = data?.suggestions ?? [];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const keys = ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"];
      if (!keys.includes(e.key) || e.ctrlKey || e.metaKey || e.altKey || !grid.current) return;
      if ((e.target as HTMLElement).closest("input, textarea, select, [role=dialog]")) return;
      const panels = [...grid.current.querySelectorAll<HTMLElement>(".panel")];
      if (!panels.length) return;
      e.preventDefault();
      const at = panels.indexOf(document.activeElement as HTMLElement);
      const cols = panels.filter((p) => p.offsetTop === panels[0]!.offsetTop).length || 1;
      const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : e.key === "ArrowDown" ? cols : -cols;
      panels[at < 0 ? 0 : Math.max(0, Math.min(panels.length - 1, at + step))]?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <section className="home">
      <form
        className="prompt is-big home-ask"
        onSubmit={(e) => {
          e.preventDefault();
          if (q.trim().length >= 3) onAsk(q.trim(), "ask");
        }}
      >
        <span aria-hidden="true">❯</span>
        <label className="sr-only" htmlFor="home-q">
          Ask the vault
        </label>
        <input
          id="home-q"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setQ("");
              e.currentTarget.blur();
            } else if (e.key === "Enter" && e.shiftKey && q.trim().length >= 3) {
              e.preventDefault();
              onAsk(q.trim(), "search");
            }
          }}
          placeholder="ask the vault, or ⌘K for everything else"
          autoComplete="off"
          spellCheck={false}
        />
        <button className="button is-primary" disabled={q.trim().length < 3}>
          ask
        </button>
      </form>
      {suggestions.length > 0 && (
        <ul className="suggest" aria-label="Suggested questions">
          {suggestions.map((s) => (
            <li key={s}>
              <button className="link-button" onClick={() => onAsk(s, "ask")} title="Ask this now">
                ▸ {s}
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="home-date">
        <span className="home-greeting">{data ? data.greeting : "…"}</span>
        <div className="date-pick" role="group" aria-label="Choose a day">
          <button className="button" onClick={() => to(addDays(date, -1))} aria-label="Previous day" title={addDays(date, -1)}>
            ‹
          </button>
          <label className="sr-only" htmlFor="home-date">
            Day
          </label>
          <input id="home-date" type="date" value={date} max={addDays(today, 30)} onChange={(e) => e.target.value && to(e.target.value)} />
          <button className="button" onClick={() => to(addDays(date, 1))} aria-label="Next day" title={addDays(date, 1)}>
            ›
          </button>
          {date !== today && (
            <button className="button" onClick={() => to(today)}>
              today
            </button>
          )}
        </div>
        <span className="home-weekday">{parseDate(date).toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric" })}</span>
      </div>

      {error && <Problem error={error} what="the home view" />}
      {!data && !error && <p className="empty">Reading the vault…</p>}
      <div className="panels" ref={grid}>
        {data?.cards.map((c) => (
          <article
            key={c.id}
            className={`panel kind-${c.kind}`}
            tabIndex={0}
            aria-label={`${c.title}. ${c.kicker}`}
            onKeyDown={(e) => {
              if (e.key === "Enter" && c.action && e.target === e.currentTarget) window.location.href = cardHref(c.action);
            }}
          >
            <header className="panel-head">
              <h2 className="panel-title" title={c.title}>
                {c.title}
              </h2>
              <span className="panel-kicker" title={c.kicker}>
                {c.kicker}
              </span>
            </header>
            {c.lines.length > 0 && (
              <ul className="panel-lines">
                {c.lines.map((l, j) => (
                  <li key={j} title={l.replace(/\[\[|\]\]/g, "")}>
                    <RichLine text={l} />
                  </li>
                ))}
              </ul>
            )}
            {c.progress !== null && (
              <div className="meter" role="meter" aria-valuenow={c.progress} aria-valuemin={0} aria-valuemax={100} aria-label="Progress">
                <span style={{ width: `${c.progress}%` }} />
                <b>{c.progress}%</b>
              </div>
            )}
            {c.action && (
              <a className="panel-action" href={cardHref(c.action)}>
                ⏎ {c.action.label.toLowerCase()}
              </a>
            )}
          </article>
        ))}
        {date === today && <Memories />}
      </div>
    </section>
  );
}
