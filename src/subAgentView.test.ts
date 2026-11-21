import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentDef, CodexDef, LibraryEntry } from "./subAgentLibrary";
import { modelName } from "./subAgentModels";
import {
  AGENT_LIST_STORAGE_KEY,
  agentItems,
  DEFAULT_AGENT_LIST_PREFS,
  loadAgentListPrefs,
  parseAgentListPrefs,
  pickSide,
  saveAgentListPrefs,
  sideOf,
  splitBySide,
} from "./subAgentView";

function entry(name: string): LibraryEntry {
  const def: AgentDef = {
    name,
    description: `${name} work`,
    model: null,
    color: null,
    tools: [],
    scope: "global",
    folder: null,
    file: `${name}.md`,
  };
  return { name, def, global: def, projects: [] };
}

function codex(name: string, folder: string | null = null): CodexDef {
  return {
    name,
    description: `${name} work`,
    model: null,
    effort: null,
    scope: folder === null ? "global" : "project",
    folder,
    file: `${name}.toml`,
    managed: false,
    backup: null,
    backups: [],
  };
}

describe("agentItems", () => {
  it("lists Claude Code's definitions first, then the own Codex files, with unique keys", () => {
    const items = agentItems(
      [entry("hangar-backend"), entry("reviewer")],
      [codex("reviewer"), codex("reviewer", "C:/work/app")],
    );
    expect(items.map((item) => [item.kind, item.name])).toEqual([
      ["claude", "hangar-backend"],
      ["claude", "reviewer"],
      ["codex", "reviewer"],
      ["codex", "reviewer"],
    ]);
    expect(new Set(items.map((item) => item.key)).size).toBe(items.length);
  });
});

describe("splitBySide", () => {
  it("puts Hangar's own names on its side and everything else on the other, in order", () => {
    const items = agentItems(
      [entry("hangar-tester"), entry("reviewer"), entry("hangar-backend")],
      [codex("hangar-docs"), codex("writer")],
    );
    const sides = splitBySide(items);
    expect(sides.hangar.map((item) => item.name)).toEqual([
      "hangar-tester",
      "hangar-backend",
      "hangar-docs",
    ]);
    expect(sides.others.map((item) => item.name)).toEqual(["reviewer", "writer"]);
    expect(items.map(sideOf)).toEqual(["hangar", "others", "hangar", "hangar", "others"]);
  });

  it("gives two empty sides for no definitions", () => {
    expect(splitBySide([])).toEqual({ hangar: [], others: [] });
  });
});

describe("pickSide", () => {
  it("starts on Hangar's side when it has entries", () => {
    expect(pickSide(null, { hangar: 12, others: 5 })).toBe("hangar");
  });

  it("starts on the other side when Hangar's is empty", () => {
    expect(pickSide(null, { hangar: 0, others: 3 })).toBe("others");
  });

  it("keeps the side chosen last while it has entries", () => {
    expect(pickSide("others", { hangar: 12, others: 5 })).toBe("others");
  });

  it("leaves a chosen side that ran empty for the side with entries", () => {
    expect(pickSide("hangar", { hangar: 0, others: 2 })).toBe("others");
    expect(pickSide("others", { hangar: 4, others: 0 })).toBe("hangar");
  });

  it("stays on the chosen side, else Hangar's, when both are empty", () => {
    expect(pickSide("others", { hangar: 0, others: 0 })).toBe("others");
    expect(pickSide(null, { hangar: 0, others: 0 })).toBe("hangar");
  });
});

describe("parseAgentListPrefs", () => {
  it("reads a stored side and view", () => {
    expect(parseAgentListPrefs('{"side":"others","view":"table"}')).toEqual({
      side: "others",
      view: "table",
    });
  });

  it("falls back to the defaults for nothing, broken text or unknown values", () => {
    expect(parseAgentListPrefs(null)).toEqual(DEFAULT_AGENT_LIST_PREFS);
    expect(parseAgentListPrefs("{")).toEqual(DEFAULT_AGENT_LIST_PREFS);
    expect(parseAgentListPrefs("42")).toEqual(DEFAULT_AGENT_LIST_PREFS);
    expect(parseAgentListPrefs('{"side":"mine","view":"list"}')).toEqual(DEFAULT_AGENT_LIST_PREFS);
  });

  it("keeps a valid value next to an invalid one", () => {
    expect(parseAgentListPrefs('{"side":"hangar","view":3}')).toEqual({
      side: "hangar",
      view: "grid",
    });
  });
});

describe("stored preferences", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("saves and loads through localStorage", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
    });
    saveAgentListPrefs({ side: "others", view: "table" });
    expect(store.has(AGENT_LIST_STORAGE_KEY)).toBe(true);
    expect(loadAgentListPrefs()).toEqual({ side: "others", view: "table" });
  });

  it("works without storage", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    });
    expect(() => {
      saveAgentListPrefs({ side: "hangar", view: "grid" });
    }).not.toThrow();
    expect(loadAgentListPrefs()).toEqual(DEFAULT_AGENT_LIST_PREFS);
  });
});

describe("modelName", () => {
  it("names Claude Code's aliases and shows any other id as it is", () => {
    expect(modelName("sonnet")).toBe("Sonnet");
    expect(modelName("opus")).toBe("Opus");
    expect(modelName("gpt-5.1-codex")).toBe("gpt-5.1-codex");
  });
});
