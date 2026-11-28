// @vitest-environment happy-dom

// Hangar's bar above an agent terminal: which sessions get it, which facts it leaves out, in which
// order it shows them and in which order they give way, and the two settings that switch it.

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EMPTY_CLAIMS, type Claim, type ClaimsSnapshot, type SessionClaims } from "./claims";
import {
  barSegments,
  changedFiles,
  dropRank,
  DROP_ORDER,
  HOW_SEGMENTS,
  showsHangarBar,
  WHERE_SEGMENTS,
  type BarFacts,
} from "./hangarBarFacts";
import { HangarBarView } from "./HangarBar";
import { createTranslator, I18nContext } from "./i18n";
import { StatusLineGroup } from "./ProgramFilesSettings";
import { defaultSettings, parseSettings, PROTECTED_KEYS, withoutBackendKeys } from "./settings";

const english = createTranslator("en");

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={english}>{element}</I18nContext.Provider>,
  );
}

const NONE: BarFacts = {
  model: null,
  folder: null,
  worktree: false,
  branch: null,
  changes: null,
  context: undefined,
  speed: undefined,
  cost: null,
  runtime: null,
};

const ALL: BarFacts = {
  model: { text: "Opus 5.5", title: "Model: Opus 5.5" },
  folder: "hangar",
  worktree: true,
  branch: "hangar/status-bar",
  changes: "3",
  context: { used: 90_000, window: 200_000 },
  speed: { value: 84, live: true },
  cost: "~$0.42",
  runtime: "12 min",
};

/** The segments in the order the markup draws them. */
function drawn(html: string): string[] {
  return [...html.matchAll(/data-segment="([a-z]+)"/g)].map((match) => match[1] ?? "");
}

describe("which sessions get the bar", () => {
  it("shows for an agent whose program does not draw Hangar's status line", () => {
    expect(showsHangarBar({ enabled: true, agent: true, statusLine: false })).toBe(true);
    // An older backend sends no field: the program draws nothing of Hangar's.
    expect(showsHangarBar({ enabled: true, agent: true, statusLine: undefined })).toBe(true);
  });

  it("stays away where the program draws it, for shells and while the setting is off", () => {
    expect(showsHangarBar({ enabled: true, agent: true, statusLine: true })).toBe(false);
    expect(showsHangarBar({ enabled: true, agent: false, statusLine: false })).toBe(false);
    expect(showsHangarBar({ enabled: false, agent: true, statusLine: false })).toBe(false);
  });
});

describe("the facts", () => {
  it("keeps the where and how groups in their order", () => {
    expect(barSegments(ALL)).toEqual({ where: WHERE_SEGMENTS, how: HOW_SEGMENTS });
    expect(drawn(render(<HangarBarView facts={ALL} language="en" />))).toEqual([
      "model",
      "folder",
      "worktree",
      "branch",
      "changes",
      "context",
      "speed",
      "cost",
      "runtime",
    ]);
  });

  it("leaves out every fact without data", () => {
    const some = { ...NONE, folder: "hangar", cost: "~$0.42" };
    expect(barSegments(some)).toEqual({ where: ["folder"], how: ["cost"] });
    const html = render(<HangarBarView facts={some} language="en" />);
    expect(drawn(html)).toEqual(["folder", "cost"]);
    expect(html).not.toContain('hangar-bar-group how"></span>');
    expect(barSegments({ ...NONE, folder: "", branch: "" })).toEqual({ where: [], how: [] });
  });

  it("draws nothing at all without any fact", () => {
    expect(render(<HangarBarView facts={NONE} language="en" />)).toBe("");
  });

  it("warns from 80 % of the context on", () => {
    const calm = render(<HangarBarView facts={ALL} language="en" />);
    expect(calm).toContain('class="hangar-bar-context"');
    expect(calm).toContain("45%");
    const full = { ...ALL, context: { used: 170_000, window: 200_000 } };
    expect(render(<HangarBarView facts={full} language="en" />)).toContain(
      'class="hangar-bar-context high"',
    );
  });

  it("names the bar and its facts for screen readers", () => {
    const html = render(<HangarBarView facts={ALL} language="en" />);
    expect(html).toContain(`aria-label="${english("hangarBar.label")}"`);
    expect(html).toContain(english("session.branch", { branch: "hangar/status-bar" }));
    expect(html).toContain(english("hangarBar.changed", { count: "3" }));
    expect(html).toContain(english("session.runtime", { duration: "12 min" }));
  });
});

describe("narrow panes", () => {
  it("gives way in a fixed order and keeps the context to the end", () => {
    expect(DROP_ORDER.slice(0, 4)).toEqual(["speed", "runtime", "worktree", "changes"]);
    expect(DROP_ORDER.slice(-2)).toEqual(["model", "context"]);
    expect(dropRank("speed")).toBe(1);
    expect(dropRank("model")).toBe(8);
    expect(dropRank("context")).toBe(0);
    const html = render(<HangarBarView facts={ALL} language="en" />);
    expect(html).toContain('class="hangar-bar-seg seg-speed drop-1"');
    expect(html).toContain('class="hangar-bar-seg seg-context"');
  });

  it("covers every fact once in the drop order", () => {
    expect([...DROP_ORDER].sort()).toEqual([...WHERE_SEGMENTS, ...HOW_SEGMENTS].sort());
  });
});

function claimsOf(entry: Partial<SessionClaims>, enabled = true): ClaimsSnapshot {
  return {
    enabled,
    overlaps: [],
    sessions: [
      {
        sessionId: 7,
        state: { kind: "ready" },
        root: "/repo",
        base: "abc",
        sharedWith: [],
        claims: [],
        truncated: false,
        level: "none",
        overlapCount: 0,
        overlapsTruncated: false,
        ...entry,
      },
    ],
  };
}

const CLAIM: Claim = { path: "a.ts", kind: "modified", wholeFile: false, ranges: [], area: null };

describe("changed files", () => {
  it("counts the session's claimed files", () => {
    expect(changedFiles(claimsOf({ claims: [CLAIM, { ...CLAIM, path: "b.ts" }] }), 7)).toBe("2");
    expect(changedFiles(claimsOf({ claims: [CLAIM], truncated: true }), 7)).toBe("1+");
  });

  it("says nothing without claims, before they are ready or without a change", () => {
    expect(changedFiles(EMPTY_CLAIMS, 7)).toBeNull();
    expect(changedFiles(claimsOf({ claims: [CLAIM] }, false), 7)).toBeNull();
    expect(changedFiles(claimsOf({ claims: [CLAIM], state: { kind: "pending" } }), 7)).toBeNull();
    expect(changedFiles(claimsOf({ claims: [] }), 7)).toBeNull();
    expect(changedFiles(claimsOf({ claims: [CLAIM] }), 8)).toBeNull();
  });
});

describe("settings", () => {
  it("has both switches on by default", () => {
    expect(defaultSettings("en").statusLine).toBe(true);
    expect(defaultSettings("en").hangarBar).toBe(true);
  });

  it("keeps the bar in localStorage and the status line with the backend", () => {
    expect(parseSettings(JSON.stringify({ hangarBar: false }), "en").hangarBar).toBe(false);
    expect(parseSettings(JSON.stringify({ hangarBar: "no" }), "en").hangarBar).toBe(true);
    expect(parseSettings(JSON.stringify({ statusLine: false }), "en").statusLine).toBe(true);
    expect(PROTECTED_KEYS).toContain("statusLine");
    const stored = withoutBackendKeys({ ...defaultSettings("en"), statusLine: false });
    expect(stored).not.toHaveProperty("statusLine");
    expect(stored).toHaveProperty("hangarBar", true);
  });
});

describe("the settings group", () => {
  Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
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

  it("switches each setting on its own", () => {
    const onChange = vi.fn();
    act(() => {
      root.render(
        <I18nContext.Provider value={english}>
          <StatusLineGroup settings={defaultSettings("en")} onChange={onChange} />
        </I18nContext.Provider>,
      );
    });
    const labels = [...host.querySelectorAll(".setting-label")].map((label) => label.textContent);
    expect(labels).toEqual([english("settings.statusLine"), english("settings.hangarBar")]);
    const switches = host.querySelectorAll<HTMLButtonElement>('[role="switch"]');
    expect(switches).toHaveLength(2);
    act(() => {
      switches[0]?.click();
    });
    expect(onChange).toHaveBeenLastCalledWith({ statusLine: false });
    act(() => {
      switches[1]?.click();
    });
    expect(onChange).toHaveBeenLastCalledWith({ hangarBar: false });
  });
});
