import { describe, expect, it } from "vitest";
import { bookmarkReportLine, OnboardingError, taskReportLine, toError, unavailable } from "../../api/onboarding.ts";
import type { PlanItem } from "../../bindings/PlanItem.ts";
import { withTicks } from "./ImportAssistant.tsx";
import { neighbour } from "./Onboarding.tsx";
import { diffStats, parseDiff } from "./parts.tsx";
import { joinPath } from "./StepBrain.tsx";
import { splitModels } from "./StepMore.tsx";

describe("diff", () => {
  const diff = "--- a/.cursor/mcp.json\n+++ b/.cursor/mcp.json\n@@ -1,3 +1,7 @@\n {\n-  \"a\": 1\n+  \"a\": 1,\n+  \"b\": 2\n }\n";
  it("classifies lines", () => {
    const lines = parseDiff(diff);
    expect(lines.map((l) => l.kind)).toEqual(["meta", "meta", "hunk", "ctx", "del", "add", "add", "ctx"]);
    expect(diffStats(lines)).toEqual({ added: 2, removed: 1 });
  });
  it("is empty for no change", () => {
    expect(parseDiff("")).toEqual([]);
  });
});

describe("errors", () => {
  it("keeps the core's error kinds", () => {
    const e = toError({ kind: "Conflict", message: "changed" }, "agent_config_apply");
    expect(e.kind).toBe("Conflict");
    expect(e.message).toBe("changed");
    expect(toError({ kind: "Weird", message: "x" }, "c").kind).toBe("Internal");
  });
  it("treats a missing command or no Tauri as unavailable", () => {
    expect(unavailable(toError("command vault_create not found", "vault_create"))).toBe(true);
    expect(unavailable(toError(new TypeError("Cannot read properties of undefined (reading 'invoke')"), "x"))).toBe(true);
    expect(unavailable(toError("disk full", "x"))).toBe(false);
    expect(unavailable(new OnboardingError("Unavailable", "x"))).toBe(true);
  });
});

describe("helpers", () => {
  it("moves between steps, clamped", () => {
    expect(neighbour("welcome", -1)).toBe("welcome");
    expect(neighbour("welcome", 1)).toBe("brain");
    expect(neighbour("done", 1)).toBe("done");
  });
  it("joins a vault path", () => {
    expect(joinPath("/x/Vault HQ/", "home")).toBe("/x/Vault HQ/home");
    expect(joinPath("/x", " a/b:c ")).toBe("/x/a-b-c");
  });
  it("splits models", () => {
    expect(splitModels(["nomic-embed-text:latest", "qwen2.5:7b", "bge-m3"])).toEqual({
      embed: ["nomic-embed-text:latest", "bge-m3"],
      chat: ["qwen2.5:7b"],
    });
  });
  it("summarises a bookmark import", () => {
    const r = { sourceId: "chrome:Default", dryRun: true, found: 10, added: 7, updated: 0, unchanged: 3, removed: 0, duplicates: 0, sample: [], errors: [] };
    expect(bookmarkReportLine(r)).toBe("10 found · 7 new · 3 already in the vault");
  });
  it("summarises a Taskwarrior import", () => {
    const r = { dryRun: true, total: 5, pending: 3, waiting: 1, completed: 1, deleted: 2, templates: 0, duplicates: 1, written: 4, targets: [], sample: [], warnings: [] };
    expect(taskReportLine(r)).toBe("5 tasks: 3 pending, 1 waiting, 1 done · 2 deleted (not imported) · 1 already in the vault · 4 would be written");
  });
  it("sends only the user's ticks", () => {
    const item = (id: string, ticked: boolean): PlanItem => ({
      id,
      group: "loose",
      summary: id,
      ticked,
      reversible: true,
      action: { kind: "move", from: id, to: `00-Inbox/${id}` },
    });
    const out = withTicks([item("a", true), item("b", false)], new Set(["b"]));
    expect(out.map((i) => [i.id, i.ticked])).toEqual([
      ["a", false],
      ["b", true],
    ]);
  });
});
