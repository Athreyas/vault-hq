import { describe, expect, it } from "vitest";
import type { WidgetCatalogEntry } from "../../bindings/WidgetCatalogEntry.ts";
import type { WidgetLayout } from "../../bindings/WidgetLayout.ts";
import { addWidget, ago, bytes, canAdd, dueLabel, move, moveById, pollSecs, removeWidget, setConfig, uptime, widgetTitle, zoneTime } from "./layout.ts";

const entry = (kind: WidgetCatalogEntry["kind"], multiple: boolean, refreshSecs = 60): WidgetCatalogEntry => ({
  kind,
  title: kind,
  description: "",
  refreshSecs,
  multiple,
  defaultConfig: { title: "", zones: [] },
  fields: [],
});

const layout: WidgetLayout = {
  version: 1,
  widgets: [
    { id: "a", kind: "clock", config: {} },
    { id: "b", kind: "system", config: {} },
    { id: "c", kind: "focus", config: { title: " Next up " } },
  ],
};
const ids = (l: WidgetLayout) => l.widgets.map((w) => w.id).join("");

describe("ordering", () => {
  it("moves and clamps", () => {
    expect(move([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(move([1, 2, 3], 2, 0)).toEqual([3, 1, 2]);
    expect(move([1, 2, 3], 1, 99)).toEqual([1, 3, 2]);
    expect(move([1, 2, 3], 5, 0)).toEqual([1, 2, 3]);
  });
  it("moves by id without touching the input", () => {
    expect(ids(moveById(layout, "c", 0))).toBe("cab");
    expect(ids(moveById(layout, "zz", 0))).toBe("abc");
    expect(ids(layout)).toBe("abc");
  });
});

describe("adding and removing", () => {
  it("refuses a second single-use widget", () => {
    expect(canAdd(entry("system", false), layout)).toBe(false);
    expect(canAdd(entry("inbox", false), layout)).toBe(true);
    expect(canAdd(entry("clock", true), layout)).toBe(true);
  });
  it("adds with a copy of the defaults and no id", () => {
    const e = entry("json_status", true);
    const l = addWidget(layout, e);
    const w = l.widgets[3]!;
    expect(w.id).toBe("");
    expect(w.config).toEqual(e.defaultConfig);
    (w.config.zones as unknown[]).push(1);
    expect(e.defaultConfig.zones).toEqual([]);
  });
  it("removes and configures", () => {
    expect(ids(removeWidget(layout, "b"))).toBe("ac");
    expect(setConfig(layout, "a", { seconds: true }).widgets[0]!.config).toEqual({ seconds: true });
  });
});

describe("formatting", () => {
  it("polls json no faster than a minute", () => {
    expect(pollSecs(entry("json_status", true), { intervalSecs: 5 })).toBe(60);
    expect(pollSecs(entry("json_status", true), { intervalSecs: 900 })).toBe(900);
    expect(pollSecs(entry("system", false, 5), {})).toBe(5);
    expect(pollSecs(undefined, {})).toBe(0);
  });
  it("titles", () => {
    expect(widgetTitle(layout.widgets[2]!, entry("focus", true))).toBe("Next up");
    expect(widgetTitle(layout.widgets[0]!, entry("clock", true))).toBe("clock");
  });
  it("labels", () => {
    expect([-2, 0, 1, 5].map(dueLabel)).toEqual(["2d late", "today", "tomorrow", "in 5d"]);
    expect(dueLabel(null)).toBe("");
    expect(bytes(512)).toBe("512 B");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(bytes(250 * 1024 ** 3)).toBe("250 GB");
    expect(uptime(59)).toBe("0m");
    expect(uptime(2 * 86400 + 3 * 3600)).toBe("2d 3h");
    expect(ago(0, 30_000)).toBe("just now");
    expect(ago(0, 5 * 60_000)).toBe("5m ago");
  });
  it("time zones", () => {
    expect(zoneTime("UTC", new Date(Date.UTC(2026, 0, 1, 9, 5)), false)).toBe("09:05");
    expect(zoneTime("Not/AZone", new Date(), false)).toBeNull();
  });
});
