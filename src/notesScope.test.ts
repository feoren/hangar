import { describe, expect, it } from "vitest";

import {
  effectiveScope,
  folderKeyOf,
  mixesScopes,
  newNoteFolder,
  sameScope,
  scopeFolders,
  scopeForFolder,
} from "./notesScope";

const shop = { path: "C:\\p\\shop" };
const web = { path: "C:\\p\\web" };
const folders = [
  { folderKey: "general", path: null, noteCount: 0, updatedAt: null },
  { folderKey: "a".repeat(32), path: "C:\\p\\web", noteCount: 2, updatedAt: 1 },
  { folderKey: "b".repeat(32), path: "C:\\p\\old", noteCount: 0, updatedAt: 1 },
];

describe("notes scope", () => {
  it("shows General for This project without a session", () => {
    expect(effectiveScope({ kind: "project" }, null)).toEqual({ kind: "general" });
    expect(effectiveScope({ kind: "project" }, shop)).toEqual({ kind: "project" });
    expect(effectiveScope({ kind: "folder", folder: { path: "c:/p/shop" } }, shop)).toEqual({
      kind: "project",
    });
  });

  it("lists the right scopes", () => {
    expect(scopeFolders({ kind: "project" }, shop, folders)).toEqual([shop]);
    expect(scopeFolders({ kind: "general" }, shop, folders)).toEqual([{ path: null }]);
    expect(scopeFolders({ kind: "all" }, shop, folders)).toEqual([{ path: null }, web, shop]);
    expect(scopeFolders({ kind: "all" }, web, folders)).toEqual([{ path: null }, web]);
  });

  it("puts new notes into the shown scope, for All into the project", () => {
    expect(newNoteFolder({ kind: "all" }, shop)).toEqual(shop);
    expect(newNoteFolder({ kind: "all" }, null)).toEqual({ path: null });
    expect(newNoteFolder({ kind: "general" }, shop)).toEqual({ path: null });
    expect(newNoteFolder({ kind: "folder", folder: web }, shop)).toEqual(web);
  });

  it("maps a folder to its scope", () => {
    expect(scopeForFolder({ path: null }, shop)).toEqual({ kind: "general" });
    expect(scopeForFolder(shop, shop)).toEqual({ kind: "project" });
    expect(scopeForFolder(web, shop)).toEqual({ kind: "folder", folder: web });
  });

  it("compares scopes and knows when rows name their project", () => {
    expect(
      sameScope({ kind: "folder", folder: web }, { kind: "folder", folder: { path: "c:/p/web" } }),
    ).toBe(true);
    expect(sameScope({ kind: "all" }, { kind: "general" })).toBe(false);
    expect(mixesScopes({ kind: "all" }, null)).toBe(true);
    expect(mixesScopes({ kind: "project" }, shop)).toBe(false);
  });

  it("finds the backend key of a scope", () => {
    expect(folderKeyOf({ path: null }, [])).toBe("general");
    expect(folderKeyOf({ path: "c:/p/web" }, folders)).toBe("a".repeat(32));
    expect(folderKeyOf(shop, folders)).toBeNull();
  });
});
