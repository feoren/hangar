import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_START_MODE,
  effectiveMode,
  loadStartMode,
  parseStartMode,
  saveStartMode,
  sessionMode,
  startModeFor,
  teamCapable,
  teamPossible,
} from "./startMode";

const worktree = { path: "C:\\wt\\a", branch: "hangar/claude-1a2b3c", repoRoot: "C:\\src\\a" };

describe("startMode", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("parses only the two modes and defaults to private", () => {
    expect(DEFAULT_START_MODE).toBe("private");
    expect(parseStartMode("team")).toBe("team");
    expect(parseStartMode("private")).toBe("private");
    for (const bad of [null, undefined, "", "Team", 1, {}, "public"]) {
      expect(parseStartMode(bad)).toBe("private");
    }
  });

  it("remembers the choice", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
    });
    expect(loadStartMode()).toBe("private");
    saveStartMode("team");
    expect(loadStartMode()).toBe("team");
    saveStartMode("private");
    expect(loadStartMode()).toBe("private");
    store.set("hangar.startMode", "garbage");
    expect(loadStartMode()).toBe("private");
  });

  it("starts private when storage fails", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    });
    expect(loadStartMode()).toBe("private");
    expect(() => {
      saveStartMode("team");
    }).not.toThrow();
  });

  it("falls back to private outside a git repository", () => {
    expect(teamPossible("yes")).toBe(true);
    expect(teamPossible("unknown")).toBe(true);
    expect(teamPossible("no")).toBe(false);
    expect(effectiveMode("team", "yes")).toBe("team");
    expect(effectiveMode("team", "unknown")).toBe("team");
    expect(effectiveMode("team", "no")).toBe("private");
    expect(effectiveMode("private", "yes")).toBe("private");
  });

  it("knows team capability only from the backend's flag", () => {
    expect(teamCapable({ teamCapable: true })).toBe(true);
    expect(teamCapable({ teamCapable: false })).toBe(false);
    expect(teamCapable({})).toBe(false);
  });

  it("starts a program as chosen, or not at all as a team session it cannot be", () => {
    expect(startModeFor("private", {})).toBe("private");
    expect(startModeFor("private", { teamCapable: true })).toBe("private");
    expect(startModeFor("team", { teamCapable: true })).toBe("team");
    expect(startModeFor("team", { teamCapable: false })).toBeNull();
    expect(startModeFor("team", {})).toBeNull();
  });

  it("counts every worktree session as a team session", () => {
    expect(sessionMode({ worktree })).toBe("team");
    expect(sessionMode({ worktree: null })).toBe("private");
    expect(sessionMode({})).toBe("private");
  });
});
