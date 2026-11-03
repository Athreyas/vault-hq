// No DOM environment is installed for vitest, and server rendering never runs error
// boundaries, so this drives the boundary through React's own contract: the static
// hook React calls when a child throws, then render() with the state it produced.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import ErrorBoundary from "./ErrorBoundary.tsx";

function Boom(): never {
  throw new Error("pulls is undefined");
}

function boundary(resetKey = "a") {
  return new ErrorBoundary({ name: "view", resetKey, children: <Boom /> });
}

describe("ErrorBoundary", () => {
  it("a throwing child renders the fallback with its message", () => {
    const b = boundary();
    let thrown: unknown;
    try {
      Boom();
    } catch (e) {
      thrown = e;
    }
    b.state = { ...b.state, ...ErrorBoundary.getDerivedStateFromError(thrown) };
    const html = renderToStaticMarkup(<>{b.render()}</>);
    expect(html).toContain('role="alert"');
    expect(html).toContain("This view hit an error:");
    expect(html).toContain("pulls is undefined");
    expect(html).toContain("reload");
  });

  it("wraps a non-Error throw", () => {
    expect(ErrorBoundary.getDerivedStateFromError("plain string").error?.message).toBe("plain string");
  });

  it("renders children while nothing has thrown, and clears on a new key", () => {
    const ok = new ErrorBoundary({ name: "view", children: <p>fine</p> });
    expect(renderToStaticMarkup(<>{ok.render()}</>)).toBe("<p>fine</p>");
    const state = { error: new Error("x"), key: "a" };
    expect(ErrorBoundary.getDerivedStateFromProps({ name: "view", resetKey: "b", children: null }, state)).toEqual({ error: null, key: "b" });
    expect(ErrorBoundary.getDerivedStateFromProps({ name: "view", resetKey: "a", children: null }, state)).toBeNull();
  });
});
