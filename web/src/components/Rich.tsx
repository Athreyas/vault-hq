import { Fragment, type ReactNode } from "react";
import { noteUrl } from "../api.ts";

// Vault markdown rendered as React elements. Never as HTML: note text can't inject markup.
// [[note]] links open in HQ's own reader; Obsidian resolves a bare stem the same way.

const INLINE = /(!?\[\[[^\]]+\]\]|`[^`]+`|\[[^\]]+\]\([^)\s]+\)|\*\*[^*]+\*\*|__[^_]+__|(?<![\w*])\*[^*\s][^*]*\*(?![\w*])|(?<![\w_])_[^_\s][^_]*_(?![\w_])|~~[^~]+~~|https?:\/\/[^\s)>\]]+)/g;

export function RichLine({ text }: { text: string }) {
  const parts = text.split(INLINE);
  return (
    <>
      {parts.map((p, i) => {
        if (!p) return null;
        const wiki = /^!?\[\[([^\]|#]+)(?:#([^\]|]*))?(?:\|([^\]]+))?\]\]$/.exec(p);
        if (wiki?.[1]) {
          return (
            <a key={i} className="note-link" href={noteUrl(wiki[1])}>
              {wiki[3] ?? (wiki[2] ? `${wiki[1]} › ${wiki[2]}` : wiki[1])}
            </a>
          );
        }
        const md = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(p);
        if (md?.[1] && md[2]) return <Link key={i} href={md[2]} label={md[1]} />;
        if (/^https?:\/\//.test(p)) return <Link key={i} href={p} label={p} />;
        if (p.length > 1 && p.startsWith("`") && p.endsWith("`")) return <code key={i}>{p.slice(1, -1)}</code>;
        if (/^(\*\*|__).+\1$/.test(p)) return <strong key={i}>{p.slice(2, -2)}</strong>;
        if (/^~~.+~~$/.test(p)) return <s key={i}>{p.slice(2, -2)}</s>;
        if (/^([*_]).+\1$/.test(p)) return <em key={i}>{p.slice(1, -1)}</em>;
        return <Fragment key={i}>{p}</Fragment>;
      })}
    </>
  );
}

function Link({ href, label }: { href: string; label: string }) {
  if (/^https?:\/\//.test(href)) {
    return (
      <a className="note-link is-external" href={href} target="_blank" rel="noopener noreferrer">
        {label}
      </a>
    );
  }
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return <>{label}</>; // other schemes (javascript:, file:) stay text
  let target = href.split("#")[0] ?? href;
  try {
    target = decodeURIComponent(target);
  } catch {
    /* keep it as written */
  }
  return (
    <a className="note-link" href={noteUrl(target)}>
      {label}
    </a>
  );
}

/** Small markdown for card and answer text: paragraphs and bullet lists. */
export function RichText({ text }: { text: string }) {
  const blocks = text.trim().split(/\n{2,}/);
  return (
    <>
      {blocks.map((b, i) => {
        const lines = b.split("\n");
        if (lines.every((l) => /^\s*[-*] /.test(l))) {
          return (
            <ul key={i}>
              {lines.map((l, j) => (
                <li key={j}>
                  <RichLine text={l.replace(/^\s*[-*] /, "")} />
                </li>
              ))}
            </ul>
          );
        }
        return (
          <p key={i}>
            <RichLine text={b.replace(/^#+ /, "")} />
          </p>
        );
      })}
    </>
  );
}

const LIST = /^(\s*)([-*+]|\d+[.)])\s+(?:\[([ xX\-/>])\]\s+)?(.*)$/;
const indent = (ws: string) => Math.floor(ws.replace(/\t/g, "  ").length / 2);
const BLOCK_START = /^(#{1,6}\s|\s*```|\s*~~~|\s*>|\s*\|)/;

/** Whole-note markdown: headings, lists with nesting and checkboxes, quotes and
 *  callouts, fenced code, tables and rules. Unknown syntax falls back to text. */
export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const out: ReactNode[] = [];
  let i = 0;
  const at = (n: number) => lines[n] ?? "";

  while (i < lines.length) {
    const line = at(i);
    const k = out.length;
    if (!line.trim()) {
      i++;
      continue;
    }

    const fence = /^\s*(```|~~~)\s*([\w+-]*)/.exec(line);
    if (fence) {
      const mark = fence[1] ?? "```";
      const body: string[] = [];
      i++;
      while (i < lines.length && !at(i).trim().startsWith(mark)) body.push(at(i++));
      i++;
      out.push(
        <pre key={k} className="md-code" data-lang={fence[2] || undefined}>
          <code>{body.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      const level = Math.min((h[1] ?? "#").length + 1, 6); // the note title is the h1
      const Tag = `h${level}` as "h2";
      out.push(
        <Tag key={k} className="md-h">
          <RichLine text={(h[2] ?? "").replace(/\s+#+\s*$/, "")} />
        </Tag>,
      );
      i++;
      continue;
    }

    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      out.push(<hr key={k} />);
      i++;
      continue;
    }

    if (/^\s*>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s*>/.test(at(i))) body.push(at(i++).replace(/^\s*> ?/, ""));
      const callout = /^\[!(\w+)\][+-]?\s*(.*)$/.exec(body[0] ?? "");
      out.push(
        <blockquote key={k} className={callout ? `md-callout is-${(callout[1] ?? "note").toLowerCase()}` : "md-quote"}>
          {callout && <p className="md-callout-title">{callout[2] || callout[1]}</p>}
          <Markdown text={(callout ? body.slice(1) : body).join("\n")} />
        </blockquote>,
      );
      continue;
    }

    if (/^\s*\|.*\|\s*$/.test(line) && /^\s*\|?\s*:?-{2,}/.test(at(i + 1))) {
      const cells = (l: string) => l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const head = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(at(i))) rows.push(cells(at(i++)));
      out.push(
        <div key={k} className="md-table">
          <table>
            <thead>
              <tr>
                {head.map((c, j) => (
                  <th key={j}>
                    <RichLine text={c} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, j) => (
                <tr key={j}>
                  {r.map((c, m) => (
                    <td key={m}>
                      <RichLine text={c} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    if (LIST.test(line)) {
      const items: { depth: number; ordered: boolean; check: string | null; text: string }[] = [];
      while (i < lines.length) {
        const m = LIST.exec(at(i));
        const last = items[items.length - 1];
        if (m) {
          items.push({ depth: indent(m[1] ?? ""), ordered: /\d/.test(m[2] ?? ""), check: m[3] ?? null, text: m[4] ?? "" });
          i++;
        } else if (last && /^\s{2,}\S/.test(at(i))) {
          last.text += ` ${at(i++).trim()}`; // a wrapped continuation line
        } else break;
      }
      const Tag = items[0]?.ordered ? "ol" : "ul";
      out.push(
        <Tag key={k} className="md-list">
          {items.map((it, j) => {
            const state = it.check === null ? "" : it.check === " " ? "is-open" : it.check.toLowerCase() === "x" ? "is-done" : "is-other";
            return (
              <li key={j} style={{ marginLeft: `${it.depth * 1.5}em` }} className={it.check !== null ? `md-task ${state}` : undefined}>
                {it.check !== null && (
                  <span className="md-box" aria-label={it.check === " " ? "open task" : "closed task"}>
                    {`[${it.check === " " ? " " : it.check.toLowerCase()}]`}
                  </span>
                )}
                <RichLine text={it.text} />
              </li>
            );
          })}
        </Tag>,
      );
      continue;
    }

    const para: string[] = [];
    while (i < lines.length && at(i).trim() && !BLOCK_START.test(at(i)) && !LIST.test(at(i))) para.push(at(i++));
    if (!para.length) para.push(at(i++)); // a line no rule claimed: show it as text
    out.push(
      <p key={k} className="md-p">
        <RichLine text={para.join(" ")} />
      </p>,
    );
  }
  return <>{out}</>;
}
