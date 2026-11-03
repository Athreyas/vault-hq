import type { InboxItem } from "../../../server/types.ts";
import type { Notify } from "../App.tsx";
import { noteUrl, post, useData } from "../api.ts";

const KIND = { compact: "Compact summary", session: "Session summary", proposal: "Proposal", dump: "Your dump" } as const;

export default function Inbox({ notify, onChange }: { notify: Notify; onChange: () => void }) {
  const { data, error, reload } = useData<InboxItem[]>("/api/inbox");

  async function archive(item: InboxItem) {
    try {
      await post("/api/inbox/archive", { rel: item.rel });
      notify(`Archived: ${item.title}`);
      reload();
      onChange();
    } catch (e) {
      notify((e as Error).message, "error");
    }
  }

  if (error) return <p className="empty">Could not read the inbox: {error}</p>;
  if (!data) return <p className="empty">Reading the inbox…</p>;
  if (!data.length) {
    return (
      <div className="empty is-large">
        <p>The inbox is clear.</p>
        <p className="empty-hint">Session and compact summaries from Claude Code land here, along with anything you dump in 00-Inbox.</p>
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
          <li key={i.rel} className={`inbox-item kind-${i.kind}`}>
            <div>
              <a className="inbox-title" href={noteUrl(i.rel)}>
                {i.title}
              </a>
              <p>
                {KIND[i.kind]}, {i.ageDays === 0 ? "today" : `${i.ageDays} days old`}, by {i.author}
              </p>
            </div>
            <div className="inbox-actions">
              <a className="button" href={noteUrl(i.rel)}>
                Open
              </a>
              <button className="button" onClick={() => archive(i)}>
                Archive
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
