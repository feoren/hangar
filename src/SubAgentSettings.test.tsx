// @vitest-environment happy-dom

// Settings, Sub-agents: the list first with the new-definition form right below it, then the
// display, the default models and the files Hangar adds, each as a group of settings. Groups for
// programs that are not installed are left out, not greyed out. The library store and the
// backend are stubbed.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings, type Settings } from "./settings";
import { EMPTY_LIBRARY, type Library } from "./subAgentLibrary";
import type { SubAgentSettings as Stored } from "./subAgentModels";

const mocks = vi.hoisted(() => {
  const library: { current: Library | null } = { current: null };
  return {
    library,
    load: vi.fn<() => Promise<unknown>>(),
    save: vi.fn<(settings: unknown) => Promise<void>>(),
  };
});

vi.mock("./api", () => ({
  api: { subAgentSettings: mocks.load, saveSubAgentSettings: mocks.save },
  errorText: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

vi.mock("./subAgentLibraryStore", () => ({
  useSubAgentLibrary: () => ({ library: mocks.library.current, figures: new Map(), error: null }),
  syncLibrary: () => Promise.resolve(),
  refreshLibrary: () => Promise.resolve(),
  applyFigures: () => undefined,
}));

const motion = vi.hoisted(() => {
  const state: { reason: "reducedMotion" | "powerSaving" | null; loops: readonly string[] } = {
    reason: null,
    loops: ["backend-03"],
  };
  return state;
});

vi.mock("./figureFiles", async (importOriginal) => {
  const original = await importOriginal<typeof import("./figureFiles")>();
  return {
    ...original,
    get ANIMATED_FIGURES() {
      return motion.loops;
    },
  };
});

vi.mock("./useFigureMotion", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./useFigureMotion")>()),
  useFigureStillReason: () => motion.reason,
}));

import { SubAgentSettings } from "./SubAgentSettings";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

const installed: Library = {
  ...EMPTY_LIBRARY,
  installed: ["claude", "codex"],
  targets: [{ program: "codex", enabled: true }],
};

beforeEach(() => {
  localStorage.clear();
  mocks.library.current = EMPTY_LIBRARY;
  mocks.load.mockReset();
  mocks.save.mockReset();
  mocks.load.mockResolvedValue({ models: {}, provideOff: [], bundledOff: false });
  mocks.save.mockResolvedValue();
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

async function render(
  library: Library,
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
) {
  mocks.library.current = library;
  await act(async () => {
    root.render(
      <I18nContext.Provider value={t}>
        <SubAgentSettings settings={{ ...defaultSettings("en"), ...changes }} onChange={onChange} />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
}

const titles = () =>
  [...host.querySelectorAll(".setting-group-title")].map((title) => title.textContent);

const button = (text: string) =>
  [...host.querySelectorAll("button")].find((element) => element.textContent === text);

/** The switch named by the row text `label`. */
function switchNamed(label: string): HTMLInputElement {
  const found = [...host.querySelectorAll<HTMLInputElement>('input[role="switch"]')].find(
    (input) =>
      document.getElementById(input.getAttribute("aria-labelledby") ?? "")?.textContent === label,
  );
  if (found === undefined) throw new Error(`no switch ${label}`);
  return found;
}

function follows(first: Element | null | undefined, second: Element | null | undefined): boolean {
  if (first == null || second == null) return false;
  return (first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
}

describe("SubAgentSettings", () => {
  it("shows the list first, the new button below it, then the groups", async () => {
    await render(installed);
    expect(titles()).toEqual([
      t("settings.groupSubAgentDisplay"),
      t("subAgentLib.defaults"),
      t("subAgentLib.files"),
    ]);
    const list = host.querySelector(".sub-agent-browser");
    const newButton = button(t("subAgentLib.new"));
    const display = host.querySelector(".setting-group");
    expect(follows(list, newButton)).toBe(true);
    expect(follows(newButton, display)).toBe(true);
  });

  it("leaves out the model and file groups while no program is installed", async () => {
    await render(EMPTY_LIBRARY);
    expect(titles()).toEqual([t("settings.groupSubAgentDisplay")]);
    expect(host.querySelectorAll('input[role="switch"]')).toHaveLength(2);
  });

  it("shows only the switch for a program that gets copies but no model", async () => {
    await render({
      ...EMPTY_LIBRARY,
      installed: [],
      targets: [{ program: "gemini", enabled: true }],
    });
    expect(titles()).toEqual([t("settings.groupSubAgentDisplay"), t("subAgentLib.files")]);
    const label = t("subAgentLib.provide", { program: "Gemini CLI" });
    expect(switchNamed(label).checked).toBe(true);
    expect(host.textContent).toContain(t("subAgentLib.provideHelpGlobal"));
    expect(host.textContent).not.toContain(t("subAgentLib.provideHelp"));
  });

  it("names each default model row by its program", async () => {
    await render(installed);
    const group = [...host.querySelectorAll(".setting-group")].find(
      (element) =>
        element.querySelector(".setting-group-title")?.textContent === t("subAgentLib.defaults"),
    );
    expect(group?.querySelector(".setting-group-desc")?.textContent).toBe(
      t("subAgentLib.defaultsHelp"),
    );
    const labels = [...(group?.querySelectorAll(".setting-label") ?? [])].map(
      (label) => label.textContent,
    );
    expect(labels).toEqual(["Claude Code", "Codex CLI"]);
    expect(
      group?.querySelector(
        `[aria-label="${t("subAgentLib.defaultFor", { program: "Claude Code" })}"]`,
      ),
    ).not.toBeNull();
  });

  it("wires the display switches to their settings", async () => {
    const patches: Partial<Settings>[] = [];
    await render(installed, { crewColumn: true, subAgentExcerpts: false }, (patch) => {
      patches.push(patch);
    });
    const crew = switchNamed(t("settings.crewColumn"));
    const excerpts = switchNamed(t("settings.subAgentExcerpts"));
    expect(crew.checked).toBe(true);
    expect(excerpts.checked).toBe(false);
    act(() => {
      crew.click();
      excerpts.click();
    });
    expect(patches).toEqual([{ crewColumn: false }, { subAgentExcerpts: true }]);
  });

  /** The segmented "Figures" choice: its radio buttons and its help line. */
  function figureMotionField() {
    const group = [...host.querySelectorAll('[role="radiogroup"]')].find(
      (element) =>
        document.getElementById(element.getAttribute("aria-labelledby") ?? "")?.textContent ===
        t("settings.figureMotion"),
    );
    if (group === undefined) throw new Error("no figure motion field");
    const radios = [...group.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    const help = document.getElementById(group.getAttribute("aria-describedby") ?? "");
    return { radios, help: help?.textContent };
  }

  it("offers figures animated or still, animated by default, and saves the choice", async () => {
    motion.reason = null;
    const patches: Partial<Settings>[] = [];
    await render(installed, {}, (patch) => {
      patches.push(patch);
    });
    const { radios, help } = figureMotionField();
    expect(radios.map((radio) => radio.parentElement?.textContent)).toEqual([
      t("settings.figureMotion.animated"),
      t("settings.figureMotion.still"),
    ]);
    expect(radios.map((radio) => radio.checked)).toEqual([true, false]);
    expect(help).toBe(t("settings.figureMotionHelp"));
    act(() => {
      radios[1]?.click();
    });
    expect(patches).toEqual([{ figureMotion: "still" }]);
  });

  it("says why figures stand still although animated is chosen", async () => {
    motion.reason = "reducedMotion";
    await render(installed);
    expect(figureMotionField().help).toBe(t("settings.figureMotionReduced"));
    motion.reason = "powerSaving";
    await render(installed);
    expect(figureMotionField().help).toBe(t("settings.figureMotionSaving"));
    motion.reason = null;
  });

  it("leaves the choice out while no figure has an idle loop", async () => {
    motion.loops = [];
    await render(installed);
    expect(host.textContent).not.toContain(t("settings.figureMotion.animated"));
    motion.loops = ["backend-03"];
  });

  it("saves the bundled and the Codex switch under their keys", async () => {
    await render(installed);
    const bundled = switchNamed(t("subAgentLib.provideBundled"));
    const codex = switchNamed(t("subAgentLib.provide", { program: "Codex CLI" }));
    expect(bundled.checked).toBe(true);
    expect(codex.checked).toBe(true);
    expect(bundled.getAttribute("aria-describedby")).not.toBeNull();
    await act(async () => {
      bundled.click();
      await Promise.resolve();
    });
    expect(mocks.save).toHaveBeenLastCalledWith<[Stored]>({
      models: {},
      provideOff: [],
      bundledOff: true,
    });
    await act(async () => {
      switchNamed(t("subAgentLib.provide", { program: "Codex CLI" })).click();
      await Promise.resolve();
    });
    expect(mocks.save).toHaveBeenLastCalledWith<[Stored]>({
      models: {},
      provideOff: ["codex"],
      bundledOff: true,
    });
  });

  it("lists what the copy run left alone below the Codex switch, even while it is off", async () => {
    mocks.load.mockResolvedValue({ models: {}, provideOff: ["codex"], bundledOff: false });
    await render({
      ...installed,
      syncIssues: [
        {
          program: "codex",
          scope: "global",
          folder: null,
          file: "review.toml",
          name: "review",
          kind: "io",
        },
      ],
    });
    const codex = switchNamed(t("subAgentLib.provide", { program: "Codex CLI" }));
    expect(codex.checked).toBe(false);
    const issues = codex.closest(".setting")?.querySelector(".sub-agent-issues");
    expect(issues?.textContent).toContain("review.toml");
  });

  it("opens the form with the cursor in it and gives the focus back on cancel", async () => {
    await render(installed);
    act(() => {
      button(t("subAgentLib.new"))?.click();
    });
    expect(titles()[0]).toBe(t("subAgentLib.new"));
    const field = host.querySelector("textarea");
    expect(document.activeElement).toBe(field);
    act(() => {
      button(t("common.cancel"))?.click();
    });
    expect(host.querySelector("textarea")).toBeNull();
    expect(document.activeElement).toBe(button(t("subAgentLib.new")));
  });

  it("offers open and model change only in folders the backend knows", async () => {
    const reviewer = {
      name: "reviewer",
      description: "Reviews changes",
      model: "sonnet",
      color: null,
      tools: [],
      scope: "project" as const,
      folder: "C:\\Work\\Chosen",
      file: "reviewer.md",
    };
    // The second render keeps the details open from the first.
    const openDetails = async (library: Library) => {
      await render(library);
      act(() => {
        [...host.querySelectorAll<HTMLButtonElement>('button[aria-expanded="false"]')]
          .find((element) => element.textContent.includes("reviewer"))
          ?.click();
      });
    };
    const modelField = () =>
      host.querySelector(`[aria-label="${t("subAgentLib.modelOf", { name: "reviewer" })}"]`);
    const hint = t("subAgentLib.startToEdit", { folder: "Chosen" });

    // Chosen for the next start, not started yet: listed, but nothing that would fail.
    await openDetails({ ...installed, defs: [reviewer] });
    expect(button(t("subAgentLib.open"))).toBeUndefined();
    expect(modelField()).toBeNull();
    expect(host.textContent).toContain(hint);

    await openDetails({ ...installed, defs: [reviewer], editable: ["C:\\Work\\Chosen"] });
    expect(button(t("subAgentLib.open"))).toBeDefined();
    expect(modelField()).not.toBeNull();
    expect(host.textContent).not.toContain(hint);
  });

  it("folds the files it could not read under their count", async () => {
    await render({
      ...installed,
      problems: [
        {
          program: "claude",
          scope: "global",
          folder: null,
          file: "broken.md",
          problem: "missingName",
        },
      ],
    });
    const details = host.querySelector("details");
    expect(details?.open).toBe(false);
    expect(details?.querySelector("summary")?.textContent).toBe(
      t("subAgentLib.problems", { count: 1 }),
    );
    expect(details?.textContent).toContain(t("subAgentLib.problem.missingName"));
  });
});
