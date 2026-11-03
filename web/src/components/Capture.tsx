import { useEffect, useRef, useState } from "react";
import { api, applyTheme, notBuilt, on, type CaptureKind } from "../api.ts";
import { actionAdd, actionDraft, errorText, type ActionAdded, type ActionDraft } from "../api/search.ts";
import { captureKind, isLoneUrl } from "../lib/text.ts";
import AskAction from "./AskAction.tsx";

const DONE: Record<CaptureKind, string> = { note: "captured to today's inbox", task: "task added", bookmark: "bookmark saved" };

// The menu-bar / ⌥⌘Space window: Enter captures a note, ⌘Enter a task, a lone link is
// saved as a bookmark. Esc (or clicking elsewhere) puts it away. ⌘Enter reads the task
// first ("call the bank Friday 9am" → day, time, project note) and shows it to confirm.
export default function Capture() {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ text: string; error: boolean } | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const [action, setAction] = useState<ActionDraft | null>(null);
  const [added, setAdded] = useState<ActionAdded | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    const ready = () => {
      setMsg(null);
      setAction(null);
      setAdded(null);
      api.settings().then(
        (s) => applyTheme(s.theme),
        () => {},
      );
      window.setTimeout(() => box.current?.focus(), 0);
    };
    ready();
    return on("capture://open", ready);
  }, []);

  async function send(mod: boolean) {
    const t = text.trim();
    if (!t || busy) return;
    const kind = captureKind(t, mod);
    setBusy(true);
    if (kind === "task") {
      try {
        const d = await actionDraft(t, true);
        if (d.isAction) {
          setActionError(null);
          setAction(d);
          return;
        }
      } catch {
        // no drafting in this build or no vault: capture the task as written
      } finally {
        setBusy(false);
      }
      setBusy(true);
    }
    try {
      await api.capture(t, kind);
      setText("");
      setMsg({ text: DONE[kind], error: false });
      window.setTimeout(() => void api.hideCapture().catch(() => {}), 450);
    } catch (e) {
      setMsg({ text: notBuilt(e) ? "Capture is not in this build yet." : (e as Error).message, error: true });
    } finally {
      setBusy(false);
    }
  }

  function onKey(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      void api.hideCapture().catch(() => {});
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send(e.metaKey || e.ctrlKey);
    }
  }

  async function addAction() {
    if (!action) return;
    setBusy(true);
    try {
      setAdded(await actionAdd(action));
      setText("");
      window.setTimeout(() => {
        setAction(null);
        setAdded(null);
        void api.hideCapture().catch(() => {});
      }, 1600);
    } catch (e) {
      setActionError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  if (action) {
    return (
      <main className="capture">
        <AskAction
          compact
          draft={action}
          added={added}
          busy={busy}
          error={actionError}
          onChange={setAction}
          onAdd={() => void addAction()}
          onCancel={() => {
            setAction(null);
            window.setTimeout(() => box.current?.focus(), 0);
          }}
        />
      </main>
    );
  }

  const kind = text.trim() ? captureKind(text, false) : null;
  return (
    <main className="capture">
      <div className="prompt is-big">
        <span aria-hidden="true">❯</span>
        <label className="sr-only" htmlFor="capture-box">
          Capture a thought, a task or a link
        </label>
        <textarea
          id="capture-box"
          ref={box}
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          placeholder="a thought, a task, a link"
          disabled={busy}
          autoComplete="off"
          spellCheck
        />
      </div>
      <p className={`capture-foot ${msg?.error ? "is-error" : ""}`} role="status" aria-live="polite">
        {msg ? (
          msg.text
        ) : (
          <>
            <kbd>⏎</kbd> {kind === "bookmark" || isLoneUrl(text) ? "bookmark" : "note"} <kbd>⌘⏎</kbd> {kind === "bookmark" ? "bookmark" : "task"} <kbd>⇧⏎</kbd> new line <kbd>Esc</kbd> close
          </>
        )}
      </p>
    </main>
  );
}
