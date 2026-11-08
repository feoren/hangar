import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import { installedKey, installedProfiles, sameProfiles } from "./profiles";

function profile(id: string, installed: boolean, extra: Partial<Profile> = {}): Profile {
  return {
    id,
    name: id,
    command: id,
    args: [],
    cwd: null,
    icon: null,
    source: "catalog",
    installed,
    homepage: null,
    installHint: null,
    ...extra,
  };
}

describe("installedProfiles", () => {
  it("shows only installed programs and keeps the order", () => {
    const list = installedProfiles([
      profile("a", true),
      profile("b", false),
      profile("c", true),
      profile("d", false),
    ]);
    expect(list.map((p) => p.id)).toEqual(["a", "c"]);
  });

  it("returns an empty list without installed entries", () => {
    expect(installedProfiles([])).toEqual([]);
    expect(installedProfiles([profile("x", false)])).toEqual([]);
  });
});

describe("installedKey", () => {
  it("depends only on the set of installed IDs", () => {
    const first = installedKey([profile("b", true), profile("a", true), profile("x", false)]);
    const reordered = installedKey([profile("a", true), profile("b", true)]);
    expect(first).toBe(reordered);
  });

  it("changes when a program is added or removed", () => {
    const before = installedKey([profile("a", true), profile("b", false)]);
    expect(installedKey([profile("a", true), profile("b", true)])).not.toBe(before);
    expect(installedKey([profile("a", false), profile("b", false)])).toBe("");
  });
});

describe("sameProfiles", () => {
  it("recognizes equal and changed lists", () => {
    expect(sameProfiles([profile("a", true)], [profile("a", true)])).toBe(true);
    expect(sameProfiles([profile("a", true)], [profile("a", false)])).toBe(false);
    expect(sameProfiles([profile("a", true)], [profile("a", true, { name: "A" })])).toBe(false);
  });
});
