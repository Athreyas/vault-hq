// Pure helpers for the widget grid: ordering, adding, removing, and the small formatters
// the renderers share. No React, no invoke, so they are tested on their own.
import type { WidgetCatalogEntry } from "../../bindings/WidgetCatalogEntry.ts";
import type { WidgetInstance } from "../../bindings/WidgetInstance.ts";
import type { WidgetLayout } from "../../bindings/WidgetLayout.ts";

/** `list` with the item at `from` moved to `to` (both clamped); the input is not changed. */
export function move<T>(list: readonly T[], from: number, to: number): T[] {
  const out = [...list];
  if (from < 0 || from >= out.length) return out;
  const [item] = out.splice(from, 1);
  out.splice(Math.max(0, Math.min(out.length, to)), 0, item as T);
  return out;
}

export function moveById(layout: WidgetLayout, id: string, to: number): WidgetLayout {
  const from = layout.widgets.findIndex((w) => w.id === id);
  return from < 0 ? layout : { ...layout, widgets: move(layout.widgets, from, to) };
}

/** A single-use kind (This Mac, Inbox) can't be added twice. */
export function canAdd(entry: WidgetCatalogEntry, layout: WidgetLayout): boolean {
  return entry.multiple || !layout.widgets.some((w) => w.kind === entry.kind);
}

/** Appends a fresh widget; the server gives it an id when the layout is saved. */
export function addWidget(layout: WidgetLayout, entry: WidgetCatalogEntry): WidgetLayout {
  const w: WidgetInstance = { id: "", kind: entry.kind, config: structuredClone(entry.defaultConfig) };
  return { ...layout, widgets: [...layout.widgets, w] };
}

export function removeWidget(layout: WidgetLayout, id: string): WidgetLayout {
  return { ...layout, widgets: layout.widgets.filter((w) => w.id !== id) };
}

export function setConfig(layout: WidgetLayout, id: string, config: Record<string, unknown>): WidgetLayout {
  return { ...layout, widgets: layout.widgets.map((w) => (w.id === id ? { ...w, config } : w)) };
}

/** Seconds between data requests. JSON status follows its own interval (the server
 *  never fetches more often than once a minute anyway). */
export function pollSecs(entry: WidgetCatalogEntry | undefined, config: Record<string, unknown>): number {
  if (!entry) return 0;
  if (entry.kind === "json_status") {
    const n = Number(config.intervalSecs);
    return Math.max(60, Number.isFinite(n) ? n : 300);
  }
  return entry.refreshSecs;
}

export function widgetTitle(w: WidgetInstance, entry: WidgetCatalogEntry | undefined): string {
  const t = typeof w.config.title === "string" ? w.config.title.trim() : "";
  return t || entry?.title || w.kind;
}

export function dueLabel(dueIn: number | null): string {
  if (dueIn === null) return "";
  if (dueIn < 0) return `${-dueIn}d late`;
  if (dueIn === 0) return "today";
  if (dueIn === 1) return "tomorrow";
  return `in ${dueIn}d`;
}

export function bytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}

export function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** Time in an IANA zone, or null if this browser doesn't know the zone. */
export function zoneTime(zone: string, at: Date, hour12: boolean): string | null {
  try {
    return new Intl.DateTimeFormat("en-GB", { timeZone: zone, hour: "2-digit", minute: "2-digit", hour12 }).format(at);
  } catch {
    return null;
  }
}
