import { useEffect, useRef, useState } from "react";
import { api, applyTheme, notBuilt, on, type CaptureKind } from "../api.ts";
import { captureKind, isLoneUrl } from "../lib/text.ts";

const DONE: Record<CaptureKind, string> = { note: "captured to today's inbox", task: "task added", bookmark: "bookmark saved" };

// The menu-bar / ⌥⌘Space window: Enter captures a note, ⌘Enter a task, a lone link is
// saved as a bookmark. Esc (or clicking elsewhere) puts it away.
export default function Capture() {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ text: string; error: boolean } | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const ready = () => {
      setMsg(null);
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
