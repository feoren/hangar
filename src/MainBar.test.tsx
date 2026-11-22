// @vitest-environment happy-dom

// The bar above the work area: the active session's name and the one switch for the right panel.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { MainBar } from "./MainBar";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
});

function render(title: string | null, sideOpen: boolean, onToggle = vi.fn()) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <MainBar title={title} sideOpen={sideOpen} onToggleSide={onToggle} />
      </I18nContext.Provider>,
    );
  });
  return onToggle;
}

const toggle = () => host.querySelector<HTMLButtonElement>(".main-bar-toggle");

describe("MainBar", () => {
  it("shows the active session's name and nothing on the home screen", () => {
    render("build the parser", false);
    expect(host.querySelector(".main-bar-title")?.textContent).toBe("build the parser");
    render(null, false);
    expect(host.querySelector(".main-bar-title")?.textContent).toBe("");
  });

  it("has one switch for the right panel that shows whether it is open", () => {
    render(null, false);
    expect(host.querySelectorAll("button")).toHaveLength(1);
    expect(toggle()?.getAttribute("aria-label")).toBe("Right panel");
    expect(toggle()?.getAttribute("aria-pressed")).toBe("false");
    render(null, true);
    expect(toggle()?.getAttribute("aria-pressed")).toBe("true");
  });

  it("toggles the panel on click", () => {
    const onToggle = render(null, false);
    act(() => {
      toggle()?.click();
    });
    expect(onToggle).toHaveBeenCalledOnce();
  });
});
