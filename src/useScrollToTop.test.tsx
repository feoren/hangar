// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useScrollToTop } from "./useScrollToTop";

let host: HTMLDivElement;
let root: Root | null = null;

function Panel({ page }: { page: string }) {
  const panel = useRef<HTMLDivElement>(null);
  useScrollToTop(panel, page);
  return (
    <div ref={panel} className="panel">
      {page}
    </div>
  );
}

function render(page: string) {
  act(() => {
    root ??= createRoot(host);
    root.render(<Panel page={page} />);
  });
}

function panel(): HTMLElement {
  const element = host.querySelector<HTMLElement>(".panel");
  if (element === null) throw new Error("panel missing");
  return element;
}

beforeEach(() => {
  Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
  host = document.createElement("div");
  document.body.appendChild(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
});

describe("useScrollToTop", () => {
  it("starts at the top when it opens", () => {
    render("general");
    expect(panel().scrollTop).toBe(0);
  });

  it("goes back to the top when the page changes", () => {
    render("general");
    panel().scrollTop = 240;
    render("sub-agents");
    expect(panel().scrollTop).toBe(0);
  });

  it("keeps the place while the page stays the same", () => {
    render("general");
    panel().scrollTop = 240;
    render("general");
    expect(panel().scrollTop).toBe(240);
  });
});
