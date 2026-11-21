// @vitest-environment happy-dom

// Terminals & Agents, the installed list: each row carries one button (its form) and the switch;
// the arrows stay in the row and the Tab order, the icon choice lives at the top of the form.
// Below the list, versions and icons are two plain switches.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Profile } from "./api";
import { badgeColor } from "./format";
import { createTranslator, I18nContext } from "./i18n";
import { DEFAULT_PROGRAM_PREFS } from "./programPrefs";
import { NO_UPDATES } from "./programUpdates";
import { ProgramsSettings } from "./ProgramsSettings";
import { defaultSettings, type Settings } from "./settings";

const saveProfile = vi.fn(() => Promise.resolve([]));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  api: { saveProfile: () => saveProfile() },
}));

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  saveProfile.mockClear();
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

function profile(id: string, name: string): Profile {
  return {
    id,
    name,
    command: id,
    args: [],
    cwd: null,
    icon: null,
    source: "catalog",
    installed: true,
    homepage: null,
    installHint: null,
  };
}

const PROFILES = [profile("claude", "Claude Code"), profile("codex", "Codex CLI")];

function render(
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <ProgramsSettings
          settings={{ ...defaultSettings("en"), ...changes }}
          profiles={PROFILES}
          versions={new Map()}
          programUpdates={{
            updates: NO_UPDATES,
            checking: false,
            error: null,
            checkedAt: null,
            checkNow: () => undefined,
            recheck: () => undefined,
          }}
          updateActions={{
            updating: new Set(),
            confirming: null,
            request: () => undefined,
            confirm: () => undefined,
            cancel: () => undefined,
          }}
          badgeFor={() => ({ kind: "letters", text: "C", color: "#000" })}
          onChange={onChange}
          onProfiles={() => undefined}
        />
      </I18nContext.Provider>,
    );
  });
}

function rows(): HTMLLIElement[] {
  return [...host.querySelectorAll<HTMLLIElement>(".program-item")];
}

function click(element: Element | null | undefined) {
  if (!(element instanceof HTMLElement)) throw new Error("element missing");
  act(() => {
    element.click();
  });
}

function button(text: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((b) => b.textContent === text);
}

describe("ProgramsSettings rows", () => {
  it("give each program one button and the sidebar switch, no separate icon button", () => {
    render();
    expect(rows()).toHaveLength(2);
    for (const row of rows()) {
      const text = [...row.querySelectorAll("button")].filter((b) => b.textContent !== "");
      expect(text.map((b) => b.textContent)).toEqual([t("profile.customize")]);
      const toggle = row.querySelector<HTMLInputElement>('input[role="switch"]');
      expect(toggle?.classList.contains("switch-large")).toBe(true);
      expect(toggle?.checked).toBe(true);
    }
    expect(host.textContent).not.toContain("Change icon");
  });

  it("keep the arrows in the row and the Tab order, only revealed by the style", () => {
    render();
    const [first] = rows();
    const arrows = first?.querySelectorAll<HTMLButtonElement>(".program-move > button");
    expect(arrows).toHaveLength(2);
    for (const arrow of arrows ?? []) {
      expect(arrow.tabIndex).toBe(0);
      expect(arrow.closest("[aria-hidden='true']")).toBeNull();
    }
    expect(arrows?.[0]?.getAttribute("aria-label")).toBe(
      t("settings.moveUp", { name: "Claude Code" }),
    );
  });

  it("move a program down by its arrow and say where it is now", () => {
    const onChange = vi.fn();
    render({}, onChange);
    click(host.querySelector(`[aria-label="${t("settings.moveDown", { name: "Claude Code" })}"]`));
    expect(onChange).toHaveBeenCalledWith({
      programs: { ...DEFAULT_PROGRAM_PREFS, order: ["codex", "claude"] },
    });
    expect(host.querySelector(".sr-only[role='status']")?.textContent).toBe(
      t("settings.moved", { name: "Claude Code", position: 2, count: 2 }),
    );
  });

  it("hide a program from the sidebar with its switch", () => {
    const onChange = vi.fn();
    render({}, onChange);
    const toggle = host.querySelector<HTMLInputElement>(
      `input[aria-label="${t("settings.showInSidebar")}: Codex CLI"]`,
    );
    click(toggle);
    expect(onChange).toHaveBeenCalledWith({
      programs: { ...DEFAULT_PROGRAM_PREFS, hidden: ["codex"] },
    });
  });
});

describe("ProgramsSettings form", () => {
  it("opens with the icon choice on top and the focus in the name", () => {
    render();
    click(button(t("profile.customize")));
    const form = host.querySelector(".profile-editor");
    expect(form).not.toBeNull();
    const icon = form?.querySelector(".profile-icon");
    expect(icon?.querySelector(".profile-label")?.textContent).toBe(t("profile.icon"));
    expect(icon?.querySelector('[role="radiogroup"]')?.getAttribute("aria-label")).toBe(
      t("settings.iconFor", { name: "Claude Code" }),
    );
    // First row of the form's grid, so its label shares the column with the other labels.
    expect(form?.querySelector(".profile-grid")?.firstElementChild).toBe(icon);
    expect(icon?.nextElementSibling?.classList.contains("profile-intro")).toBe(true);
    expect(document.activeElement?.getAttribute("data-field")).toBe("name");
  });

  it("applies an icon at once, and Enter in it does not save the form", () => {
    const onChange = vi.fn();
    render({}, onChange);
    click(button(t("profile.customize")));
    const letters = [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find(
      (input) => input.value === "letters",
    );
    click(letters);
    expect(onChange).toHaveBeenLastCalledWith({
      programs: {
        ...DEFAULT_PROGRAM_PREFS,
        icons: { claude: { kind: "letters", text: "C", color: badgeColor("claude") } },
      },
    });
    const field = host.querySelector<HTMLInputElement>(".letters-input");
    act(() => {
      field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(saveProfile).not.toHaveBeenCalled();
    expect(host.querySelector(".profile-editor")).not.toBeNull();
  });

  it("still saves on Enter in the program's own fields", () => {
    render();
    click(button(t("profile.customize")));
    const name = host.querySelector<HTMLInputElement>('[data-field="name"]');
    act(() => {
      name?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(saveProfile).toHaveBeenCalledTimes(1);
  });

  it("starts the icon choice over after a reset", () => {
    const onChange = vi.fn();
    render({ programs: { ...DEFAULT_PROGRAM_PREFS, icons: {} } }, onChange);
    click(button(t("profile.customize")));
    const letters = () =>
      [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find(
        (input) => input.value === "letters",
      );
    click(letters());
    expect(letters()?.checked).toBe(true);
    click(button(t("settings.resetPrograms")));
    expect(onChange).toHaveBeenLastCalledWith({ programs: DEFAULT_PROGRAM_PREFS });
    expect(letters()?.checked).toBe(false);
    expect(host.querySelector("#reset-programs-help")?.textContent).toBe(
      t("settings.programsResetDone"),
    );
  });
});

describe("ProgramsSettings display", () => {
  it("shows versions and icons as two switches, only versions with a help line", () => {
    const onChange = vi.fn();
    render({ showVersions: true, showIcons: true }, onChange);
    const group = [...host.querySelectorAll(".setting-group")].find(
      (element) =>
        element.querySelector(".setting-group-title")?.textContent === t("settings.display"),
    );
    const labels = [...(group?.querySelectorAll(".setting-label") ?? [])].map((l) => l.textContent);
    expect(labels).toEqual([t("settings.showVersions"), t("settings.showIcons")]);
    const helps = [...(group?.querySelectorAll(".setting-help") ?? [])].map((h) => h.textContent);
    expect(helps).toEqual([t("settings.showVersionsHelp")]);
    const switches = group?.querySelectorAll<HTMLInputElement>('input[role="switch"]');
    click(switches?.[0]);
    expect(onChange).toHaveBeenLastCalledWith({ showVersions: false });
    click(switches?.[1]);
    expect(onChange).toHaveBeenLastCalledWith({ showIcons: false });
  });
});
