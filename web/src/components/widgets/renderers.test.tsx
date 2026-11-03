import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { WidgetData } from "../../bindings/WidgetData.ts";
import type { WidgetInstance } from "../../bindings/WidgetInstance.ts";
import { registerWidgetRenderer, WidgetBody } from "./renderers.tsx";

const inst = (kind: WidgetInstance["kind"], config: Record<string, unknown> = {}): WidgetInstance => ({ id: "w1", kind, config });
const data = (kind: WidgetData["kind"], body: WidgetData["body"]): WidgetData => ({ kind, status: "ok", summary: "", at: 0, body });
const html = (i: WidgetInstance, d: WidgetData | null) => renderToStaticMarkup(<WidgetBody instance={i} data={d} />);

describe("renderers", () => {
  it("draws this Mac", () => {
    const out = html(
      inst("system"),
      data("system", {
        type: "system",
        now: {
          at: 0,
          cpuPct: 12.4,
          cpuCount: 10,
          memUsed: 8 * 1024 ** 3,
          memTotal: 16 * 1024 ** 3,
          memPressure: "normal",
          swapUsed: 0,
          diskFree: 100 * 1024 ** 3,
          diskTotal: 500 * 1024 ** 3,
          battery: { pct: 80, charging: true, plugged: true },
          uptimeSecs: 7200,
          top: [{ name: "editor", cpuPct: 30, mem: 1 }],
        },
        cpuHistory: [5, 10, 12],
        memHistory: [50, 50, 50],
        notes: [],
      }),
    );
    expect(out).toContain("12%");
    expect(out).toContain("100 GB free");
    expect(out).toContain("80% ↑");
    expect(out).toContain("editor");
    expect(out).toContain("<svg");
  });

  it("draws burn numbers that agree with the bars", () => {
    const day = (date: string, done: number) => ({ date, open: 5, created: 0, done });
    const out = html(
      inst("tasks_burn"),
      data("tasks_burn", { type: "tasksBurn", days: [day("2026-11-01", 3), day("2026-11-02", 0), day("2026-11-03", 1)], openNow: 5, donePerDay: 4 / 3, done7d: 4, net7d: -4 }),
    );
    expect(out).toContain("1.3/d");
    expect(out).toContain("week 4 done");
  });

  it("draws tasks, json and unavailable", () => {
    const tasks = html(
      inst("tasks_due"),
      data("tasks_due", {
        type: "taskList",
        overdue: 1,
        dueToday: 0,
        total: 3,
        tasks: [{ id: "a", title: "Fix the fence", project: null, due: "2026-11-01", dueIn: -1, priority: null, urgency: 9, started: false }],
      }),
    );
    expect(tasks).toContain("1d late");
    expect(tasks).toContain("+2 more");

    const json = html(
      inst("json_status"),
      data("json_status", {
        type: "jsonStatus",
        url: "http://status.example/x",
        httpStatus: 200,
        fields: [{ label: "status", value: "up", unit: "", ok: true }],
        error: null,
        fetchedAt: null,
        nextAt: 0,
      }),
    );
    expect(json).toContain("✓");
    expect(json).toContain("status.example");

    expect(html(inst("focus"), data("focus", { type: "unavailable", message: "Not connected." }))).toContain("Not connected.");
    expect(html(inst("focus"), null)).toContain("…");
  });

  it("draws the clock without data and lets a stream take over a kind", () => {
    expect(html(inst("clock", { zones: [{ label: "UTC", zone: "UTC" }] }), null)).toContain("UTC");
    registerWidgetRenderer("memories", () => <p>from F</p>);
    expect(html(inst("memories"), null)).toBe("<p>from F</p>");
  });
});
