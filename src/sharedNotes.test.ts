import { describe, expect, it } from "vitest";

import { folderId, parseSearchHits, sameFolder } from "./notes";
import { projectFolder, scopeFolders } from "./notesScope";
import {
  asksTeam,
  parseSaveCode,
  parseSharedStatus,
  parseTeamNotes,
  parseUnlock,
  projectNamesOf,
  sharedFolder,
  sharedHints,
  sharedLine,
  sharedTeamLine,
  sharedWritable,
  showsGroups,
  teamProjectName,
  unlockError,
  type LockReason,
  type SharedStatus,
  type TeamNotes,
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
      renewing: false,
      team: null,
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
      renewing: false,
      team: null,
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
      renewing: false,
      team: null,
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

  it("reads the team states of phase 2", () => {
    for (const reason of ["link", "waiting", "oldServer"] as const) {
      expect(parseSharedStatus({ state: "locked", reason })).toEqual({ state: "locked", reason });
    }
    const renewing = parseSharedStatus({ state: "unlocked", renewing: true, resolved: [] });
    expect(renewing).toMatchObject({ renewing: true, team: true });
    const solo = parseSharedStatus({ state: "unlocked", renewing: false, team: false });
    expect(solo).toMatchObject({ renewing: false, team: false });
  });

  it("reads the team of a folder and refuses anything else", () => {
    const team = { project: "p-1", manager: true, speaks: null, linked: true, exists: false };
    expect(parseTeamNotes(team)).toEqual(team);
    expect(parseTeamNotes(null)).toBeNull();
    expect(parseTeamNotes({ ...team, project: "" })).toBeNull();
    expect(parseTeamNotes({ ...team, speaks: "yes" })).toBeNull();
    expect(parseTeamNotes({ ...team, exists: undefined })).toBeNull();
    expect(parseTeamNotes({ ...team, manager: 1 })).toBeNull();
  });

  it("names the team's project, else the folder", () => {
    const names = projectNamesOf([{ id: "p-1", name: "Atlas" }]);
    expect(teamProjectName(teamOf({}), names, "repo")).toBe("Atlas");
    expect(teamProjectName(teamOf({ project: "p-2" }), names, "repo")).toBe("repo");
    expect(teamProjectName(null, names, "repo")).toBe("repo");
    expect(projectNamesOf(null).size).toBe(0);
  });

  it("reads asksTeam and writable", () => {
    expect(asksTeam(null)).toBe(false);
    expect(asksTeam({ state: "unavailable" })).toBe(false);
    expect(asksTeam({ state: "newerFormat" })).toBe(false);
    expect(asksTeam({ state: "off" })).toBe(true);
    expect(asksTeam({ state: "locked", reason: "link" })).toBe(true);
    expect(sharedWritable(unlockedWith({}))).toBe(true);
    expect(sharedWritable(unlockedWith({ renewing: true }))).toBe(false);
    expect(sharedWritable({ state: "off" })).toBe(false);
    expect(sharedWritable(null)).toBe(false);
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

  it("lists only private scopes; the Shared side adds the shared one", () => {
    expect(scopeFolders({ kind: "project" }, project, [])).toEqual([project]);
    expect(scopeFolders({ kind: "general" }, project, [])).toEqual([{ path: null }]);
    expect(scopeFolders({ kind: "all" }, project, [])).toEqual([{ path: null }, project]);
  });

  it("names the project whose shared notes are shown", () => {
    expect(projectFolder({ kind: "project" }, project)).toEqual(project);
    expect(projectFolder({ kind: "all" }, project)).toEqual(project);
    expect(projectFolder({ kind: "general" }, project)).toEqual(project);
    expect(projectFolder({ kind: "project" }, null)).toBeNull();
    expect(projectFolder({ kind: "general" }, null)).toBeNull();
    const other = { path: "D:\\other" };
    expect(projectFolder({ kind: "folder", folder: other }, project)).toEqual(other);
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

function teamOf(patch: Partial<TeamNotes>): TeamNotes {
  return { project: "p-1", manager: true, speaks: true, linked: true, exists: null, ...patch };
}

function unlockedWith(patch: Partial<Extract<SharedStatus, { state: "unlocked" }>>): SharedStatus {
  return {
    state: "unlocked",
    uncommitted: 0,
    ignored: false,
    unreadable: 0,
    resolved: [],
    renewing: false,
    team: false,
    ...patch,
  };
}

// ADR 0006, phase 2, section 8: one line and at most one action per situation.
describe("shared notes of a team", () => {
  const off: SharedStatus = { state: "off" };
  const solo = { text: "sharedNotes.offLine", action: "turnOn" };

  it("lets an owner or admin turn team notes on without a code", () => {
    expect(sharedLine(off, teamOf({}))).toEqual({
      text: "sharedNotes.teamOffLine",
      action: "teamOn",
    });
    expect(sharedTeamLine(off, teamOf({}))).toBeNull();
  });

  it("asks members to find an owner or admin, without a solo setup", () => {
    const ask = { text: "sharedNotes.askManager", action: null };
    expect(sharedLine(off, teamOf({ manager: false }))).toEqual(ask);
    expect(sharedLine(off, teamOf({ manager: false, speaks: null }))).toEqual(ask);
  });

  it("keeps the solo setup on an old team server and tells only managers to update", () => {
    expect(sharedLine(off, teamOf({ speaks: false }))).toEqual(solo);
    expect(sharedTeamLine(off, teamOf({ speaks: false }))).toEqual({
      text: "sharedNotes.updateServer",
      action: null,
    });
    expect(sharedLine(off, teamOf({ manager: false, speaks: false }))).toEqual(solo);
    expect(sharedTeamLine(off, teamOf({ manager: false, speaks: false }))).toBeNull();
  });

  it("keeps the solo setup while the server is not known, and without a linked team", () => {
    expect(sharedLine(off, teamOf({ speaks: null }))).toEqual(solo);
    expect(sharedLine(off, null)).toEqual(solo);
    expect(sharedLine(off, teamOf({ linked: false }))).toEqual(solo);
    expect(sharedTeamLine(off, null)).toBeNull();
  });

  it("says why a team vault is locked, with the one action to link", () => {
    const line = (reason: LockReason) => sharedLine({ state: "locked", reason }, teamOf({}));
    expect(line("link")).toEqual({ text: "sharedNotes.linkLine", action: "link" });
    expect(line("waiting")).toEqual({ text: "sharedNotes.waiting", action: null });
    expect(line("oldServer")).toEqual({ text: "sharedNotes.oldServer", action: null });
    expect(line("team")).toEqual({ text: "sharedNotes.teamLocked", action: null });
  });

  it("offers owners and admins to share a solo vault with the team", () => {
    expect(sharedTeamLine(unlockedWith({}), teamOf({}))).toEqual({
      text: "sharedNotes.shareLine",
      action: "shareTeam",
    });
    expect(sharedTeamLine(unlockedWith({}), teamOf({ speaks: false }))).toEqual({
      text: "sharedNotes.updateServer",
      action: null,
    });
    // Members get nothing; neither does a vault that is a team vault or not known to be solo.
    expect(sharedTeamLine(unlockedWith({}), teamOf({ manager: false }))).toBeNull();
    expect(sharedTeamLine(unlockedWith({ team: true }), teamOf({}))).toBeNull();
    expect(sharedTeamLine(unlockedWith({ team: null }), teamOf({}))).toBeNull();
    expect(sharedTeamLine(unlockedWith({}), teamOf({ speaks: null }))).toBeNull();
    expect(sharedTeamLine(unlockedWith({}), null)).toBeNull();
  });

  it("says that saving is paused while the key is renewed, for everyone", () => {
    const renewing = unlockedWith({ renewing: true, team: true });
    const paused = { text: "sharedNotes.renewing", action: null };
    expect(sharedTeamLine(renewing, teamOf({}))).toEqual(paused);
    expect(sharedTeamLine(renewing, teamOf({ manager: false }))).toEqual(paused);
    expect(sharedTeamLine(renewing, null)).toEqual(paused);
    expect(sharedLine(renewing, teamOf({}))).toBeNull();
  });
});
