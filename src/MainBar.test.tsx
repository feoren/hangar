// @vitest-environment happy-dom

// The bar above the work area: the active session's name and the right panel's tabs, which left
// the sidebar's footer for it.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { SideTab } from "./layout";
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

function render(title: string | null, sideTab: SideTab | null, onToggle = vi.fn()) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <MainBar title={title} sideTab={sideTab} onToggleSideTab={onToggle} />
      </I18nContext.Provider>,
    );
  });
  return onToggle;
}

const tabs = () => [...host.querySelectorAll<HTMLButtonElement>(".main-bar-tab")];

describe("MainBar", () => {
  it("shows the active session's name and nothing on the home screen", () => {
    render("build the parser", null);
    expect(host.querySelector(".main-bar-title")?.textContent).toBe("build the parser");
    render(null, null);
    expect(host.querySelector(".main-bar-title")?.textContent).toBe("");
  });

  it("offers the three panel tabs in order and marks the one on screen", () => {
    render(null, "changes");
    expect(tabs().map((button) => button.getAttribute("aria-pressed"))).toEqual([
      "false",
      "true",
      "false",
    ]);
  });

  it("toggles the clicked tab", () => {
    const onToggle = render(null, null);
    act(() => {
      tabs()[2]?.click();
    });
    expect(onToggle).toHaveBeenCalledWith("files");
  });
});
