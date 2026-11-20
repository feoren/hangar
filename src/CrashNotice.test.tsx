// @vitest-environment happy-dom

// The crash notice goes by itself only when it names no session; one that names sessions stays
// until the person closes it, even when a `resumeFallback` turns a whole restore into one with a
// problem while the timer runs.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CRASH_NOTICE_MS, CrashNotice } from "./CrashNotice";
import type { CrashSummary } from "./crashRestore";
import { createTranslator, I18nContext } from "./i18n";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
const whole: CrashSummary = { restored: 2, resumed: 2, notStarted: [], fellBack: [] };
const withProblem: CrashSummary = { restored: 2, resumed: 1, notStarted: [], fellBack: ["Codex"] };

let host: HTMLElement;
let root: Root | null = null;
let closed = 0;
const onClose = () => {
  closed += 1;
};

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  closed = 0;
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

function show(summary: CrashSummary) {
  act(() => {
    root?.render(
      <I18nContext.Provider value={t}>
        <CrashNotice summary={summary} language="en" onClose={onClose} />
      </I18nContext.Provider>,
    );
  });
}

function wait(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe("CrashNotice", () => {
  it("goes by itself after a whole restore", () => {
    show(whole);
    expect(host.textContent).toContain("2 sessions restored");
    wait(CRASH_NOTICE_MS - 1);
    expect(closed).toBe(0);
    wait(1);
    expect(closed).toBe(1);
  });

  it("stays while it names sessions, also when a fallback arrives during the timer", () => {
    show(whole);
    wait(CRASH_NOTICE_MS / 2);
    show(withProblem);
    expect(host.textContent).toContain("Codex");
    wait(CRASH_NOTICE_MS * 4);
    expect(closed).toBe(0);
  });

  it("closes with its button", () => {
    show(withProblem);
    const button = host.querySelector<HTMLButtonElement>("button");
    expect(button?.getAttribute("aria-label")).toBeTruthy();
    act(() => {
      button?.click();
    });
    expect(closed).toBe(1);
  });
});
