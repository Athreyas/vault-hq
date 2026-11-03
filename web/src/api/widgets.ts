// The widget commands (SPEC 10, 12), as the widget UI calls them. They go through
// api.ts's `call`, which stays the only place that touches `invoke`.
import { call } from "../api.ts";
import type { WidgetKind } from "../bindings/WidgetKind.ts";
import type { WidgetLayout } from "../bindings/WidgetLayout.ts";

export const widgetsCatalog = () => call("widgets_catalog", {});

export const widgetsLayoutGet = () => call("widgets_layout_get", {});

/** Saves and returns the layout as stored: new widgets come back with their ids. */
export const widgetsLayoutSet = (layout: WidgetLayout) => call("widgets_layout_set", { layout });

export const widgetData = (kind: WidgetKind, config: Record<string, unknown>) => call("widget_data", { kind, config });

/** A command's error as text (api.ts turns every failure into an HqError). */
export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
