import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  defaultTarget,
  projectFolder,
  resolveStartTarget,
  runningProjects,
  startPlaces,
  targetTooltip,
  worktreeChance,
  type ProjectSession,
} from "./startTarget";

const hangar: ProjectSession = { cwd: "C:\\src\\hangar", status: "running" };
const notes: ProjectSession = { cwd: "C:\\src\\notes", status: "running" };
const worktreeSession: ProjectSession = {
  cwd: "C:\\src\\.worktrees\\hangar-1a2b3c",
  status: "running",
  worktree: { repoRoot: "C:\\src\\hangar" },
};
const updateSession: ProjectSession = { cwd: "C:\\Users\\ada", status: "running", kind: "update" };

describe("projectFolder", () => {
  it("uses the repository of a worktree session and skips update sessions", () => {
    expect(projectFolder(hangar)).toBe("C:\\src\\hangar");
    expect(projectFolder(worktreeSession)).toBe("C:\\src\\hangar");
    expect(projectFolder(updateSession)).toBeNull();
    expect(projectFolder({ cwd: " ", status: "running" })).toBeNull();
  });
});

describe("resolveStartTarget", () => {
  const base = { chosen: null, active: undefined, lastUsed: null, home: "C:\\Users\\ada" };

  it("follows the active session", () => {
    expect(resolveStartTarget({ ...base, active: notes, lastUsed: "C:\\src\\hangar" })).toEqual({
      path: "C:\\src\\notes",
      source: "session",
    });
    expect(resolveStartTarget({ ...base, active: worktreeSession }).path).toBe("C:\\src\\hangar");
  });

  it("falls back to the last used folder, then home", () => {
    expect(resolveStartTarget({ ...base, lastUsed: "C:\\src\\hangar" })).toEqual({
      path: "C:\\src\\hangar",
      source: "last",
    });
    expect(resolveStartTarget(base)).toEqual({ path: "C:\\Users\\ada", source: "home" });
    expect(resolveStartTarget({ ...base, home: null })).toEqual({ path: null, source: "home" });
    // An update session has no project: the last folder applies.
    expect(
      resolveStartTarget({ ...base, active: updateSession, lastUsed: "C:\\src\\notes" }).source,
    ).toBe("last");
  });

  it("lets a chosen folder win, unless it is the default anyway", () => {
    expect(resolveStartTarget({ ...base, active: hangar, chosen: "C:\\src\\notes" })).toEqual({
      path: "C:\\src\\notes",
      source: "chosen",
    });
    expect(resolveStartTarget({ ...base, active: hangar, chosen: "c:\\src\\HANGAR\\" })).toEqual({
      path: "C:\\src\\hangar",
      source: "session",
    });
    expect(resolveStartTarget({ ...base, chosen: "" }).source).toBe("home");
  });

  it("exposes the default without the deviation", () => {
    expect(defaultTarget({ active: hangar, lastUsed: null, home: null }).source).toBe("session");
  });
});

describe("runningProjects", () => {
  it("lists running projects once, in order, without exited and update sessions", () => {
    expect(
      runningProjects([
        notes,
        worktreeSession,
        hangar,
        updateSession,
        { cwd: "C:\\src\\old", status: "exited" },
      ]),
    ).toEqual(["C:\\src\\notes", "C:\\src\\hangar"]);
  });
});

describe("startPlaces", () => {
  it("puts the target first and removes duplicates across lists", () => {
    expect(
      startPlaces(
        "C:\\src\\hangar",
        ["C:\\src\\notes", "c:\\src\\hangar"],
        ["C:\\src\\notes\\", "D:\\fav"],
        ["D:\\FAV", "E:\\recent"],
      ),
    ).toEqual(["C:\\src\\hangar", "C:\\src\\notes", "D:\\fav", "E:\\recent"]);
    expect(startPlaces(null, [], [], [])).toEqual([]);
  });
});

describe("worktreeChance", () => {
  it("knows yes, no and pending", () => {
    const branches = new Map<string, string | null>([
      ["C:\\src\\hangar", "main"],
      ["C:\\tmp", null],
    ]);
    expect(worktreeChance(branches, "C:\\src\\hangar")).toBe("yes");
    expect(worktreeChance(branches, "C:\\tmp")).toBe("no");
    expect(worktreeChance(branches, "C:\\other")).toBe("unknown");
    expect(worktreeChance(branches, null)).toBe("no");
  });
});

describe("targetTooltip", () => {
  it("names path and source", () => {
    const t = createTranslator("en");
    expect(targetTooltip({ path: "C:\\src", source: "chosen" }, t)).toBe(
      `C:\\src\n${t("start.sourceChosen")}`,
    );
    expect(targetTooltip({ path: null, source: "home" }, t)).toBe(t("start.sourceHome"));
  });
});
