// Bookmarks over Tauri commands (SPEC 8, 10). Argument names are camelCase here; Tauri maps
// them to the Rust commands' snake_case parameters.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Bookmark } from "../bindings/Bookmark";
import type { BookmarkFilter } from "../bindings/BookmarkFilter";
import type { BookmarkImportReport } from "../bindings/BookmarkImportReport";
import type { BookmarkPage } from "../bindings/BookmarkPage";
import type { BrowserSource } from "../bindings/BrowserSource";
import type { EnrichReport } from "../bindings/EnrichReport";
import type { HqError } from "../bindings/HqError";
import type { Memory } from "../bindings/Memory";
import type { Platform } from "../bindings/Platform";

export type { Bookmark, BookmarkFilter, BookmarkImportReport, BookmarkPage, BrowserSource, EnrichReport, Memory, Platform };

export const emptyFilter: BookmarkFilter = { source: null, tag: null, platform: null, hideRemoved: false, limit: 0 };

export const browsersDetect = () => invoke<BrowserSource[]>("browsers_detect");
export const browserAllow = (sourceId: string, allowed: boolean) => invoke<void>("browser_allow", { sourceId, allowed });
export const bookmarksImport = (sourceId: string, dryRun: boolean) =>
  invoke<BookmarkImportReport>("bookmarks_import", { sourceId, dryRun });
export const bookmarksQuery = (q: string, filter: BookmarkFilter) => invoke<BookmarkPage>("bookmarks_query", { q, filter });
export const bookmarkAdd = (url: string, note?: string) => invoke<Bookmark>("bookmark_add", { url, note: note ?? null });
export const bookmarksEnrich = (ids?: string[]) => invoke<EnrichReport>("bookmarks_enrich", { ids: ids ?? null });
export const bookmarksEnrichCancel = () => invoke<void>("bookmarks_enrich_cancel");
export const memories = (date: string, salt: number) => invoke<Memory[]>("memories", { date, salt });
export const memoryDismiss = (id: string) => invoke<void>("memory_dismiss", { id });

/** An `html:` source id for an exported bookmarks file the user picked. */
export const htmlSource = (path: string) => `html:${path}`;

export interface JobProgress {
  job: string;
  done: number;
  total: number;
  message: string;
}

/** `jobs://progress` for one job (`bookmarks-import`, `bookmarks-enrich`). */
export function onProgress(job: string, cb: (p: JobProgress) => void): Promise<UnlistenFn> {
  return listen<JobProgress>("jobs://progress", (e) => {
    if (e.payload.job === job) cb(e.payload);
  });
}

/** `vault://changed`: a pasted link got its title, or an import wrote notes. */
export function onVaultChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("vault://changed", () => cb());
}

/** Commands reject with `HqError`; anything else becomes one. */
export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as HqError).message);
  return String(e);
}

export const PLATFORM_LABEL: Record<Platform, string> = {
  youtube: "YouTube",
  github: "GitHub",
  x: "X",
  reddit: "Reddit",
  hackernews: "Hacker News",
  instagram: "Instagram",
  tiktok: "TikTok",
  pdf: "PDF",
  article: "Article",
};

/** The short tag a badge shows, terminal style. */
export const PLATFORM_SHORT: Record<Platform, string> = {
  youtube: "yt",
  github: "gh",
  x: "x",
  reddit: "rd",
  hackernews: "hn",
  instagram: "ig",
  tiktok: "tt",
  pdf: "pdf",
  article: "web",
};

/** `www.example.com/a/b` → `example.com`. */
export function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** "3 days ago", "2 months ago", "1 year ago". */
export function ageLabel(days: number): string {
  if (days < 1) return "today";
  if (days < 2) return "yesterday";
  if (days < 60) return `${days} days ago`;
  if (days < 365 * 2) return `${Math.round(days / 30)} months ago`;
  return `${Math.round(days / 365)} years ago`;
}
