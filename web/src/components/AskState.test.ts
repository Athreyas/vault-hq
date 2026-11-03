import { describe, expect, it } from "vitest";
import type { AskEnvelope } from "../bindings/AskEnvelope.ts";
import { applyEvent, fromConversation, newTurn, scopeLabel } from "./AskState.ts";

const hit = {
  id: "v:01-Projects/garden.md",
  vault: "v",
  rel: "01-Projects/garden.md",
  stem: "garden",
  title: "Garden",
  heading: "Garden",
  excerpt: "Lay the drip line",
  score: 0.8,
  kind: "note" as const,
};

describe("applyEvent", () => {
  it("streams tokens, then replaces them with the tidied answer", () => {
    const events: AskEnvelope[] = [
      { id: "a", kind: "sources", sources: [hit], semantic: true },
      { id: "a", kind: "token", text: "Along " },
      { id: "a", kind: "token", text: "(Excerpt 1)" },
      { id: "a", kind: "done", answer: "Along [[garden]]", model: "m", stopped: false, conversation: "v:06-Memory/conversations/x.md" },
    ];
    let t = newTurn("ask", "where?", { vaults: [], kinds: "both" });
    t = applyEvent(t, events[0]!);
    expect(t.sources).toHaveLength(1);
    t = applyEvent(applyEvent(t, events[1]!), events[2]!);
    expect(t.answer).toBe("Along (Excerpt 1)");
    expect(t.streaming).toBe(true);
    t = applyEvent(t, events[3]!);
    expect(t.answer).toBe("Along [[garden]]");
    expect(t.streaming).toBe(false);
  });

  it("keeps what streamed when an error ends it", () => {
    let t = newTurn("ask", "q", { vaults: [], kinds: "both" });
    t = applyEvent(t, { id: "a", kind: "token", text: "half" });
    t = applyEvent(t, { id: "a", kind: "error", error: "Unavailable", message: "Ollama is not reachable" });
    expect(t.answer).toBe("half");
    expect(t.error).toMatch(/not reachable/);
    expect(t.streaming).toBe(false);
  });
});

describe("conversations and scope", () => {
  it("turns a saved conversation into finished turns", () => {
    const turns = fromConversation({
      id: "v:x",
      title: "t",
      created: "2026-11-01",
      updated: "2026-11-02",
      turns: [{ at: "2026-11-01 09:00", question: "q", answer: "a [[garden]]", model: "m", sources: ["garden"] }],
    });
    expect(turns[0]?.streaming).toBe(false);
    expect(turns[0]?.cited).toEqual(["garden"]);
  });

  it("labels scopes", () => {
    expect(scopeLabel({ vaults: [], kinds: "both" })).toBe("notes + bookmarks · all time");
    expect(scopeLabel({ vaults: ["abc123def"], kinds: "notes", days: 7 }, new Map([["abc123def", "home"]]))).toBe("notes · 7 days · home");
  });
});
