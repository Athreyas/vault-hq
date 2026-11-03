import { describe, expect, it } from "vitest";
import { countsLine, OnboardingError, toError, unavailable } from "../../api/onboarding.ts";
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
  it("summarises another stream's import report", () => {
    expect(countsLine({ created: 3, skipped: 1, dryRun: true })).toBe("3 created · 1 skipped");
    expect(countsLine({})).toBe("done");
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
