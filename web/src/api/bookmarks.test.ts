import { describe, expect, it } from "vitest";
import { ageLabel, errorMessage, host, htmlSource, PLATFORM_LABEL, PLATFORM_SHORT } from "./bookmarks.ts";

describe("bookmark helpers", () => {
  it("shortens hosts", () => {
    expect(host("https://www.example.com/a/b")).toBe("example.com");
    expect(host("https://news.ycombinator.com/item?id=1")).toBe("news.ycombinator.com");
    expect(host("not a url")).toBe("not a url");
  });

  it("says how long ago", () => {
    expect(ageLabel(0)).toBe("today");
    expect(ageLabel(1)).toBe("yesterday");
    expect(ageLabel(9)).toBe("9 days ago");
    expect(ageLabel(90)).toBe("3 months ago");
    expect(ageLabel(1100)).toBe("3 years ago");
  });

  it("labels every platform", () => {
    expect(Object.keys(PLATFORM_LABEL).sort()).toEqual(Object.keys(PLATFORM_SHORT).sort());
    expect(PLATFORM_SHORT.hackernews).toBe("hn");
  });

  it("reads command errors", () => {
    expect(errorMessage({ kind: "Denied", message: "Tick it first." })).toBe("Tick it first.");
    expect(errorMessage("plain")).toBe("plain");
    expect(htmlSource("/tmp/b.html")).toBe("html:/tmp/b.html");
  });
});
