// @vitest-environment happy-dom

// Terminals & Agents, lower part: the HANGAR.md switch shows what it reaches and opens the file
// only while it is on; the worktree check and the own profiles wait under "Advanced".

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { InstructionsReach, Profile } from "./api";
import { createTranslator, I18nContext } from "./i18n";
import { AgentInstructionsGroup, ProgramsAdvanced } from "./ProgramFilesSettings";
import { defaultSettings, type Settings } from "./settings";

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

function agent(name: string, instructions: InstructionsReach): Profile {
  return {
    id: name.toLowerCase(),
    name,
    command: name,
    args: [],
    cwd: null,
    icon: null,
    source: "catalog",
    installed: true,
    homepage: null,
    installHint: null,
    instructions,
  };
}

const PROFILES = [
  agent("Claude", "yes"),
  agent("Gemini", "yes"),
  agent("Codex", "launcher"),
  agent("PowerShell", "none"),
];

function renderInstructions(
  changes: Partial<Settings>,
  onChange: (patch: Partial<Settings>) => void = () => undefined,
  onOpen: () => Promise<void> = () => Promise.resolve(),
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <AgentInstructionsGroup
          settings={{ ...defaultSettings("en"), ...changes }}
          profiles={PROFILES}
          onChange={onChange}
          onOpen={onOpen}
        />
      </I18nContext.Provider>,
    );
  });
}

function renderAdvanced(
  onChange: (patch: Partial<Settings>) => void = () => undefined,
  onReload: () => Promise<void> = () => Promise.resolve(),
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <ProgramsAdvanced
          settings={defaultSettings("en")}
          onChange={onChange}
          onOpenProfileFile={() => Promise.resolve()}
          onReloadProfiles={onReload}
        />
      </I18nContext.Provider>,
    );
  });
}

function button(text: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((b) => b.textContent === text);
}

async function click(element: HTMLElement | null | undefined) {
  if (!element) throw new Error("element missing");
  await act(async () => {
    element.click();
    await Promise.resolve();
  });
}

describe("AgentInstructionsGroup", () => {
  it("is one switch with its help; the open button waits until it is on", async () => {
    const onChange = vi.fn();
    renderInstructions({ hangarMd: false }, onChange);
    expect(host.querySelector(".setting-label")?.textContent).toBe(t("settings.hangarMd"));
    expect(host.querySelector(".setting-help")?.textContent).toBe(t("settings.hangarMdHelp"));
    expect(button(t("settings.openHangarMd"))).toBeUndefined();
    await click(host.querySelector<HTMLInputElement>('input[role="switch"]'));
    expect(onChange).toHaveBeenCalledWith({ hangarMd: true });
  });

  it("names the programs it reaches and the ones it cannot reach here", () => {
    renderInstructions({ hangarMd: true });
    expect(button(t("settings.openHangarMd"))).toBeDefined();
    const status = host.querySelector(".setting-children [role='status']");
    expect(status?.textContent).toBe(
      `${t("settings.hangarMdReach", { names: "Claude, Gemini" })} ${t("settings.hangarMdUnavailable", { names: "Codex" })}`,
    );
  });

  it("shows why the file did not open in place of that line", async () => {
    renderInstructions({ hangarMd: true }, undefined, () => Promise.reject(new Error("no editor")));
    await click(button(t("settings.openHangarMd")));
    const status = host.querySelector(".setting-children [role='status']");
    expect(status?.textContent).toBe("no editor");
    expect(status?.classList.contains("error-text")).toBe(true);
  });
});

describe("ProgramsAdvanced", () => {
  it("keeps the worktree check and the own profiles closed under Advanced", async () => {
    const onChange = vi.fn();
    renderAdvanced(onChange);
    const details = host.querySelector("details");
    expect(details?.open).toBe(false);
    expect(details?.querySelector("summary")?.textContent).toBe(t("settings.advanced"));
    const labels = [...host.querySelectorAll(".setting-label")].map((l) => l.textContent);
    expect(labels).toEqual([t("settings.worktreeUpstreamCheck"), t("settings.profileFile")]);
    expect(host.textContent).toContain(t("settings.profileFileHelp"));
    await click(host.querySelector<HTMLInputElement>('input[role="switch"]'));
    expect(onChange).toHaveBeenCalledWith({ worktreeUpstreamCheck: false });
  });

  it("reports a reload in place of the help, and a problem as an error", async () => {
    let fail = false;
    renderAdvanced(undefined, () =>
      fail ? Promise.reject(new Error("broken")) : Promise.resolve(),
    );
    await click(button(t("settings.reloadProfiles")));
    const help = () => [...host.querySelectorAll(".setting-help")].at(-1);
    expect(help()?.textContent).toBe(t("settings.profilesReloaded"));
    expect(help()?.getAttribute("role")).toBe("status");
    fail = true;
    await click(button(t("settings.reloadProfiles")));
    expect(help()?.textContent).toBe("broken");
    expect(help()?.classList.contains("error-text")).toBe(true);
  });
});
