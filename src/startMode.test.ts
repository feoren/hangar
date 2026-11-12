import { afterEach, describe, expect, it, vi } from "vitest";

import { createTranslator } from "./i18n";
import {
  DEFAULT_START_MODE,
  effectiveMode,
  loadStartMode,
  parseStartMode,
  saveStartMode,
  isTeamSession,
  sessionMode,
  teamBlock,
  teamUnsupportedHint,
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

  it("knows team capability from the backend's flag and the profile's folder", () => {
    expect(teamCapable({ teamCapable: true, cwd: null })).toBe(true);
    expect(teamCapable({ teamCapable: false, cwd: null })).toBe(false);
    expect(teamCapable({ cwd: null })).toBe(false);
    // A folder fixed in the profile would replace the team session's worktree.
    expect(teamCapable({ teamCapable: true, cwd: "C:\\fixed" })).toBe(false);
    expect(teamBlock({ teamCapable: true, cwd: null })).toBeNull();
    expect(teamBlock({ teamCapable: false, cwd: "C:\\fixed" })).toBe("noHooks");
    expect(teamBlock({ teamCapable: true, cwd: "C:\\fixed" })).toBe("fixedFolder");
  });

  it("says why a program cannot be a team session", () => {
    const t = createTranslator("en");
    expect(teamUnsupportedHint({ name: "Codex", teamCapable: false, cwd: null }, t)).toBe(
      t("start.teamUnsupported", { name: "Codex" }),
    );
    expect(teamUnsupportedHint({ name: "Mine", teamCapable: true, cwd: "C:\\x" }, t)).toBe(
      t("start.teamFixedFolder", { name: "Mine" }),
    );
  });

  it("starts a program as chosen, or not at all as a team session it cannot be", () => {
    const capable = { teamCapable: true, cwd: null };
    expect(startModeFor("private", { cwd: null })).toBe("private");
    expect(startModeFor("private", capable)).toBe("private");
    expect(startModeFor("team", capable)).toBe("team");
    expect(startModeFor("team", { teamCapable: false, cwd: null })).toBeNull();
    expect(startModeFor("team", { cwd: null })).toBeNull();
    expect(startModeFor("team", { teamCapable: true, cwd: "/fixed" })).toBeNull();
  });

  it("restores a worktree session as a team session only for a capable program", () => {
    const capable = { teamCapable: true, cwd: null };
    expect(sessionMode({ worktree }, capable)).toBe("team");
    // Older workspaces may hold worktree sessions of programs without a hook format.
    expect(sessionMode({ worktree }, { teamCapable: false, cwd: null })).toBe("private");
    expect(sessionMode({ worktree }, undefined)).toBe("private");
    expect(sessionMode({ worktree: null }, capable)).toBe("private");
    expect(sessionMode({}, capable)).toBe("private");
  });

  it("marks only sessions the backend runs as team sessions", () => {
    expect(isTeamSession({ mode: "team" })).toBe(true);
    expect(isTeamSession({ mode: "private" })).toBe(false);
    expect(isTeamSession({})).toBe(false);
  });
});
