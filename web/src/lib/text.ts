// Pure text helpers the views share. No IPC here, so they are tested without the app.

import type { CaptureKind } from "../api.ts";

/** A single http(s) URL and nothing else: the capture box saves it as a bookmark. */
export function isLoneUrl(text: string): boolean {
  const t = text.trim();
  if (!/^https?:\/\/\S+$/i.test(t)) return false;
  try {
    return Boolean(new URL(t).host);
  } catch {
    return false;
  }
}

/** Enter captures a note, ⌘Enter a task; a lone URL is a bookmark either way. */
export function captureKind(text: string, mod: boolean): CaptureKind {
  if (isLoneUrl(text)) return "bookmark";
  return mod ? "task" : "note";
}

/**
 * Add `- line` at the end of the `## heading` section of a markdown note (before the
 * next heading of the same or higher level, after the section's last non-blank line).
 * Returns null when the note has no such heading.
 */
export function appendUnderHeading(text: string, heading: string, line: string): string | null {
  const lines = text.split("\n");
  const at = lines.findIndex((l) => /^##\s+/.test(l) && l.replace(/^##\s+/, "").trim().toLowerCase() === heading.trim().toLowerCase());
  if (at < 0) return null;
  let end = lines.length;
  for (let i = at + 1; i < lines.length; i++) {
    if (/^#{1,2}\s/.test(lines[i]!)) {
      end = i;
      break;
    }
  }
  let insert = end;
  while (insert > at + 1 && lines[insert - 1]!.trim() === "") insert--;
  const item = `- ${line.trim()}`;
  lines.splice(insert, 0, item);
  return lines.join("\n");
}

export type DiffLine = { kind: " " | "-" | "+"; text: string };

/** Lines only in `a` (removed) and only in `b` (added), by longest common subsequence. */
export function lineDiff(a: string, b: string): DiffLine[] {
  const x = a.split("\n");
  const y = b.split("\n");
  if (x.length * y.length > 4_000_000) return [{ kind: " ", text: "(too large to compare line by line)" }];
  const dp = Array.from({ length: x.length + 1 }, () => new Uint32Array(y.length + 1));
  for (let i = x.length - 1; i >= 0; i--) for (let j = y.length - 1; j >= 0; j--) dp[i]![j] = x[i] === y[j] ? dp[i + 1]![j + 1]! + 1 : Math.max(dp[i + 1]![j]!, dp[i]![j + 1]!);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < x.length || j < y.length) {
    if (i < x.length && j < y.length && x[i] === y[j]) {
      out.push({ kind: " ", text: x[i++]! });
      j++;
    } else if (j < y.length && (i === x.length || dp[i]![j + 1]! >= dp[i + 1]![j]!)) out.push({ kind: "+", text: y[j++]! });
    else out.push({ kind: "-", text: x[i++]! });
  }
  return out;
}

/** "_tasks_pending" → "tasks pending": daily-note metric keys as labels. */
export function metricLabel(key: string): string {
  return key.replace(/^_+/, "").replace(/_/g, " ");
}
