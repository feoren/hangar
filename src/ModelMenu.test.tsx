// @vitest-environment happy-dom

// The "›" of a program row (ADR 0022): its menu's rows, and that it opens with Enter or Right
// arrow, starts with the picked model and gives the focus back on Escape.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SessionModel } from "./api";
import { createTranslator, I18nContext } from "./i18n";
import { modelSections, type LocalLlmState } from "./localLlm";
import { ModelMenuButton, modelMenuItems } from "./ModelMenu";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

const LOCAL: LocalLlmState = {
  ollama: {
    running: true,
    models: [{ id: "qwen3:8b", label: "qwen3:8b", loaded: true }],
  },
  lmstudio: {
    running: true,
    models: [{ id: "qwen/qwen3-8b", label: "Qwen3 8B", loaded: false }],
  },
};

const SECTIONS = modelSections(
  {
    name: "Claude Code",
    models: [{ id: "opus", label: "Opus 5.5" }],
    localSources: ["ollama", "lmstudio"],
  },
  LOCAL,
);

describe("modelMenuItems", () => {
  it("heads each section and marks loaded local models quietly", () => {
    const items = modelMenuItems("Claude Code", SECTIONS, vi.fn(), t);
    expect(
      items.map((item) =>
        item.type === "heading"
          ? `# ${item.label}`
          : item.type === "action"
            ? [item.label, item.detail].filter(Boolean).join(" / ")
            : item.type,
      ),
    ).toEqual([
      "# Claude Code",
      "Opus 5.5",
      "# Ollama",
      "qwen3:8b / loaded",
      "# LM Studio",
      "Qwen3 8B",
    ]);
  });

  it("starts with the picked model", () => {
    const onPick = vi.fn<(model: SessionModel) => void>();
    const items = modelMenuItems("Claude Code", SECTIONS, onPick, t);
    const qwen = items.find((item) => item.type === "action" && item.label === "Qwen3 8B");
    if (qwen?.type === "action") qwen.run();
    expect(onPick).toHaveBeenCalledWith({ source: "lmstudio", id: "qwen/qwen3-8b" });
    const opus = items.find((item) => item.type === "action" && item.label === "Opus 5.5");
    expect(opus?.type === "action" ? opus.title : null).toBe("Start Claude Code with Opus 5.5");
  });
});

describe("ModelMenuButton", () => {
  let root: Root;
  let host: HTMLDivElement;

  beforeEach(() => {
    host = document.createElement("div");
    host.className = "profile-row";
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

  function render(onPick = vi.fn<(model: SessionModel) => void>(), onAction = vi.fn()) {
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <ModelMenuButton
            program="Claude Code"
            sections={SECTIONS}
            onPick={onPick}
            onAction={onAction}
          />
        </I18nContext.Provider>,
      );
    });
    return { onPick, onAction };
  }

  const button = () => host.querySelector<HTMLButtonElement>(".profile-models");
  const menu = () => document.querySelector<HTMLElement>('[role="menu"]');

  it("names what it does and opens its menu on click", () => {
    render();
    expect(button()?.getAttribute("aria-label")).toBe("Start Claude Code with a model");
    expect(button()?.getAttribute("aria-expanded")).toBe("false");
    act(() => {
      button()?.click();
    });
    expect(menu()).not.toBeNull();
    expect(button()?.getAttribute("aria-expanded")).toBe("true");
  });

  it("opens with Right arrow and returns the focus on Escape", () => {
    render();
    button()?.focus();
    act(() => {
      button()?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    expect(menu()).not.toBeNull();
    act(() => {
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(menu()).toBeNull();
    expect(document.activeElement).toBe(button());
  });

  it("starts the session with the clicked model", () => {
    const { onPick, onAction } = render();
    act(() => {
      button()?.click();
    });
    const qwen = [...document.querySelectorAll<HTMLElement>('[role^="menuitem"]')].find((item) =>
      item.textContent.includes("qwen3:8b"),
    );
    act(() => {
      qwen?.click();
    });
    expect(onPick).toHaveBeenCalledWith({ source: "ollama", id: "qwen3:8b" });
    expect(onAction).toHaveBeenCalledOnce();
    expect(menu()).toBeNull();
  });
});
