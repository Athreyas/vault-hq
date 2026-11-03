import { describe, expect, it } from "vitest";
import { appendUnderHeading, captureKind, isLoneUrl, lineDiff, metricLabel } from "./text.ts";

describe("capture routing", () => {
  it("treats a lone URL as a bookmark", () => {
    expect(isLoneUrl("https://example.com/a?b=1")).toBe(true);
    expect(isLoneUrl("  http://example.com  ")).toBe(true);
    expect(captureKind("https://example.com", true)).toBe("bookmark");
  });
  it("keeps text with a URL in it as a note or task", () => {
    expect(isLoneUrl("read https://example.com later")).toBe(false);
    expect(isLoneUrl("ftp://example.com")).toBe(false);
    expect(isLoneUrl("https://")).toBe(false);
    expect(captureKind("read https://example.com later", false)).toBe("note");
    expect(captureKind("call the plumber", true)).toBe("task");
  });
});

describe("appendUnderHeading", () => {
  const note = "---\ntitle: d\n---\n## Quick Wins\n- one\n\n## Work\n- w\n\n## Tomorrow\n";
  it("adds to the end of the section, before blank lines", () => {
    expect(appendUnderHeading(note, "Quick Wins", "two")).toBe("---\ntitle: d\n---\n## Quick Wins\n- one\n- two\n\n## Work\n- w\n\n## Tomorrow\n");
  });
  it("works on the last and on an empty section", () => {
    expect(appendUnderHeading(note, "tomorrow", "plan")).toBe(note.replace("## Tomorrow\n", "## Tomorrow\n- plan\n"));
    expect(appendUnderHeading("## Work\n## Next\n", "Work", "x")).toBe("## Work\n- x\n## Next\n");
  });
  it("keeps subsections inside the section", () => {
    expect(appendUnderHeading("## Work\n### a\n- 1\n## End", "Work", "2")).toBe("## Work\n### a\n- 1\n- 2\n## End");
  });
  it("returns null without the heading", () => {
    expect(appendUnderHeading("# Title\nbody", "Work", "x")).toBeNull();
  });
});

describe("lineDiff", () => {
  it("marks removed and added lines", () => {
    expect(lineDiff("a\nb\nc", "a\nc\nd").filter((l) => l.kind !== " ")).toEqual([
      { kind: "-", text: "b" },
      { kind: "+", text: "d" },
    ]);
  });
});

it("labels metrics", () => {
  expect(metricLabel("_tasks_pending")).toBe("tasks pending");
});
