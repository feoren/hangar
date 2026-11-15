import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_NOTES_SIDE,
  effectiveSide,
  listedFolders,
  loadNotesSide,
  newNoteTarget,
  onSide,
  parseNotesSide,
  saveNotesSide,
  sideCounts,
  sideFolders,
  sideOf,
  sideSwitchShown,
} from "./notesSide";
import { sharedFolder, type SharedStatus } from "./sharedNotes";

const project = { path: "C:\\work\\repo" };
const shared = sharedFolder(project);
const general = { path: null };
const unlocked: SharedStatus = {
  state: "unlocked",
  uncommitted: 0,
  ignored: false,
  unreadable: 0,
  resolved: [],
};
const locked: SharedStatus = { state: "locked", reason: "needsCode" };

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("notes side storage", () => {
  it("parses only the two sides and defaults to private", () => {
    expect(DEFAULT_NOTES_SIDE).toBe("private");
    expect(parseNotesSide("shared")).toBe("shared");
    expect(parseNotesSide("private")).toBe("private");
    for (const bad of [null, undefined, "", "Shared", 1, {}, "team"]) {
      expect(parseNotesSide(bad)).toBe("private");
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
    expect(loadNotesSide()).toBe("private");
    saveNotesSide("shared");
    expect(loadNotesSide()).toBe("shared");
    store.set("hangar.notesSide", "garbage");
    expect(loadNotesSide()).toBe("private");
  });

  it("falls back to private when storage fails", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    });
    expect(loadNotesSide()).toBe("private");
    expect(() => {
      saveNotesSide("shared");
    }).not.toThrow();
  });
});

describe("notes side switch", () => {
  it("is offered only where shared notes exist as a feature", () => {
    expect(sideSwitchShown(true, unlocked)).toBe(true);
    expect(sideSwitchShown(true, locked)).toBe(true);
    expect(sideSwitchShown(true, { state: "off" })).toBe(true);
    expect(sideSwitchShown(true, { state: "newerFormat" })).toBe(true);
    expect(sideSwitchShown(true, { state: "unavailable" })).toBe(false);
    expect(sideSwitchShown(true, null)).toBe(false);
    expect(sideSwitchShown(false, unlocked)).toBe(false);
  });

  it("keeps a remembered shared side while loading and drops it where it cannot work", () => {
    expect(effectiveSide("private", true, unlocked)).toBe("private");
    expect(effectiveSide("shared", true, unlocked)).toBe("shared");
    expect(effectiveSide("shared", true, locked)).toBe("shared");
    expect(effectiveSide("shared", true, null)).toBe("shared");
    expect(effectiveSide("shared", true, { state: "unavailable" })).toBe("private");
    expect(effectiveSide("shared", false, null)).toBe("private");
  });
});

describe("notes side folders", () => {
  it("lists both sides together and each side alone", () => {
    expect(listedFolders([general, project], shared)).toEqual([general, project, shared]);
    expect(listedFolders([project], null)).toEqual([project]);
    expect(sideFolders("private", [project], shared)).toEqual([project]);
    expect(sideFolders("shared", [project], shared)).toEqual([shared]);
    expect(sideFolders("shared", [project], null)).toEqual([]);
  });

  it("splits and counts notes by side", () => {
    const notes = [{ folder: shared }, { folder: project }, { folder: general }];
    expect(sideOf(shared)).toBe("shared");
    expect(sideOf(project)).toBe("private");
    expect(onSide(notes, "shared")).toEqual([{ folder: shared }]);
    expect(onSide(notes, "private")).toEqual([{ folder: project }, { folder: general }]);
    expect(sideCounts(notes)).toEqual({ private: 2, shared: 1 });
    expect(sideCounts([])).toEqual({ private: 0, shared: 0 });
  });

  it("creates new notes on the chosen side", () => {
    expect(newNoteTarget("private", project, shared)).toEqual(project);
    expect(newNoteTarget("shared", project, shared)).toEqual(shared);
    expect(newNoteTarget("shared", project, null)).toBeNull();
  });
});
