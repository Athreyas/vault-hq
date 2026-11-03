import { describe, expect, it } from "vitest";
import type { Task } from "../bindings/Task.ts";
import { addDays, parseFilter, projectTitles, parsePriority, parseQuickAdd, patchFrom, resolveDate, resolveRecur, taskToSyntax } from "./TasksSyntax.ts";

// 2026-11-02 is a Monday
const TODAY = "2026-11-02";

function task(over: Partial<Task> = {}): Task {
  return {
    key: "abc123",
    id: "abc123",
    vault: "v",
    note: "v:09-Planning/Tasks.md",
    noteTitle: "Tasks",
    line: 3,
    status: "pending",
    description: "Fix the gate",
    tags: ["home"],
    project: null,
    priority: null,
    created: "2026-10-01",
    start: null,
    scheduled: null,
    due: null,
    done: null,
    cancelled: null,
    recur: null,
    depends: [],
    annotations: [],
    blocked: false,
    blocking: false,
    hidden: false,
    urgency: 1,
    raw: "- [ ] Fix the gate #home",
    ...over,
  };
}

describe("dates", () => {
  it("resolves words relative to today", () => {
    expect(resolveDate("today", TODAY)).toBe("2026-11-02");
    expect(resolveDate("tomorrow", TODAY)).toBe("2026-11-03");
    expect(resolveDate("yesterday", TODAY)).toBe("2026-11-01");
    expect(resolveDate("fri", TODAY)).toBe("2026-11-06");
    expect(resolveDate("Friday", TODAY)).toBe("2026-11-06");
    expect(resolveDate("mon", TODAY)).toBe("2026-11-09"); // the next Monday, not today
    expect(resolveDate("thurs", TODAY)).toBe("2026-11-05");
    expect(resolveDate("eow", TODAY)).toBe("2026-11-08");
    expect(resolveDate("eom", TODAY)).toBe("2026-11-30");
    expect(resolveDate("eoy", TODAY)).toBe("2026-12-31");
    expect(resolveDate("+3d", TODAY)).toBe("2026-11-05");
    expect(resolveDate("2w", TODAY)).toBe("2026-11-16");
    expect(resolveDate("1m", "2026-01-31")).toBe("2026-02-28");
    expect(resolveDate("1y", "2028-02-29")).toBe("2029-02-28");
    expect(resolveDate("2026-12-25", TODAY)).toBe("2026-12-25");
  });

  it("rejects what is not a date", () => {
    for (const w of ["someday", "2026-02-30", "", "mo", "3x", "13/01"]) expect(resolveDate(w, TODAY)).toBeNull();
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
  });

  it("maps priorities and repeats", () => {
    expect(parsePriority("H")).toBe("high");
    expect(parsePriority("hh")).toBe("highest");
    expect(parsePriority("none")).toBeNull();
    expect(parsePriority("x")).toBeUndefined();
    expect(resolveRecur("weekly")).toBe("every week");
    expect(resolveRecur("3d")).toBe("every 3 days");
    expect(resolveRecur("1m")).toBe("every month");
    expect(resolveRecur("weekdays")).toBe("every weekday");
    expect(resolveRecur("0d")).toBeNull();
    expect(resolveRecur("sometimes")).toBeNull();
  });
});

describe("quick add", () => {
  it("pulls every field out of the line", () => {
    const { input, errors } = parseQuickAdd("Lay the main line #garden #outdoor +home/garden due:fri p:h rec:weekly sched:+3d start:today dep:aaa111,bbb222", TODAY);
    expect(errors).toEqual([]);
    expect(input.description).toBe("Lay the main line");
    expect(input.tags).toEqual(["garden", "outdoor"]);
    expect(input.project).toBe("home/garden");
    expect(input.due).toBe("2026-11-06");
    expect(input.priority).toBe("high");
    expect(input.recur).toBe("every week");
    expect(input.scheduled).toBe("2026-11-05");
    expect(input.start).toBe("2026-11-02");
    expect(input.depends).toEqual(["aaa111", "bbb222"]);
    expect(input.waiting).toBe(false);
  });

  it("keeps plain words, numbers and issue refs in the description", () => {
    const { input } = parseQuickAdd("Reply to #123 at 3:30 about C++ and +3d", TODAY);
    expect(input.description).toBe("Reply to #123 at 3:30 about C++ and +3d");
    expect(input.tags).toEqual([]);
    expect(input.project).toBeNull();
  });

  it("wait: makes it waiting until the date", () => {
    const { input } = parseQuickAdd("Chase the parts wait:mon project:house", TODAY);
    expect(input.waiting).toBe(true);
    expect(input.scheduled).toBe("2026-11-09");
    expect(input.project).toBe("house");
  });

  it("reports bad syntax instead of guessing", () => {
    const { errors, input } = parseQuickAdd("x due:someday p:z rec:often", TODAY);
    expect(errors).toEqual(["due:someday", "p:z", "rec:often"]);
    expect(input.due).toBeNull();
  });
});

describe("inline edit", () => {
  it("round-trips a task through its syntax with no changes", () => {
    const t = task({ project: "house", priority: "medium", due: "2026-11-10", scheduled: "2026-11-05", recur: "every 2 weeks", depends: ["zzz999"] });
    const line = taskToSyntax(t);
    expect(line).toBe("Fix the gate #home +house p:m sched:2026-11-05 due:2026-11-10 rec:2w dep:zzz999");
    expect(patchFrom(t, parseQuickAdd(line, TODAY))).toEqual({});
  });

  it("produces only the changed fields, with null to clear", () => {
    const t = task({ priority: "high", due: "2026-11-10" });
    const patch = patchFrom(t, parseQuickAdd("Fix the side gate #home #urgent due:fri", TODAY));
    expect(patch).toEqual({ description: "Fix the side gate", tags: ["home", "urgent"], priority: null, due: "2026-11-06" });
  });

  it("leaves a rule the short form cannot show alone", () => {
    const t = task({ recur: "every 2 weeks when done" });
    expect(taskToSyntax(t)).not.toContain("rec:");
    expect(patchFrom(t, parseQuickAdd(taskToSyntax(t), TODAY))).toEqual({});
  });

  it("waiting toggles status", () => {
    const t = task({ status: "waiting", scheduled: "2026-12-01" });
    expect(taskToSyntax(t)).toContain("wait:2026-12-01");
    expect(patchFrom(t, parseQuickAdd("Fix the gate #home", TODAY))).toEqual({ scheduled: null, status: "pending" });
  });
});

describe("filter", () => {
  it("parses tags, project, dates, priority and virtual tags", () => {
    const { query, errors } = parseFilter("+garden -travel #outdoor project:home due:eow p:h +BLOCKED fence repair", TODAY);
    expect(errors).toEqual([]);
    expect(query.tagsAll).toEqual(["garden", "outdoor"]);
    expect(query.tagsNone).toEqual(["travel"]);
    expect(query.project).toBe("home");
    expect(query.dueTo).toBe("2026-11-08");
    expect(query.priority).toEqual(["highest", "high"]);
    expect(query.blocked).toBe(true);
    expect(query.text).toBe("fence repair");
  });

  it("handles status and the date virtual tags", () => {
    expect(parseFilter("status:done,cancelled", TODAY).query.status).toEqual(["done", "cancelled"]);
    expect(parseFilter("status:all", TODAY).query.status).toHaveLength(5);
    expect(parseFilter("+OVERDUE", TODAY).query.dueTo).toBe("2026-11-01");
    expect(parseFilter("+TODAY", TODAY).query).toEqual({ dueFrom: TODAY, dueTo: TODAY });
    expect(parseFilter("+WAITING", TODAY).query).toEqual({ status: ["waiting"], includeHidden: true });
    expect(parseFilter("due.before:fri due.after:tue", TODAY).query).toEqual({ dueTo: "2026-11-05", dueFrom: "2026-11-04" });
    expect(parseFilter("p:none", TODAY).query.noPriority).toBe(true);
    expect(parseFilter("status:nope due:never", TODAY).errors).toEqual(["status:nope", "due:never"]);
    expect(parseFilter("", TODAY).query).toEqual({});
  });
});

describe("projectTitles", () => {
  it("names a project by its note's title, keeping tag-only projects as they are", () => {
    const m = projectTitles([
      { project: "garden-irrigation", note: "v1:01-Projects/garden-irrigation.md", noteTitle: "Garden drip irrigation" },
      { project: "taxes", note: "v1:09-Planning/Tasks.md", noteTitle: "Tasks" },
      { project: null, note: "v1:x.md", noteTitle: "X" },
    ]);
    expect(m.get("garden-irrigation")).toBe("Garden drip irrigation");
    expect(m.has("taxes")).toBe(false);
    expect(m.size).toBe(1);
  });
});
