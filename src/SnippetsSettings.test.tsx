// @vitest-environment happy-dom

// Snippets: the intro names how to reach them, an empty list names the next step with its button
// right below, and the variables stay folded below the list.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import type { Snippet } from "./snippets";
import { SnippetsSettings } from "./SnippetsSettings";

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

function render(snippets: Snippet[], onChange: (snippets: Snippet[]) => void = () => undefined) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <SnippetsSettings snippets={snippets} onChange={onChange} />
      </I18nContext.Provider>,
    );
  });
}

const addButton = () =>
  [...host.querySelectorAll("button")].find((button) => button.textContent === t("snippets.add"));

describe("SnippetsSettings", () => {
  it("names the shortcut that opens the snippets", () => {
    render([]);
    expect(host.textContent).toContain(
      t("snippets.help", { keys: formatShortcut(SHORTCUT_LABELS.launcher, t) }),
    );
  });

  it("names the next step when there are none, with the add button right below", () => {
    let added: Snippet[] = [];
    render([], (next) => {
      added = next;
    });
    const empty = host.querySelector(".snippet-empty");
    expect(empty?.textContent).toBe(t("snippets.empty"));
    expect(host.querySelector(".snippet-list")).toBeNull();
    expect(empty?.nextElementSibling?.contains(addButton() ?? null)).toBe(true);
    act(() => {
      addButton()?.click();
    });
    expect(added).toHaveLength(1);
    expect(added[0]?.text).toBe("");
  });

  it("shows the list instead of the empty text once there are snippets", () => {
    render([{ id: "a", text: "Review the changes" }]);
    expect(host.querySelector(".snippet-empty")).toBeNull();
    expect(host.querySelectorAll(".snippet-row")).toHaveLength(1);
  });

  it("keeps the variables folded below the list", () => {
    render([{ id: "a", text: "Hello" }]);
    const details = host.querySelector("details");
    expect(details?.open).toBe(false);
    expect(details?.querySelector("summary")?.textContent).toBe(t("snippets.variables"));
    expect(details?.textContent).toContain("{{branch}}");
    const list = host.querySelector(".snippet-list");
    expect(
      list !== null &&
        details !== null &&
        (list.compareDocumentPosition(details) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    ).toBe(true);
  });
});
