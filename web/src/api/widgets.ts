// The widget commands (SPEC 10, 12). Stream D folds this into api.ts when the shell lands;
// until then it is the only file in the widget UI that calls `invoke`.
import { invoke } from "@tauri-apps/api/core";
import type { HqError } from "../bindings/HqError.ts";
import type { WidgetCatalogEntry } from "../bindings/WidgetCatalogEntry.ts";
import type { WidgetData } from "../bindings/WidgetData.ts";
import type { WidgetKind } from "../bindings/WidgetKind.ts";
import type { WidgetLayout } from "../bindings/WidgetLayout.ts";

export const widgetsCatalog = () => invoke<WidgetCatalogEntry[]>("widgets_catalog");

export const widgetsLayoutGet = () => invoke<WidgetLayout>("widgets_layout_get");

/** Saves and returns the layout as stored: new widgets come back with their ids. */
export const widgetsLayoutSet = (layout: WidgetLayout) => invoke<WidgetLayout>("widgets_layout_set", { layout });

export const widgetData = (kind: WidgetKind, config: Record<string, unknown>) => invoke<WidgetData>("widget_data", { kind, config });

/** A command's error as text: HQ's own `{kind, message}`, or whatever else was thrown. */
export function errorText(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as HqError).message);
  return String(e);
}
