// @vitest-environment happy-dom

// A program row in "New session": the whole row starts with the default, the "Model" pill shows
// only when there is something to choose, is a sibling button and never starts the row itself.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SessionModel } from "./api";
import { createTranslator, I18nContext } from "./i18n";
import { modelSections, type ModelSection } from "./localLlm";
import { ProgramRow } from "./ProgramRow";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

const SECTIONS = modelSections(
  { name: "Claude Code", models: [{ id: "opus", label: "Opus 5.5" }], localSources: [] },
  null,
);

describe("ProgramRow", () => {
  let root: Root;
  let host: HTMLUListElement;

  beforeEach(() => {
    host = document.createElement("ul");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    document.body.replaceChildren();
  });

  function render(models: readonly ModelSection[] = SECTIONS) {
    const onStart = vi.fn<(model?: SessionModel) => void>();
    const onMenuClose = vi.fn();
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <ProgramRow
            name="Claude Code"
            commandLine="claude --verbose"
            badge={{ kind: "letters", text: "CC", color: "#5aa9ff" }}
            version="2.1.0"
            updateAvailable={false}
            models={models}
            onStart={onStart}
            onMenuClose={onMenuClose}
          />
        </I18nContext.Provider>,
      );
    });
    return { onStart, onMenuClose };
  }

  const row = () => host.querySelector<HTMLButtonElement>("button.profile");
  const pill = () => host.querySelector<HTMLButtonElement>("button.profile-models");

  it("starts with the default from the whole row, named for what it does", () => {
    const { onStart } = render();
    expect(row()?.getAttribute("aria-label")).toBe("Start a new session with Claude Code");
    expect(row()?.title).toBe("Start a new session with Claude Code\nclaude --verbose");
    expect(row()?.textContent).toContain("2.1.0");
    act(() => {
      row()?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith();
  });

  it("has no plus and no arrow, only the word", () => {
    render();
    expect(host.querySelector("svg")).toBeNull();
    expect(host.textContent).not.toContain("›");
    expect(pill()?.textContent).toBe("Model");
  });

  it("shows the pill only when there are models to choose", () => {
    render([]);
    expect(pill()).toBeNull();
  });

  it("keeps the pill a sibling of the row button, not inside it", () => {
    render();
    expect(row()?.contains(pill())).toBe(false);
    expect(host.querySelector("button button")).toBeNull();
  });

  it("opens the menu from the pill without starting a session", () => {
    const { onStart } = render();
    act(() => {
      pill()?.click();
    });
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    expect(onStart).not.toHaveBeenCalled();
  });

  it("starts with the chosen model", () => {
    const { onStart, onMenuClose } = render();
    act(() => {
      pill()?.click();
    });
    const opus = [...document.querySelectorAll<HTMLElement>('[role^="menuitem"]')].find((item) =>
      item.textContent.includes("Opus 5.5"),
    );
    act(() => {
      opus?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith({ source: "provider", id: "opus" });
    expect(onMenuClose).toHaveBeenCalledOnce();
  });
});
