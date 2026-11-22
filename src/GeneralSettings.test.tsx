// @vitest-environment happy-dom

// General: the language, what happens on close, and performance and costs. Settings that only
// matter while another one is on show only then, instead of greyed out.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { GeneralSettings } from "./GeneralSettings";
import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings, type Settings } from "./settings";
import type { UsageConsent } from "./usageStats";

// A machine on battery, so the power saving switch shows as well.
vi.mock("./usePower", () => ({ usePowerState: () => "battery" }));

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

interface Usage {
  consent?: UsageConsent | undefined;
  onConsent?: (enabled: boolean) => void;
  onOpenInfo?: () => void;
}

function render(
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
  onClearScrollbacks: () => Promise<void> = () => Promise.resolve(),
  usage: Usage = {},
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <GeneralSettings
          settings={{ ...defaultSettings("en"), ...changes }}
          onChange={onChange}
          onClearScrollbacks={onClearScrollbacks}
          usageConsent={"consent" in usage ? usage.consent : null}
          onUsageConsent={usage.onConsent ?? (() => undefined)}
          onOpenUsageInfo={usage.onOpenInfo ?? (() => undefined)}
        />
      </I18nContext.Provider>,
    );
  });
}

/** The switch a row with this label carries. */
function switchFor(label: string): HTMLInputElement | null {
  const name = [...host.querySelectorAll(".setting-label")].find(
    (element) => element.textContent === label,
  );
  if (name === undefined) return null;
  return host.querySelector<HTMLInputElement>(`input[role="switch"][aria-labelledby="${name.id}"]`);
}

function groupTitles(): string[] {
  return [...host.querySelectorAll(".setting-group-title")].map((h) => h.textContent);
}

describe("GeneralSettings", () => {
  it("groups the settings under 'When closing', 'Performance and costs' and 'Privacy'", () => {
    render();
    expect(groupTitles()).toEqual([
      t("settings.groupOnClose"),
      t("settings.groupPerformance"),
      t("settings.groupPrivacy"),
    ]);
    expect(host.querySelector("#settings-language")).not.toBeNull();
  });

  it("shows the history length and its delete button only while sessions are restored", () => {
    render({ restoreSessions: false });
    expect(switchFor(t("settings.restoreSessions"))?.checked).toBe(false);
    expect(host.querySelector("#scrollback-lines")).toBeNull();
    expect(host.textContent).not.toContain(t("settings.clearScrollbacks"));

    render({ restoreSessions: true });
    expect(host.querySelector("#scrollback-lines")).not.toBeNull();
    expect(host.textContent).toContain(t("settings.clearScrollbacks"));
  });

  it("says in the restore help that the history is unencrypted and can hold passwords", () => {
    expect(t("settings.restoreSessionsHelp")).toContain("unencrypted");
    expect(t("settings.restoreSessionsHelp")).toContain("passwords");
  });

  it("asks in place before deleting the histories and reports the result", async () => {
    const clear = vi.fn(() => Promise.resolve());
    render({ restoreSessions: true }, undefined, clear);
    const button = [...host.querySelectorAll("button")].find(
      (element) => element.textContent === t("settings.clearScrollbacks"),
    );
    act(() => {
      button?.click();
    });
    expect(host.textContent).toContain(t("settings.clearConfirm"));
    expect(document.activeElement?.textContent).toBe(t("common.cancel"));
    const yes = [...host.querySelectorAll("button")].find(
      (element) => element.textContent === t("settings.clearYes"),
    );
    await act(async () => {
      yes?.click();
      await Promise.resolve();
    });
    expect(clear).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="status"]')?.textContent).toBe(
      t("settings.clearScrollbacksDone"),
    );
  });

  it("gives the focus back to the delete button when the question is cancelled", async () => {
    render({ restoreSessions: true });
    const findButton = (text: string) =>
      [...host.querySelectorAll("button")].find((element) => element.textContent === text);
    act(() => {
      findButton(t("settings.clearScrollbacks"))?.click();
    });
    act(() => {
      findButton(t("common.cancel"))?.click();
    });
    await act(async () => {
      await new Promise((resolve) => requestAnimationFrame(resolve));
    });
    expect(host.textContent).not.toContain(t("settings.clearConfirm"));
    expect(document.activeElement).toBe(findButton(t("settings.clearScrollbacks")));
  });

  it("saves a switch at once", () => {
    const onChange = vi.fn();
    render({ keepInTray: false }, onChange);
    act(() => {
      switchFor(t("settings.keepInTray"))?.click();
    });
    expect(onChange).toHaveBeenCalledWith({ keepInTray: true });
  });

  it("offers resources and power saving under performance", () => {
    render();
    expect(switchFor(t("settings.showResources"))).not.toBeNull();
    expect(switchFor(t("settings.powerSaving"))).not.toBeNull();
  });

  it("offers the early warning only with a budget, at 80 % unless chosen otherwise", () => {
    render({ dailyBudget: null });
    expect(host.querySelector("#daily-budget")).not.toBeNull();
    expect(host.querySelector("#budget-early")).toBeNull();
    render({ dailyBudget: 10 });
    expect(host.querySelector<HTMLSelectElement>("#budget-early")?.value).toBe("80");
    render({ dailyBudget: 10, budgetEarlyPercent: null });
    expect(host.querySelector<HTMLSelectElement>("#budget-early")?.value).toBe("off");
  });

  it("lists a stored share that is not one of the choices, so it never reads as off", () => {
    render({ dailyBudget: 10, budgetEarlyPercent: 65 });
    expect(host.querySelector<HTMLSelectElement>("#budget-early")?.value).toBe("65");
  });

  it("offers the morning summary, on by default", () => {
    render();
    expect(switchFor(t("morning.setting"))?.checked).toBe(true);
    render({ morningReview: false });
    expect(switchFor(t("morning.setting"))?.checked).toBe(false);
  });

  it("shows the usage statistics off until the person said yes", () => {
    render({}, undefined, undefined, { consent: null });
    expect(switchFor(t("usage.setting"))?.checked).toBe(false);
    render({}, undefined, undefined, { consent: false });
    expect(switchFor(t("usage.setting"))?.checked).toBe(false);
    render({}, undefined, undefined, { consent: true });
    expect(switchFor(t("usage.setting"))?.checked).toBe(true);
    // While the answer is being read, the switch waits instead of guessing.
    render({}, undefined, undefined, { consent: undefined });
    expect(switchFor(t("usage.setting"))?.disabled).toBe(true);
  });

  it("turns the usage statistics on and off with the switch", () => {
    const onConsent = vi.fn();
    render({}, undefined, undefined, { consent: null, onConsent });
    act(() => {
      switchFor(t("usage.setting"))?.click();
    });
    expect(onConsent).toHaveBeenLastCalledWith(true);
    render({}, undefined, undefined, { consent: true, onConsent });
    act(() => {
      switchFor(t("usage.setting"))?.click();
    });
    expect(onConsent).toHaveBeenLastCalledWith(false);
  });

  it("links the usage statistics to the list of what is sent, without toggling", () => {
    const onConsent = vi.fn();
    const onOpenInfo = vi.fn();
    render({}, undefined, undefined, { consent: false, onConsent, onOpenInfo });
    const link = [...host.querySelectorAll("button")].find(
      (element) => element.textContent === t("usage.whatIsSent"),
    );
    act(() => {
      link?.click();
    });
    expect(onOpenInfo).toHaveBeenCalledTimes(1);
    expect(onConsent).not.toHaveBeenCalled();
    expect(t("usage.settingHelp")).toContain("No project or person names, paths or content");
  });

  it("leaves the paste question and the sub-agent switches to their own sections", () => {
    render();
    expect(switchFor(t("settings.warnMultilinePaste"))).toBeNull();
    expect(switchFor(t("settings.claims"))).toBeNull();
    expect(switchFor(t("settings.subAgentExcerpts"))).toBeNull();
    expect(switchFor(t("settings.crewColumn"))).toBeNull();
  });
});
