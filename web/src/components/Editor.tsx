import { useEffect, useMemo, useRef, useState } from "react";
import { api, HqError, type Source } from "../api.ts";
import { lineDiff } from "../lib/text.ts";

// Editing a whole note, properties included. The core refuses a save if the file changed
// since the editor read it (another agent, a sync, another editor); then the user decides.
// Unsaved text is kept in this window per note, so leaving the view never loses it.

const draftKey = (id: string) => `hq.draft.${id}`;

function loadDraft(id: string): { base: string; text: string } | null {
  try {
    const raw = localStorage.getItem(draftKey(id));
    return raw ? (JSON.parse(raw) as { base: string; text: string }) : null;
  } catch {
    return null;
  }
}

function storeDraft(id: string, base: string, text: string | null): void {
  try {
    if (text === null) localStorage.removeItem(draftKey(id));
    else localStorage.setItem(draftKey(id), JSON.stringify({ base, text }));
  } catch {
    /* storage refused: drafts last for this page only */
  }
}

type Saved = { ok: true; src: Source } | { ok: false; current: Source | null; error: string };

async function saveNote(id: string, text: string, base: string): Promise<Saved> {
  try {
    const r = await api.noteSave(id, text, base);
    return { ok: true, src: { id: r.id ?? id, text, hash: r.hash } };
  } catch (e) {
    if (e instanceof HqError && e.kind === "Conflict" && e.current) return { ok: false, current: { id, ...e.current }, error: e.message };
    return { ok: false, current: null, error: (e as Error).message };
  }
}

// Enter continues a list item or checkbox; Enter on an empty item ends the list.
const ITEM = /^(\s*)([-*+]|\d+[.)])(\s+)(\[[ xX\-/>]\]\s+)?/;

export default function Editor({ id, onClose, notify }: { id: string; onClose: (saved: boolean) => void; notify: (m: string, tone?: "ok" | "error") => void }) {
  const [src, setSrc] = useState<Source | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState<Source | null>(null);
  const [saved, setSaved] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const dirty = src !== null && text !== src.text;

  useEffect(() => {
    let live = true;
    api.noteSource(id).then(
      (s) => {
        if (!live) return;
        const d = loadDraft(id);
        setSrc(s);
        if (d && d.text !== s.text) {
          setText(d.text);
          if (d.base !== s.hash) {
            setConflict(s); // the note moved on since this draft was made
            notify("Restored your unsaved draft; the note has changed since", "error");
          } else notify("Restored your unsaved draft");
        } else setText(s.text);
        window.setTimeout(() => area.current?.focus(), 0);
      },
      (e: Error) => live && setError(e.message),
    );
    return () => {
      live = false;
    };
  }, [id, notify]);

  // keep the draft while it differs from the file; drop it once saved or reverted
  useEffect(() => {
    if (src) storeDraft(id, src.hash, text === src.text ? null : text);
  }, [id, src, text]);

  useEffect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);

  async function save(close: boolean, base = src?.hash) {
    if (!src || !base || busy) return;
    setBusy(true);
    try {
      const r = await saveNote(id, text, base);
      if (r.ok) {
        setSrc(r.src);
        setText(r.src.text);
        setConflict(null);
        setSaved(true);
        notify("Saved");
        if (close) onClose(true);
      } else if (r.current) {
        setConflict(r.current);
      } else notify(r.error, "error");
    } catch (e) {
      notify((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  }

  function close() {
    if (dirty && !window.confirm("Close the editor? Your unsaved text stays as a draft and comes back when you edit this note again.")) return;
    onClose(saved);
  }

  function takeTheirs() {
    if (!conflict) return;
    setSrc(conflict);
    setText(conflict.text);
    setConflict(null);
    notify("Loaded the version on disk; your edits were dropped");
  }

  async function copyMine() {
    try {
      await navigator.clipboard.writeText(text);
      notify("Your version is on the clipboard");
    } catch {
      notify("Could not copy to the clipboard", "error");
    }
  }

  // replace the selection [a, b) with `insert`, keeping undo working where the browser allows
  function splice(a: number, b: number, insert: string, caret = a + insert.length) {
    const el = area.current;
    if (!el) return;
    el.setSelectionRange(a, b);
    const done = insert ? document.execCommand("insertText", false, insert) : a === b || document.execCommand("delete");
    if (done) {
      // the DOM is already updated: place the caret now, never later, or fast typing lands in the wrong place
      if (caret !== a + insert.length) el.setSelectionRange(caret, caret);
    } else {
      setText(el.value.slice(0, a) + insert + el.value.slice(b));
      requestAnimationFrame(() => el.setSelectionRange(caret, caret));
    }
  }

  function onKey(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    const el = e.currentTarget;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save(false);
    } else if (mod && e.key === "Enter") {
      e.preventDefault();
      void save(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") {
      e.preventDefault();
      const { selectionStart: a, selectionEnd: b, value } = el;
      const lineStart = value.lastIndexOf("\n", a - 1) + 1;
      if (e.shiftKey) {
        const n = value.startsWith("  ", lineStart) ? 2 : value.startsWith(" ", lineStart) || value.startsWith("\t", lineStart) ? 1 : 0;
        if (n) splice(lineStart, lineStart + n, "", Math.max(lineStart, a - n));
      } else if (a !== b || ITEM.test(value.slice(lineStart, a))) {
        splice(lineStart, lineStart, "  ", a + 2); // indent the list item, not just the caret
      } else splice(a, b, "  ");
    } else if (e.key === "Enter" && !e.shiftKey && !mod && el.selectionStart === el.selectionEnd) {
      const { selectionStart: a, value } = el;
      const lineStart = value.lastIndexOf("\n", a - 1) + 1;
      const line = value.slice(lineStart, a);
      const m = ITEM.exec(line);
      if (!m) return;
      e.preventDefault();
      if (line.length === m[0].length) {
        splice(lineStart, a, ""); // empty item: end the list
      } else {
        const num = /^\d+/.exec(m[2] ?? "");
        const marker = num ? `${Number(num[0]) + 1}${(m[2] ?? ".").slice(-1)}` : m[2];
        splice(a, a, `\n${m[1]}${marker}${m[3]}${m[4] ? "[ ] " : ""}`);
      }
    }
  }

  const diff = useMemo(() => (conflict && src ? lineDiff(src.text, conflict.text).filter((l) => l.kind !== " ") : []), [conflict, src]);

  if (error) return <p className="error">{error}</p>;
  if (!src) return <p className="empty">Opening the editor…</p>;
  return (
    <div className="editor">
      <div className="editor-bar">
        <span className={`editor-state ${dirty ? "is-dirty" : ""}`}>{busy ? "saving…" : dirty ? "● unsaved" : "saved"}</span>
        <button type="button" className="button" onClick={() => void save(false)} disabled={!dirty || busy} title="Save (⌘S)">
          save
        </button>
        <button type="button" className="button" onClick={() => void save(true)} disabled={busy} title="Save and close (⌘⏎)">
          save &amp; close
        </button>
        <button type="button" className="button" onClick={close} title="Close (Esc)">
          close
        </button>
      </div>

      {conflict && (
        <div className="editor-conflict" role="alert">
          <p>
            <b>This note changed on disk while you were editing.</b> Something else saved it (an agent, a sync, or another editor). Nothing was
            overwritten. What changed on disk since you opened it:
          </p>
          <pre className="diff">
            {diff.length ? diff.slice(0, 60).map((l, i) => <span key={i} className={l.kind === "+" ? "is-add" : "is-del"}>{`${l.kind} ${l.text}\n`}</span>) : "(only whitespace or line endings)"}
            {diff.length > 60 ? `… and ${diff.length - 60} more lines\n` : ""}
          </pre>
          <div className="editor-actions">
            <button type="button" className="button" onClick={takeTheirs}>
              use the disk version (drop mine)
            </button>
            <button type="button" className="button" onClick={() => void copyMine()}>
              copy mine to clipboard
            </button>
            <button type="button" className="button is-danger" onClick={() => void save(false, conflict.hash)}>
              overwrite with mine
            </button>
          </div>
        </div>
      )}

      <label className="sr-only" htmlFor="note-editor">
        Note text
      </label>
      <textarea id="note-editor" ref={area} className="editor-text" autoFocus value={text} onChange={(e) => setText(e.target.value)} onKeyDown={onKey} spellCheck autoComplete="off" autoCapitalize="off" autoCorrect="off" />
    </div>
  );
}
