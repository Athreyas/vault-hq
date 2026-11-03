import type { Notify } from "../App.tsx";
import { api, noteUrl, useCall, type InboxItem } from "../api.ts";
import Problem from "./Problem.tsx";

const KIND: Record<string, string> = { capture: "Capture", compact: "Compact summary", session: "Session summary", proposal: "Proposal", dump: "Your dump" };

export default function Inbox({ notify, onChange }: { notify: Notify; onChange: () => void }) {
  const { data, error, reload } = useCall(api.inbox, "inbox");

  async function archive(item: InboxItem) {
    try {
      await api.inboxArchive(item.id);
      notify(`Archived: ${item.title}`);
      reload();
      onChange();
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  if (error) return <Problem error={error} what="the inbox" />;
  if (!data) return <p className="empty">Reading the inbox…</p>;
  if (!data.length) {
    return (
      <div className="empty is-large">
        <p>The inbox is clear.</p>
        <p className="empty-hint">Quick captures, agent session notes and proposals land here, along with anything you drop in 00-Inbox.</p>
      </div>
    );
  }
  return (
    <section className="inbox">
      <h1>
        Inbox <span className="count">{data.length}</span>
      </h1>
      <p className="lede">Curate what matters into a project or memory note, then archive the capture.</p>
      <ul className="inbox-list">
        {data.map((i) => (
          <li key={i.id} className={`inbox-item kind-${i.kind}`}>
            <div>
              <a className="inbox-title" href={noteUrl(i.id)}>
                {i.title}
              </a>
              <p>
                {KIND[i.kind] ?? i.kind}, {i.ageDays === 0 ? "today" : `${i.ageDays} days old`}, by {i.author}
              </p>
            </div>
            <div className="inbox-actions">
              <a className="button" href={noteUrl(i.id)}>
                Open
              </a>
              <button className="button" onClick={() => void archive(i)}>
                Archive
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
