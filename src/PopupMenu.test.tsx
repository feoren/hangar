// @vitest-environment happy-dom

// Roles of the floating menu's rows: a choice among neighbors is a radio item, a switch of its own
// (the sidebar's "One line") a checkbox item, so screen readers do not count it to the group.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { PopupMenu, type PopupItem } from "./PopupMenu";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
});

const run = () => undefined;

describe("PopupMenu roles", () => {
  it("renders a toggle as a checkbox item beside radio choices", () => {
    const items: PopupItem[] = [
      { type: "heading", key: "h", label: "Group sessions" },
      { type: "action", key: "program", label: "By program", current: true, run },
      { type: "action", key: "project", label: "By project", current: false, run },
      { type: "separator", key: "s" },
      { type: "action", key: "one-line", label: "One line", current: false, toggle: true, run },
      { type: "action", key: "plain", label: "Plain", run },
    ];
    root = createRoot(host);
    act(() => {
      root?.render(
        <I18nContext.Provider value={createTranslator("en")}>
          <PopupMenu items={items} label="Group" position={{ x: 0, y: 0 }} onClose={run} />
        </I18nContext.Provider>,
      );
    });
    const role = (label: string) =>
      [...document.querySelectorAll("button")].find((b) => b.textContent === label);
    expect(role("By program")?.getAttribute("role")).toBe("menuitemradio");
    expect(role("By program")?.getAttribute("aria-checked")).toBe("true");
    expect(role("One line")?.getAttribute("role")).toBe("menuitemcheckbox");
    expect(role("One line")?.getAttribute("aria-checked")).toBe("false");
    expect(role("Plain")?.getAttribute("role")).toBe("menuitem");
  });
});
