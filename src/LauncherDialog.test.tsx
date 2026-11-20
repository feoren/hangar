// @vitest-environment happy-dom

// Component test for the searchers of quick access (the notes' full text): asked only after a
// pause in typing, and an answer for an older query never shows.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { LauncherDialog, SEARCH_DELAY_MS, type LauncherItem } from "./LauncherDialog";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

function note(label: string): LauncherItem {
  return { key: `note:${label}`, section: "notes", label, found: true, run: () => undefined };
}

async function mount(search: (query: string) => Promise<LauncherItem[]>) {
  root = createRoot(host);
  await act(async () => {
    root?.render(
      <I18nContext.Provider value={t}>
        <LauncherDialog
          buildItems={() => []}
          searchers={[search]}
          onClose={() => undefined}
          onRun={() => undefined}
        />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
}

async function type(text: string) {
  const field = host.querySelector<HTMLInputElement>("[role=combobox]");
  if (!field) throw new Error("no field");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    await Promise.resolve();
  });
}

async function wait(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
    await Promise.resolve();
  });
}

const options = () => [...host.querySelectorAll("[role=option]")].map((o) => o.textContent);

describe("LauncherDialog searchers", () => {
  it("asks once, after the pause in typing, and shows what was found", async () => {
    const search = vi.fn((query: string) => Promise.resolve([note(`hit ${query}`)]));
    await mount(search);
    await type("r");
    await wait(50);
    await type("re");
    await wait(50);
    await type("ref");
    expect(search).not.toHaveBeenCalled();
    await wait(SEARCH_DELAY_MS);
    expect(search.mock.calls).toEqual([["ref"]]);
    expect(options().some((text) => text.includes("hit ref"))).toBe(true);
  });

  it("drops an answer that arrives for an older query", async () => {
    const answers = new Map<string, (items: LauncherItem[]) => void>();
    const search = vi.fn(
      (query: string) =>
        new Promise<LauncherItem[]>((resolve) => {
          answers.set(query, resolve);
        }),
    );
    await mount(search);
    await type("old");
    await wait(SEARCH_DELAY_MS);
    await type("new");
    await wait(SEARCH_DELAY_MS);
    await act(async () => {
      answers.get("old")?.([note("stale hit")]);
      await Promise.resolve();
    });
    expect(options().some((text) => text.includes("stale hit"))).toBe(false);
    await act(async () => {
      answers.get("new")?.([note("fresh hit")]);
      await Promise.resolve();
    });
    expect(options().some((text) => text.includes("fresh hit"))).toBe(true);
  });

  it("keeps the last hits while typing on, until the new answer replaces them", async () => {
    const answers = new Map<string, (items: LauncherItem[]) => void>();
    const search = vi.fn(
      (query: string) =>
        new Promise<LauncherItem[]>((resolve) => {
          answers.set(query, resolve);
        }),
    );
    await mount(search);
    await type("ref");
    await wait(SEARCH_DELAY_MS);
    await act(async () => {
      answers.get("ref")?.([note("first hit")]);
      await Promise.resolve();
    });
    expect(options().some((text) => text.includes("first hit"))).toBe(true);
    await type("refund");
    // Still there while the new search waits for its pause and its answer.
    expect(options().some((text) => text.includes("first hit"))).toBe(true);
    await wait(SEARCH_DELAY_MS);
    expect(options().some((text) => text.includes("first hit"))).toBe(true);
    await act(async () => {
      answers.get("refund")?.([note("second hit")]);
      await Promise.resolve();
    });
    expect(options().some((text) => text.includes("first hit"))).toBe(false);
    expect(options().some((text) => text.includes("second hit"))).toBe(true);
    // Another query that does not grow from it drops the old hits at once.
    await type("x");
    expect(options().some((text) => text.includes("second hit"))).toBe(false);
  });

  it("asks nothing for an empty query", async () => {
    const search = vi.fn(() => Promise.resolve([note("x")]));
    await mount(search);
    await type("   ");
    await wait(SEARCH_DELAY_MS * 2);
    expect(search).not.toHaveBeenCalled();
  });
});
