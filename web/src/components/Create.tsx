import { useEffect, useRef, useState } from "react";
import type { CreateRequest, Notify } from "../App.tsx";
import { api, noteUrl, notBuilt, useCall, type AppStatus, type Priority } from "../api.ts";
import { isLoneUrl } from "../lib/text.ts";

export type CreateKind = "note" | "task" | "bookmark";

const KINDS: { kind: CreateKind; label: string }[] = [
  { kind: "note", label: "note" },
  { kind: "task", label: "task" },
  { kind: "bookmark", label: "bookmark" },
];

const PRIORITIES: { value: Priority | ""; label: string }[] = [
  { value: "", label: "no priority" },
  { value: "highest", label: "🔺 highest" },
  { value: "high", label: "⏫ high" },
  { value: "medium", label: "🔼 medium" },
  { value: "low", label: "🔽 low" },
  { value: "lowest", label: "⏬ lowest" },
];

// The "+" menu and the palette's "new …" land here: a note from a template, a task, or a
// bookmark from a pasted link. Each goes to the primary vault unless another is picked.
export default function Create({ request, status, onClose, notify, go, onSaved }: { request: CreateRequest; status: AppStatus | null; onClose: () => void; notify: Notify; go: (hash: string) => void; onSaved: () => void }) {
  const [kind, setKind] = useState<CreateKind>(request.kind);
  const [text, setText] = useState(request.text);
  const [extra, setExtra] = useState(""); // template id, due date or bookmark note
  const [priority, setPriority] = useState<Priority | "">("");
  const [vault, setVault] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const first = useRef<HTMLInputElement>(null);
  const templates = useCall(api.templates, "templates", { live: false });
  const vaults = status?.vaults ?? [];

  useEffect(() => {
    setExtra("");
    setError(null);
    first.current?.focus();
  }, [kind]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const ready = kind === "bookmark" ? isLoneUrl(text) : text.trim().length > 0;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    const v = vault || undefined;
    try {
      if (kind === "note") {
        const n = await api.noteCreate({ title: text.trim(), template: extra || undefined, vault: v });
        notify(`Created: ${n.title}`);
        go(noteUrl(n.id));
      } else if (kind === "task") {
        await api.taskAdd({ description: text.trim(), due: extra || undefined, priority: priority || undefined, vault: v });
        notify(`Task added: ${text.trim()}`);
      } else {
        await api.bookmarkAdd(text.trim(), extra.trim() || undefined);
        notify("Bookmark saved; its title arrives in a moment");
      }
      onSaved();
      onClose();
    } catch (err) {
      setError(notBuilt(err) ? `Creating a ${kind} is not in this build yet.` : (err as Error).message);
    } finally {
      setBusy(false);
    }
  }

  const placeholder = kind === "note" ? "title of the new note" : kind === "task" ? "what needs doing" : "https://…";
  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <form className="palette create" role="dialog" aria-modal="true" aria-label={`New ${kind}`} onMouseDown={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="segmented create-kinds" role="tablist" aria-label="What to create">
          {KINDS.map((k) => (
            <button key={k.kind} type="button" role="tab" aria-selected={kind === k.kind} className={kind === k.kind ? "is-active" : ""} onClick={() => setKind(k.kind)}>
              {k.label}
            </button>
          ))}
        </div>
        <div className="prompt">
          <span aria-hidden="true">+</span>
          <label className="sr-only" htmlFor="create-text">
            {placeholder}
          </label>
          <input id="create-text" ref={first} value={text} onChange={(e) => setText(e.target.value)} placeholder={placeholder} autoComplete="off" spellCheck={kind !== "bookmark"} />
        </div>
        <div className="create-fields">
          {kind === "note" && (
            <label className="set-field">
              <span>template</span>
              <select value={extra} onChange={(e) => setExtra(e.target.value)} disabled={!templates.data?.length}>
                <option value="">{templates.data?.length ? "blank note" : templates.error && notBuilt(templates.error) ? "templates not in this build" : "no templates in _templates/"}</option>
                {templates.data?.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          {kind === "task" && (
            <>
              <label className="set-field">
                <span>due</span>
                <input type="date" value={extra} onChange={(e) => setExtra(e.target.value)} />
              </label>
              <label className="set-field">
                <span>priority</span>
                <select value={priority} onChange={(e) => setPriority(e.target.value as Priority | "")}>
                  {PRIORITIES.map((p) => (
                    <option key={p.value} value={p.value}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </label>
            </>
          )}
          {kind === "bookmark" && (
            <label className="set-field">
              <span>note</span>
              <input value={extra} onChange={(e) => setExtra(e.target.value)} placeholder="why you saved it (optional)" />
            </label>
          )}
          {kind !== "bookmark" && vaults.length > 1 && (
            <label className="set-field">
              <span>vault</span>
              <select value={vault} onChange={(e) => setVault(e.target.value)}>
                <option value="">primary</option>
                {vaults.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.name}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
        {error && <p className="create-error is-error">{error}</p>}
        <p className="palette-foot">
          <button type="submit" className="button is-primary" disabled={!ready || busy}>
            {busy ? "saving…" : `create ${kind}`}
          </button>{" "}
          <kbd>⏎</kbd> create <kbd>Esc</kbd> close
          {kind === "task" && " · goes to 09-Planning/Tasks.md"}
          {kind === "bookmark" && " · into 10-Bookmarks/"}
        </p>
      </form>
    </div>
  );
}
