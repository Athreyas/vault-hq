import { describe, expect, it } from "vitest";
import { HqError, notBuilt, toHqError } from "./api.ts";

describe("toHqError", () => {
  it("keeps the core's error shape", () => {
    const e = toHqError({ kind: "Conflict", message: "changed", current: { text: "t", hash: "h" } }, "note_save");
    expect(e.kind).toBe("Conflict");
    expect(e.current?.hash).toBe("h");
  });
  it("turns Tauri's missing-command refusal into Unavailable", () => {
    expect(toHqError("Command home not found", "home").kind).toBe("Unavailable");
    expect(notBuilt(toHqError("Command home not allowed by ACL", "home"))).toBe(true);
  });
  it("leaves other strings Internal and unknown kinds Internal", () => {
    expect(toHqError("disk full", "note_save").kind).toBe("Internal");
    expect(toHqError({ kind: "Weird", message: "m" }, "x").kind).toBe("Internal");
    expect(toHqError(new HqError("Denied", "no"), "x").kind).toBe("Denied");
  });
});
