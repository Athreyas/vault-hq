// Small pieces every onboarding step uses: a copy button, a diff view, a status line.

import { useState } from "react";
import { copyText } from "../../api/onboarding.ts";

export type DiffKind = "meta" | "hunk" | "add" | "del" | "ctx";
export interface DiffLine {
  kind: DiffKind;
  text: string;
}

/** A unified diff, line by line, classified for colouring. */
export function parseDiff(diff: string): DiffLine[] {
  if (!diff) return [];
  const lines = diff.replace(/\n$/, "").split("\n");
  return lines.map((text): DiffLine => {
    if (text.startsWith("+++") || text.startsWith("---")) return { kind: "meta", text };
    if (text.startsWith("@@")) return { kind: "hunk", text };
    if (text.startsWith("+")) return { kind: "add", text };
    if (text.startsWith("-")) return { kind: "del", text };
    return { kind: "ctx", text };
  });
}

export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  return {
    added: lines.filter((l) => l.kind === "add").length,
    removed: lines.filter((l) => l.kind === "del").length,
  };
}

export function Diff({ diff }: { diff: string }) {
  const lines = parseDiff(diff);
  if (!lines.length) return <p className="ob-muted">No change: already set up like this.</p>;
  const { added, removed } = diffStats(lines);
  return (
    <div className="ob-diff" role="region" aria-label={`Change: ${added} lines added, ${removed} removed`}>
      <pre>
        {lines.map((l, i) => (
          <span key={i} className={`ob-diff-${l.kind}`}>
            {l.text}
            {"\n"}
          </span>
        ))}
      </pre>
    </div>
  );
}

export function CopyButton({ text, label = "copy" }: { text: string; label?: string }) {
  const [state, setState] = useState<"idle" | "ok" | "fail">("idle");
  return (
    <button
      type="button"
      className="ob-button"
      onClick={() =>
        void copyText(text).then((ok) => {
          setState(ok ? "ok" : "fail");
          setTimeout(() => setState("idle"), 1600);
        })
      }
    >
      {state === "ok" ? "copied ✓" : state === "fail" ? "copy failed" : label}
    </button>
  );
}

/** A block of text to paste somewhere else, with its copy button. */
export function Copyable({ text, title }: { text: string; title?: string }) {
  return (
    <div className="ob-copyable">
      <div className="ob-copyable-head">
        <span className="ob-label">{title ?? ""}</span>
        <CopyButton text={text} />
      </div>
      <pre tabIndex={0}>{text}</pre>
    </div>
  );
}

export function Status({ error, ok }: { error?: string | null; ok?: string | null }) {
  if (error) {
    return (
      <p className="ob-status is-error" role="alert">
        {error}
      </p>
    );
  }
  if (ok) {
    return (
      <p className="ob-status is-ok" role="status">
        {ok}
      </p>
    );
  }
  return null;
}

/** "Not in this build yet": another stream's command is missing. The step can be skipped. */
export function NotBuilt({ what }: { what: string }) {
  return (
    <p className="ob-status" role="status">
      {what} isn't in this build of Vault HQ yet. Skip this step; you can come back to it from Settings.
    </p>
  );
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}
