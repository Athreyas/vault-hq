import type { Status } from "../../../server/types.ts";
import type { Route, View } from "../App.tsx";
import { VIEWS } from "../App.tsx";

const LABEL: Record<View, string> = { home: "home", needs: "needs", days: "days", tasks: "tasks", inbox: "inbox", notes: "notes", ask: "ask" };

// Tooltips are built from what is actually in the vault, not fixed text.
function tip(view: View, s: Status | null): string {
  if (!s) return LABEL[view];
  const t = s.tips;
  switch (view) {
    case "home":
      return `Overview of ${s.vault}`;
    case "needs":
      return t.needs.length ? `${s.needs} waiting. Top: ${t.needs.join("; ")}` : "Nothing is waiting on you";
    case "days":
      return `${t.today} agent actions and finished tasks today`;
    case "tasks":
      return `${t.tasksOpen} open, ${t.tasksOverdue} overdue`;
    case "inbox":
      return `${t.inbox} captures to curate`;
    case "notes":
      return `Read any of ${t.notes} notes`;
    case "ask":
      return `Search and ask across ${t.notes} notes`;
  }
}

function badge(view: View, s: Status | null): number {
  if (!s) return 0;
  return view === "needs" ? s.needs : view === "inbox" ? s.tips.inbox : view === "tasks" ? s.tips.tasksOverdue : 0;
}

function ago(isoStamp: string | null): string {
  if (!isoStamp) return "never";
  const min = Math.round((Date.now() - Date.parse(isoStamp)) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  return `${Math.round(min / 60)} h ago`;
}

export default function TopBar({ now, route, status }: { now: Date; route: Route; status: Status | null }) {
  return (
    <header className="bar">
      <a className="brand" href="#/home" title={status ? `Vault HQ serving ${status.vault}` : "Vault HQ"}>
        <span aria-hidden="true">▌</span>hq<span className="brand-vault">{status ? `:${status.vault}` : ""}</span>
      </a>
      <nav className="tabs" aria-label="Sections">
        {VIEWS.map((v, i) => {
          const n = badge(v, status);
          return (
            <a key={v} href={`#/${v}`} className={`tab ${route.view === v ? "is-active" : ""}`} aria-current={route.view === v ? "page" : undefined} title={tip(v, status)}>
              <kbd>{i + 1}</kbd>
              {LABEL[v]}
              {n > 0 && <b className={`tab-count ${v === "needs" || v === "tasks" ? "is-attention" : ""}`}>{n}</b>}
            </a>
          );
        })}
      </nav>
      <div className="bar-right">
        {status && status.unreadable.length > 0 && (
          <span className="sync is-attention" title={`HQ could not read these notes, so they are missing from every view:\n${status.unreadable.join("\n")}\n\nIn Finder, right-click the vault folder and choose Keep Downloaded.`}>
            {status.unreadable.length} notes unreadable
          </span>
        )}
        <span className={`sync ${status?.errors24h ? "is-attention" : ""}`} title="Background sync and metrics on this Mac">
          {status?.errors24h ? `${status.errors24h} sync errors` : `synced ${ago(status?.lastTick ?? null)}`}
        </span>
        <time className="clock" title={now.toLocaleDateString("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" })}>
          {now.toLocaleDateString("en-US", { month: "short", day: "numeric" })} {now.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false })}
        </time>
      </div>
    </header>
  );
}
