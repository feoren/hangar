import { describe, expect, it } from "vitest";

import { folderId, parseSearchHits, sameFolder } from "./notes";
import { groupsScope, projectFolder, scopeFolders } from "./notesScope";
import {
  parseSaveCode,
  parseSharedStatus,
  parseUnlock,
  sharedFolder,
  sharedHints,
  sharedLine,
  showsGroups,
  unlockError,
} from "./sharedNotes";

const project = { path: "C:\\work\\repo" };

describe("shared notes status", () => {
  it("parses every state and refuses anything else", () => {
    expect(parseSharedStatus({ state: "off" })).toEqual({ state: "off" });
    expect(parseSharedStatus({ state: "unavailable" })).toEqual({ state: "unavailable" });
    expect(parseSharedStatus({ state: "newerFormat" })).toEqual({ state: "newerFormat" });
    expect(parseSharedStatus({ state: "locked", reason: "needsCode" })).toEqual({
      state: "locked",
      reason: "needsCode",
    });
    expect(parseSharedStatus({ state: "locked", reason: "other" })).toBeNull();
    expect(
      parseSharedStatus({
        state: "unlocked",
        uncommitted: 2,
        ignored: false,
        unreadable: -1,
        resolved: [{ title: "Plan", copy: "Plan (other version)" }, { copy: "x" }],
      }),
    ).toEqual({
      state: "unlocked",
      uncommitted: 2,
      ignored: false,
      unreadable: 0,
      resolved: [{ title: "Plan", copy: "Plan (other version)" }],
    });
    expect(parseSharedStatus("off")).toBeNull();
    expect(parseSharedStatus({ state: "gone" })).toBeNull();
  });

  it("offers at most one action per state", () => {
    expect(sharedLine({ state: "off" })).toEqual({
      text: "sharedNotes.offLine",
      action: "turnOn",
    });
    expect(sharedLine({ state: "locked", reason: "needsCode" })?.action).toBe("unlock");
    expect(sharedLine({ state: "locked", reason: "team" })?.action).toBeNull();
    expect(sharedLine({ state: "locked", reason: "damaged" })?.action).toBeNull();
    expect(sharedLine({ state: "newerFormat" })?.action).toBeNull();
    const unlocked = {
      state: "unlocked" as const,
      uncommitted: 0,
      ignored: false,
      unreadable: 0,
      resolved: [],
    };
    expect(sharedLine(unlocked)).toBeNull();
    expect(showsGroups(unlocked)).toBe(true);
    expect(showsGroups({ state: "unavailable" })).toBe(false);
    expect(showsGroups(null)).toBe(false);
  });

  it("hints at uncommitted, ignored and unreadable notes", () => {
    const base = {
      state: "unlocked" as const,
      uncommitted: 0,
      ignored: false,
      unreadable: 0,
      resolved: [],
    };
    expect(sharedHints(base)).toEqual([]);
    expect(sharedHints({ ...base, uncommitted: 1 })).toEqual([
      { key: "sharedNotes.uncommittedOne" },
    ]);
    expect(sharedHints({ ...base, uncommitted: 3, unreadable: 2 })).toEqual([
      { key: "sharedNotes.uncommitted", params: { count: 3 } },
      { key: "sharedNotes.unreadable", params: { count: 2 } },
    ]);
    // Ignored wins over "not committed": committing would not help.
    expect(sharedHints({ ...base, uncommitted: 3, ignored: true })).toEqual([
      { key: "sharedNotes.ignored" },
    ]);
    expect(sharedHints({ state: "off" })).toEqual([]);
  });

  it("reads unlock outcomes", () => {
    expect(parseUnlock({ outcome: "typo" })).toEqual({ outcome: "typo" });
    const done = parseUnlock({ outcome: "unlocked", status: { state: "off" } });
    expect(done).toEqual({ outcome: "unlocked", status: { state: "off" } });
    expect(parseUnlock({ outcome: "unlocked" })).toBeNull();
    expect(parseUnlock(null)).toBeNull();
    expect(unlockError({ outcome: "wrongCode" })).toBe("sharedNotes.wrongCode");
    expect(unlockError({ outcome: "teamCode" })).toBe("sharedNotes.teamCode");
    expect(done && unlockError(done)).toBeNull();
  });
});

describe("shared notes scopes", () => {
  it("keeps shared and private notes of one folder apart", () => {
    const shared = sharedFolder(project);
    expect(folderId(shared)).not.toBe(folderId(project));
    expect(sameFolder(shared, project)).toBe(false);
    expect(sameFolder(shared, sharedFolder({ path: "c:/work/repo/" }))).toBe(true);
  });

  it("lists the shared notes next to the project's private notes", () => {
    const shared = sharedFolder(project);
    expect(scopeFolders({ kind: "project" }, project, [], shared)).toEqual([shared, project]);
    expect(scopeFolders({ kind: "project" }, project, [])).toEqual([project]);
    expect(scopeFolders({ kind: "general" }, project, [], shared)).toEqual([{ path: null }]);
    expect(scopeFolders({ kind: "all" }, project, [], shared)).toEqual([
      { path: null },
      project,
      shared,
    ]);
  });

  it("names the project whose shared notes are shown and when to group", () => {
    expect(projectFolder({ kind: "project" }, project)).toEqual(project);
    expect(projectFolder({ kind: "all" }, project)).toEqual(project);
    expect(projectFolder({ kind: "general" }, project)).toBeNull();
    expect(projectFolder({ kind: "project" }, null)).toBeNull();
    const other = { path: "D:\\other" };
    expect(projectFolder({ kind: "folder", folder: other }, project)).toEqual(other);
    expect(groupsScope({ kind: "project" }, project)).toBe(true);
    expect(groupsScope({ kind: "all" }, project)).toBe(false);
    expect(groupsScope({ kind: "project" }, null)).toBe(false);
  });

  it("marks shared search hits", () => {
    const hits = parseSearchHits([
      { folderKey: "general", path: "C:\\work\\repo", id: "a", title: "t", shared: true },
      { folderKey: "general", path: null, id: "b", title: "u" },
    ]);
    expect(hits[0]?.folder).toEqual({ path: "C:\\work\\repo", shared: true });
    expect(hits[1]?.folder).toEqual({ path: null });
  });
});

describe("saving the recovery code", () => {
  it("reads the outcomes", () => {
    expect(parseSaveCode({ outcome: "saved", path: "/home/x.txt" })).toEqual({
      outcome: "saved",
      path: "/home/x.txt",
    });
    expect(parseSaveCode({ outcome: "insideRepository" })).toEqual({
      outcome: "insideRepository",
    });
    expect(parseSaveCode({ outcome: "cancelled" })).toEqual({ outcome: "cancelled" });
    expect(parseSaveCode({ outcome: "saved" })).toBeNull();
    expect(parseSaveCode("x")).toBeNull();
  });
});
