// @vitest-environment happy-dom

// The bar above the work area: the active session's name, the one switch for the right panel and
// the support link, which opens the project's Buy Me a Coffee page through the backend.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { barTitle, MainBar, SUPPORT_URL } from "./MainBar";

const openWebUrl = vi.hoisted(() => vi.fn<(url: string) => Promise<null>>());

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, openWebUrl } };
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  openWebUrl.mockResolvedValue(null);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.clearAllMocks();
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

  it("puts the support link right before the panel switch, which ends the bar", () => {
    render(null, false);
    const coffee = host.querySelector<HTMLButtonElement>(".main-bar-coffee");
    expect(coffee?.textContent).toBe("Buy me a coffee");
    expect(coffee?.nextElementSibling).toBe(toggle());
    expect(toggle()?.nextElementSibling).toBeNull();
    act(() => {
      coffee?.click();
    });
    expect(openWebUrl).toHaveBeenCalledWith(SUPPORT_URL);
  });

  it("points to the project's page over https", () => {
    expect(SUPPORT_URL).toBe("https://buymeacoffee.com/feoren");
  });
});

describe("barTitle", () => {
  it("names the session with its emoji, and nothing without one", () => {
    expect(barTitle({ title: "Webhook retries", marker: { emoji: "🚀" } })).toBe(
      "🚀 Webhook retries",
    );
    expect(barTitle({ title: "Webhook retries", marker: null })).toBe("Webhook retries");
    expect(barTitle(undefined)).toBeNull();
  });
});

describe("MainBar support link", () => {
  it("leaves the support link out for a paid plan", () => {
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <MainBar title={null} sideOpen={false} onToggleSide={vi.fn()} support={false} />
        </I18nContext.Provider>,
      );
    });
    expect(host.querySelector(".main-bar-coffee")).toBeNull();
    expect(host.querySelector(".main-bar-toggle")).not.toBeNull();
  });
});

describe("MainBar local servers", () => {
  const state = {
    ollama: {
      running: true,
      models: [
        { id: "qwen3:8b", label: "qwen3:8b", loaded: true },
        { id: "llama3.2", label: "llama3.2", loaded: false },
      ],
    },
    lmstudio: { running: false, models: [] },
  };

  function renderWith(localLlm: typeof state | null) {
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <MainBar title={null} sideOpen={false} onToggleSide={vi.fn()} localLlm={localLlm} />
        </I18nContext.Provider>,
      );
    });
  }

  const badges = () => [...host.querySelectorAll<HTMLElement>(".local-llm-badge")];

  it("shows one badge per server before the support link, in words as well as color", () => {
    renderWith(state);
    expect(badges().map((badge) => badge.getAttribute("aria-label"))).toEqual([
      "Ollama is running · 2 models",
      "LM Studio is off",
    ]);
    expect(badges().map((badge) => badge.title)).toEqual([
      "Ollama is running · 2 models",
      "LM Studio is off",
    ]);
    expect(badges().map((badge) => badge.classList.contains("running"))).toEqual([true, false]);
    expect(badges()[0]?.getAttribute("role")).toBe("img");
    expect(host.querySelector(".local-llm")?.nextElementSibling).toBe(
      host.querySelector(".main-bar-coffee"),
    );
    // The marks are the bundled paths, drawn in the text color.
    expect(badges()[0]?.querySelector("path")?.getAttribute("d")?.length).toBeGreaterThan(100);
    expect(badges()[0]?.querySelector("svg")?.getAttribute("fill")).toBe("currentColor");
  });

  it("shows no badge while the state is unknown", () => {
    renderWith(null);
    expect(badges()).toHaveLength(0);
  });
});
