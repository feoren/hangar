// @vitest-environment happy-dom

// The one question about the usage statistics: only while unanswered, only after a calm moment,
// two answers alike plus the field list, and gone for good after an answer.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { UsageConsent } from "./usageStats";
import { USAGE_NOTICE_DELAY_MS, UsageStatsNotice } from "./UsageStatsNotice";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let host: HTMLElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.useRealTimers();
});

function render(
  consent: UsageConsent | undefined,
  onAnswer: (enabled: boolean) => void = () => undefined,
  onOpenInfo: () => void = () => undefined,
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <UsageStatsNotice consent={consent} onAnswer={onAnswer} onOpenInfo={onOpenInfo} />
      </I18nContext.Provider>,
    );
  });
}

function wait(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

function button(text: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((element) => element.textContent === text);
}

describe("UsageStatsNotice", () => {
  it("asks only after a calm moment, not with the first window", () => {
    render(null);
    expect(host.textContent).toBe("");
    wait(USAGE_NOTICE_DELAY_MS - 1);
    expect(host.textContent).toBe("");
    wait(1);
    expect(host.textContent).toContain(t("usage.title"));
    expect(host.textContent).toContain(t("usage.text"));
    expect(host.querySelector(".dialog")).toBeNull();
    // Read out politely, like the other cards of the notice stack.
    expect(host.querySelector('[role="status"]')?.getAttribute("aria-labelledby")).toBeTruthy();
  });

  it("never asks once answered or while the answer is unknown", () => {
    for (const consent of [true, false, undefined]) {
      render(consent);
      wait(USAGE_NOTICE_DELAY_MS * 2);
      expect(host.textContent).toBe("");
    }
  });

  it("offers two answers that look alike and the list of what is sent", () => {
    const onOpenInfo = vi.fn();
    render(null, undefined, onOpenInfo);
    wait(USAGE_NOTICE_DELAY_MS);
    const yes = button(t("usage.send"));
    const no = button(t("usage.noThanks"));
    expect(yes?.className).toBe(no?.className);
    act(() => {
      button(t("usage.whatIsSent"))?.click();
    });
    expect(onOpenInfo).toHaveBeenCalledTimes(1);
    // Looking at the list is no answer: the question stays.
    expect(button(t("usage.send"))).toBeDefined();
  });

  it.each([
    [true, "usage.send"],
    [false, "usage.noThanks"],
  ] as const)("takes the answer %s and goes away at once", (enabled, key) => {
    const onAnswer = vi.fn();
    render(null, onAnswer);
    wait(USAGE_NOTICE_DELAY_MS);
    act(() => {
      button(t(key))?.click();
    });
    expect(onAnswer).toHaveBeenCalledExactlyOnceWith(enabled);
    expect(host.textContent).toBe("");
  });

  it("goes away when the switch in Settings answered first", () => {
    render(null);
    wait(USAGE_NOTICE_DELAY_MS);
    expect(host.textContent).not.toBe("");
    render(true);
    expect(host.textContent).toBe("");
  });
});
