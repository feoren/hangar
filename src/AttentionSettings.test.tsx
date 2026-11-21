// @vitest-environment happy-dom

// Notifications: the master switch, then what happens when a session waits, quiet hours, the push
// to the phone, the look of each state and the timings under "Advanced". Settings that only matter
// while another one is on show only then, instead of greyed out.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AttentionSettings } from "./AttentionSettings";
import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings, type Settings } from "./settings";
import { DEFAULT_LOOKS } from "./stateLook";

const push = vi.hoisted(() => ({
  settings: vi.fn<() => Promise<unknown>>(),
  save: vi.fn<(input: unknown) => Promise<unknown>>(),
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, pushSettings: push.settings, pushSave: push.save },
  };
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/140";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15";

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  push.settings.mockResolvedValue({ provider: "off" });
  push.save.mockImplementation((input) => Promise.resolve(input));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

async function render(
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
) {
  await act(async () => {
    root.render(
      <I18nContext.Provider value={t}>
        <AttentionSettings
          settings={{ ...defaultSettings("en"), ...changes }}
          theme="dark"
          onChange={onChange}
        />
      </I18nContext.Provider>,
    );
    // The push settings load from the backend.
    await Promise.resolve();
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

function button(text: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((element) => element.textContent === text);
}

function groupTitles(): string[] {
  return [...host.querySelectorAll(".setting-group-title")].map((h) => h.textContent);
}

/** The line of a state in "Colors and pulse", which opens its editor. */
function lookRow(state: string): HTMLButtonElement | null {
  return host.querySelector<HTMLButtonElement>(`#look-${state}-toggle`);
}

describe("AttentionSettings", () => {
  it("groups the notices under their own titles while highlighting is on", async () => {
    await render();
    expect(switchFor(t("settings.attentionEnabled"))?.checked).toBe(true);
    expect(groupTitles()).toEqual([
      t("settings.groupWaiting"),
      t("settings.groupQuietHours"),
      t("push.heading"),
      t("look.heading"),
    ]);
    expect(host.querySelector(".setting-advanced")).not.toBeNull();
  });

  it("shows only quiet hours and the looks of a resting and a failed session while off", async () => {
    await render({ attentionEnabled: false });
    expect(groupTitles()).toEqual([t("settings.groupQuietHours"), t("look.heading")]);
    expect(switchFor(t("settings.taskbarFlash"))).toBeNull();
    expect(host.querySelector(".setting-advanced")).toBeNull();
    expect([...host.querySelectorAll(".look-row")].map((row) => row.id)).toEqual([
      "look-idle-toggle",
      "look-failed-toggle",
    ]);
  });

  it("saves the master switch at once", async () => {
    const onChange = vi.fn();
    await render({}, onChange);
    act(() => {
      switchFor(t("settings.attentionEnabled"))?.click();
    });
    expect(onChange).toHaveBeenCalledWith({ attentionEnabled: false });
  });

  it.each([
    ["Windows", WINDOWS],
    ["Linux", LINUX],
    ["macOS", MAC],
  ])("offers desktop notifications on %s", async (_name, userAgent) => {
    vi.stubGlobal("navigator", { userAgent });
    await render();
    expect(switchFor(t("settings.notifyDesktop"))).not.toBeNull();
  });

  it("shows the volume and its preview only while the sound is on", async () => {
    await render({ chime: false });
    expect(switchFor(t("settings.chime"))?.checked).toBe(false);
    expect(host.querySelector("#chime-volume")).toBeNull();
    expect(button(t("settings.chimePreview"))).toBeUndefined();

    await render({ chime: true });
    expect(host.querySelector("#chime-volume")).not.toBeNull();
    expect(button(t("settings.chimePreview"))).not.toBeUndefined();
  });

  it("shows the quiet hours' times in one line only while quiet hours are on", async () => {
    await render({ quietHours: { enabled: false, from: 22 * 60, to: 7 * 60 } });
    expect(host.querySelectorAll('input[type="time"]')).toHaveLength(0);

    await render({ quietHours: { enabled: true, from: 22 * 60, to: 7 * 60 } });
    const times = host.querySelectorAll('input[type="time"]');
    expect(times).toHaveLength(2);
    expect(times[0]?.closest(".setting-line")).toBe(times[1]?.closest(".setting-line"));
    expect(host.textContent).not.toContain(t("settings.quietHoursEmpty"));
  });

  it("keeps the quiet hours usable while highlighting is off, for the budget warning", async () => {
    const onChange = vi.fn();
    const quietHours = { enabled: true, from: 22 * 60, to: 7 * 60 };
    await render({ attentionEnabled: false, quietHours }, onChange);
    expect(switchFor(t("settings.quietHours"))?.checked).toBe(true);
    const from = host.querySelector<HTMLInputElement>('input[type="time"]');
    expect(from?.disabled).toBe(false);
    act(() => {
      if (from === null) return;
      // React tracks the value it set; the prototype's setter makes the change visible to it.
      Reflect.set(HTMLInputElement.prototype, "value", "21:30", from);
      from.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(onChange).toHaveBeenCalledWith({ quietHours: { ...quietHours, from: 21 * 60 + 30 } });
    expect(t("settings.quietHoursHelp")).toContain("no budget warning");
  });

  it("says when the quiet hours start and end at the same time", async () => {
    await render({ quietHours: { enabled: true, from: 60, to: 60 } });
    expect(host.textContent).toContain(t("settings.quietHoursEmpty"));
  });

  it("names the push contents and never terminal content", () => {
    expect(t("push.help")).toContain("program, number and state, never terminal content");
  });

  it("leaves resources, budget and the morning summary to General", async () => {
    await render();
    expect(host.textContent).not.toContain(t("settings.showResources"));
    expect(host.querySelector("#daily-budget")).toBeNull();
    expect(host.textContent).not.toContain(t("morning.setting"));
  });
});

describe("AttentionSettings colors and pulse", () => {
  it("lists every state collapsed, each with its color and a live preview", async () => {
    await render();
    const rows = [...host.querySelectorAll<HTMLButtonElement>(".look-row")];
    expect(rows).toHaveLength(5);
    for (const row of rows) {
      expect(row.getAttribute("aria-expanded")).toBe("false");
      expect(row.querySelector(".look-dot")).not.toBeNull();
      expect(row.querySelector(".look-preview .session")).not.toBeNull();
    }
    expect(host.querySelector(".look-editor")).toBeNull();
  });

  it("opens one state's editor at a time", async () => {
    await render();
    act(() => {
      lookRow("done")?.click();
    });
    expect(lookRow("done")?.getAttribute("aria-expanded")).toBe("true");
    const editor = host.querySelector("#look-done-editor");
    expect(editor).not.toBeNull();
    expect(lookRow("done")?.getAttribute("aria-controls")).toBe("look-done-editor");
    expect(editor?.querySelectorAll('input[type="radio"]').length).toBeGreaterThan(4);
    expect(editor?.querySelector('input[type="color"]')).not.toBeNull();

    act(() => {
      lookRow("asks")?.click();
    });
    expect(host.querySelector("#look-done-editor")).toBeNull();
    expect(host.querySelector("#look-asks-editor")).not.toBeNull();

    act(() => {
      lookRow("asks")?.click();
    });
    expect(host.querySelector(".look-editor")).toBeNull();
  });

  it("shows the pulse duration only while the state pulses", async () => {
    await render();
    act(() => {
      lookRow("working")?.click();
    });
    expect(DEFAULT_LOOKS.working.pulse).toBe("off");
    expect(host.querySelector("#look-working-seconds")).toBeNull();
    act(() => {
      lookRow("done")?.click();
    });
    expect(host.querySelector("#look-done-seconds")).not.toBeNull();
  });

  it("offers the dot motion only for the working state", async () => {
    await render();
    act(() => {
      lookRow("done")?.click();
    });
    expect(host.textContent).not.toContain(t("look.dots"));
    act(() => {
      lookRow("working")?.click();
    });
    expect(host.textContent).toContain(t("look.dots"));
  });

  it("offers a reset only for a changed look and saves the default", async () => {
    const onChange = vi.fn();
    await render({}, onChange);
    act(() => {
      lookRow("done")?.click();
    });
    expect(button(t("look.reset"))).toBeUndefined();

    const changed = { ...DEFAULT_LOOKS, done: { ...DEFAULT_LOOKS.done, color: "#123456" } };
    await render({ looks: changed }, onChange);
    act(() => {
      button(t("look.reset"))?.click();
    });
    expect(onChange).toHaveBeenCalledWith({ looks: DEFAULT_LOOKS });
  });
});

describe("AttentionSettings timings", () => {
  it("keeps the three timings under a closed 'Advanced', the wait at ten minutes", async () => {
    await render();
    const advanced = host.querySelector<HTMLDetailsElement>(".setting-advanced");
    expect(advanced?.open).toBe(false);
    for (const id of ["quiet-seconds", "stall-minutes", "wait-minutes"]) {
      const slider = advanced?.querySelector(`#${id}`);
      expect(slider, id).not.toBeNull();
      expect(slider?.getAttribute("aria-describedby")).toBe(`${id}-help`);
    }
    expect(advanced?.textContent).toContain(t("settings.waitTime"));
    expect(advanced?.textContent).toContain(t("settings.minutes", { count: 10 }));
  });

  it("names the timings as nouns, not as half sentences", () => {
    const de = createTranslator("de");
    expect(de("settings.quietTime")).toBe("Hinweis nach Ruhe");
    expect(de("settings.stallTime")).toBe("Als hängend zeigen nach");
    expect(de("settings.waitTime")).toBe("Erinnern nach");
  });
});

describe("AttentionSettings phone push", () => {
  it("offers only the service while the push is off and unchanged", async () => {
    await render();
    expect(host.querySelector("#push-ntfy-url")).toBeNull();
    expect(switchFor(t("push.eventWaiting"))).toBeNull();
    expect(button(t("push.test"))).toBeUndefined();
    expect(button(t("push.save"))).toBeUndefined();
  });

  it("offers a save for switching a stored push off", async () => {
    push.settings.mockResolvedValue({
      provider: "ntfy",
      ntfyUrl: "https://ntfy.sh/a1b2c3d4e5f6g7h8",
    });
    await render();
    const off = host.querySelector<HTMLInputElement>('input[type="radio"][value="off"]');
    act(() => {
      off?.click();
    });
    expect(button(t("push.save"))?.disabled).toBe(false);
    expect(host.textContent).toContain(t("push.unsaved"));
  });

  it("says 'not saved yet' again once a saved form changes", async () => {
    await render();
    act(() => {
      host.querySelector<HTMLInputElement>('input[type="radio"][value="pushover"]')?.click();
    });
    await act(async () => {
      button(t("push.save"))?.click();
      await Promise.resolve();
    });
    expect(host.textContent).toContain(t("push.saved"));
    act(() => {
      switchFor(t("push.eventDone"))?.click();
    });
    expect(host.textContent).not.toContain(t("push.saved"));
    expect(host.textContent).toContain(t("push.unsaved"));
  });

  it("asks for the address, the token and the events once ntfy is chosen", async () => {
    await render();
    const ntfy = host.querySelector<HTMLInputElement>('input[type="radio"][value="ntfy"]');
    act(() => {
      ntfy?.click();
    });
    expect(host.querySelector("#push-ntfy-url")).not.toBeNull();
    expect(host.querySelector("#push-ntfyToken")).not.toBeNull();
    for (const key of ["push.eventWaiting", "push.eventDone", "push.eventFailed"] as const) {
      expect(switchFor(t(key))?.checked, key).toBe(true);
    }
    expect(switchFor(t("push.includeTitle"))?.checked).toBe(false);
    expect(button(t("push.test"))).not.toBeUndefined();
    expect(host.textContent).toContain(t("push.unsaved"));
  });

  it("keeps an explicit save and reports it", async () => {
    await render();
    const pushover = host.querySelector<HTMLInputElement>('input[type="radio"][value="pushover"]');
    act(() => {
      pushover?.click();
    });
    act(() => {
      switchFor(t("push.eventDone"))?.click();
    });
    expect(push.save).not.toHaveBeenCalled();
    await act(async () => {
      button(t("push.save"))?.click();
      await Promise.resolve();
    });
    expect(push.save).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "pushover",
        events: { waiting: true, done: false, failed: true },
      }),
    );
    expect(host.textContent).toContain(t("push.saved"));
  });
});
