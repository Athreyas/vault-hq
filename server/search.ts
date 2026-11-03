// Ask, answered by local Ollama only: nothing leaves the machine.
// 1. Every note is split into heading-sized chunks.
// 2. Chunks are ranked by BM25 (exact words) blended with cosine similarity from an
//    Ollama embedding model (meaning). Embeddings are cached on disk by chunk hash, so
//    only new or changed chunks are embedded again.
// 3. The top chunks go to an Ollama chat model, which must answer from them and cite
//    [[note]] links. The sources are returned too, so the UI can show what was searched.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { config } from "./config.ts";
import { addDays } from "./dates.ts";
import { notes } from "./vault.ts";

export interface Source {
  rel: string;
  stem: string;
  heading: string;
  excerpt: string;
  score: number;
}

interface Chunk {
  id: string; // sha1 of rel + text: the embedding cache key
  rel: string;
  stem: string;
  heading: string;
  text: string;
  tokens: string[];
  dates: string[]; // created, updated and filename dates, for scoping by day
}

const STOP = new Set(
  "a an and are as at be by for from has have i in is it its of on or that the this to was were what when where which who why with how do does did my me we our you your not no can will about into than then there their them they".split(
    " ",
  ),
);

function tokenize(s: string): string[] {
  return s
    .toLowerCase()
    .replace(/[`*_>#\[\]()|]/g, " ")
    .split(/[^\p{L}\p{N}.-]+/u)
    .map((t) => t.replace(/^[.-]+|[.-]+$/g, ""))
    .filter((t) => t.length > 1 && !STOP.has(t));
}

// ---- chunking --------------------------------------------------------------------------

let index: { signature: string; chunks: Chunk[]; df: Map<string, number>; avgLen: number } | null = null;

function buildIndex(): NonNullable<typeof index> {
  const all = notes().filter((n) => !n.rel.startsWith("_") && !n.rel.endsWith("AGENTS.md"));
  const signature = all.map((n) => `${n.rel}:${n.mtime}`).join("|");
  if (index && index.signature === signature) return index;
  const chunks: Chunk[] = [];
  for (const n of all) {
    const title = n.props.title ?? n.stem;
    const dates = [n.props.created, n.props.updated, n.stem]
      .map((v) => /\d{4}-\d{2}-\d{2}/.exec(v ?? "")?.[0])
      .filter((v): v is string => Boolean(v));
    let heading = title;
    let buf: string[] = [];
    const flush = () => {
      const text = buf.join("\n").trim();
      if (text.length > 40) {
        // long sections are split so each chunk stays a focused, embeddable size
        for (let i = 0; i < text.length; i += 1400) {
          const part = text.slice(i, i + 1600);
          chunks.push({
            id: crypto.createHash("sha1").update(`${n.rel}\n${heading}\n${part}`).digest("hex"),
            rel: n.rel,
            stem: n.stem,
            heading,
            text: part,
            tokens: tokenize(`${title} ${heading} ${part}`),
            dates,
          });
        }
      }
      buf = [];
    };
    for (const line of n.body.split("\n")) {
      const h = /^#{1,3} (.+)$/.exec(line);
      if (h?.[1]) {
        flush();
        heading = h[1].trim() === title ? title : `${title} › ${h[1].trim()}`;
      } else buf.push(line);
    }
    flush();
  }
  const df = new Map<string, number>();
  for (const c of chunks) for (const t of new Set(c.tokens)) df.set(t, (df.get(t) ?? 0) + 1);
  const avgLen = chunks.reduce((s, c) => s + c.tokens.length, 0) / Math.max(chunks.length, 1);
  index = { signature, chunks, df, avgLen };
  return index;
}

function bm25(query: string[], idx: NonNullable<typeof index>): Map<string, number> {
  const N = idx.chunks.length;
  const scores = new Map<string, number>();
  for (const c of idx.chunks) {
    let s = 0;
    for (const q of query) {
      const tf = c.tokens.filter((t) => t === q).length;
      if (!tf) continue;
      const df = idx.df.get(q) ?? 0;
      const idf = Math.log(1 + (N - df + 0.5) / (df + 0.5));
      s += idf * ((tf * 2.2) / (tf + 1.2 * (0.25 + 0.75 * (c.tokens.length / idx.avgLen))));
    }
    if (s > 0) scores.set(c.id, s);
  }
  return scores;
}

// ---- Ollama ------------------------------------------------------------------------------

async function ollama<T>(endpoint: string, body: unknown, timeoutMs: number): Promise<T> {
  const r = await fetch(`${config.ollama.url}${endpoint}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!r.ok) throw new Error(`Ollama ${endpoint} returned ${r.status}: ${(await r.text()).slice(0, 200)}`);
  return (await r.json()) as T;
}

export async function ollamaStatus(): Promise<{ up: boolean; chat: boolean; embed: boolean; models: string[] }> {
  try {
    const r = await fetch(`${config.ollama.url}/api/tags`, { signal: AbortSignal.timeout(3000) });
    const models = ((await r.json()) as { models: { name: string }[] }).models.map((m) => m.name);
    const has = (m: string) => models.some((x) => x === m || x.startsWith(`${m}:`) || x.split(":")[0] === m.split(":")[0]);
    return { up: true, chat: has(config.ollama.chatModel), embed: has(config.ollama.embedModel), models };
  } catch {
    return { up: false, chat: false, embed: false, models: [] };
  }
}

const CACHE = path.join(config.state, "hq-embeddings.json");
let vectors: Map<string, number[]> | null = null;

/** Forget every embedding, on disk too: called when the embedding model changes. */
export function resetVectors(): void {
  vectors = new Map();
  try {
    fs.rmSync(CACHE, { force: true });
  } catch {
    /* nothing cached */
  }
}

export function vectorCount(): number {
  return loadVectors().size;
}

function loadVectors(): Map<string, number[]> {
  if (vectors) return vectors;
  try {
    vectors = new Map(Object.entries(JSON.parse(fs.readFileSync(CACHE, "utf8")) as Record<string, number[]>));
  } catch {
    vectors = new Map();
  }
  return vectors;
}

async function embed(texts: string[]): Promise<number[][]> {
  const r = await ollama<{ embeddings: number[][] }>("/api/embed", { model: config.ollama.embedModel, input: texts }, 120_000);
  return r.embeddings;
}

async function ensureVectors(chunks: Chunk[]): Promise<Map<string, number[]>> {
  const v = loadVectors();
  const missing = chunks.filter((c) => !v.has(c.id));
  for (let i = 0; i < missing.length; i += 32) {
    const batch = missing.slice(i, i + 32);
    const out = await embed(batch.map((c) => `${c.heading}\n${c.text}`));
    batch.forEach((c, j) => out[j] && v.set(c.id, out[j]!));
  }
  if (missing.length) {
    // keep only vectors for chunks that still exist, so the cache doesn't grow forever
    const live = new Set(chunks.map((c) => c.id));
    for (const id of v.keys()) if (!live.has(id)) v.delete(id);
    fs.mkdirSync(path.dirname(CACHE), { recursive: true });
    fs.writeFileSync(CACHE, JSON.stringify(Object.fromEntries(v)));
  }
  return v;
}

function cosine(a: number[], b: number[]): number {
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i]! * b[i]!;
    na += a[i]! ** 2;
    nb += b[i]! ** 2;
  }
  return dot / (Math.sqrt(na * nb) || 1);
}

// ---- search + answer --------------------------------------------------------------------

// Living notes describe the present; session logs and captures describe the past.
function weight(rel: string): number {
  if (rel.startsWith("01-Projects/")) return 1.2;
  if (rel.startsWith("05-References/")) return 1.1;
  if (rel.startsWith("06-Memory/sessions/") || rel.startsWith("00-Inbox/")) return 0.85;
  return 1;
}

export async function search(
  question: string,
  k = 8,
  opts?: { date?: string; span?: number }
): Promise<{ sources: Source[]; semantic: boolean }> {
  const idx = buildIndex();
  // a scope is a window of days ending on `date`, matching how the Days view counts
  const from = opts?.date && opts.span ? addDays(opts.date, -(opts.span - 1)) : null;
  const chunks = from && opts?.date ? idx.chunks.filter((c) => c.dates.some((d) => d >= from && d <= opts.date!)) : idx.chunks;

  const lexical = bm25(tokenize(question), { ...idx, chunks });
  const maxLex = Math.max(1e-9, ...lexical.values());
  let semantic = new Map<string, number>();
  const status = await ollamaStatus();
  if (status.embed) {
    const v = await ensureVectors(chunks);
    const [q] = await embed([question]);
    if (q) for (const c of chunks) semantic.set(c.id, cosine(q, v.get(c.id) ?? []));
  }
  const ranked = chunks
    .map((c) => ({ c, score: weight(c.rel) * (0.45 * ((lexical.get(c.id) ?? 0) / maxLex) + 0.55 * (semantic.get(c.id) ?? 0)) }))
    .filter((x) => x.score > 0.05)
    .sort((a, b) => b.score - a.score);
  // at most three chunks per note, so one long note can't crowd out the rest
  const perNote = new Map<string, number>();
  const picked = ranked.filter(({ c }) => {
    const n = perNote.get(c.rel) ?? 0;
    perNote.set(c.rel, n + 1);
    return n < 3;
  });
  return {
    semantic: semantic.size > 0,
    sources: picked.slice(0, k).map(({ c, score }) => ({
      rel: c.rel,
      stem: c.stem,
      heading: c.heading,
      excerpt: c.text
        .replace(/\s*🆔\s*[\w-]+/gu, "")
        .replace(/\s*(🔺|⏫|🔼|🔽|⏬)/gu, "")
        .replace(/- \[ \] /g, "○ ")
        .replace(/- \[[xX]\] /g, "✓ ")
        .replace(/\*\*/g, "")
        .replace(/\s+/g, " ")
        .slice(0, 280),
      score: Math.round(score * 100) / 100,
    })),
  };
}

export async function ask(
  question: string,
  opts?: { date?: string; span?: number }
): Promise<{ answer: string; sources: Source[]; model: string; semantic: boolean }> {
  const status = await ollamaStatus();
  if (!status.up) throw new Error(`Ollama is not reachable at ${config.ollama.url}. Start it with: ollama serve`);
  if (!status.chat) throw new Error(`Ollama has no ${config.ollama.chatModel}. Pull it with: ollama pull ${config.ollama.chatModel}`);
  const { sources, semantic } = await search(question, 8, opts);
  const idx = buildIndex();
  const context = sources
    .map((s, i) => {
      const full = idx.chunks.find((c) => c.rel === s.rel && c.heading === s.heading)?.text ?? s.excerpt;
      return `Excerpt ${i + 1}, from note [[${s.stem}]] (${s.rel.split("/")[0]}), section "${s.heading}":\n${full}`;
    })
    .join("\n\n---\n\n");
  const r = await ollama<{ message: { content: string } }>(
    "/api/chat",
    {
      model: config.ollama.chatModel,
      stream: false,
      options: { temperature: 0.1, num_ctx: 8192 },
      messages: [
        {
          role: "system",
          content:
            "You answer questions about the user's own notes. Use ONLY the excerpts provided. " +
            "Notes from 01-Projects describe the current state; prefer them over session logs, which describe the past. " +
            "In checklists, '- [ ]' is still to do and '- [x]' is done. " +
            `Today is ${new Date().toISOString().slice(0, 10)}. When excerpts disagree, the one with the most recent date wins; ` +
            "words like 'was', 'replaced' or 'before' mark the older fact. " +
            "Cite the note for every fact as [[note-stem]], copied from the excerpt header. " +
            "If the excerpts do not answer the question, say plainly that the notes don't say. " +
            "Answer directly: a short paragraph or a few bullets, no preamble.",
        },
        { role: "user", content: `Excerpts from the vault:\n\n${context || "(no matching notes)"}\n\nQuestion: ${question}` },
      ],
    },
    180_000,
  );
  // small models cite "Excerpt 3" despite instructions; turn those into note links
  const answer = r.message.content
    .trim()
    .replace(/\(?\b[Ee]xcerpts? (\d+)\)?/g, (m, n: string) => {
      const s = sources[Number(n) - 1];
      return s ? `[[${s.stem}]]` : m;
    });
  return { answer, sources, model: config.ollama.chatModel, semantic };
}
