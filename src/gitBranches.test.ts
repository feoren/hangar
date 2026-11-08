import { describe, expect, it } from "vitest";

import { branchPaths, MAX_BRANCH_PATHS, parseBranches, sameBranches } from "./gitBranches";

describe("branchPaths", () => {
  it("lists each folder once in sidebar order", () => {
    expect(
      branchPaths([{ cwd: "C:\\b" }, { cwd: "C:\\a" }, { cwd: "C:\\b" }, { cwd: "" }]),
    ).toEqual(["C:\\b", "C:\\a"]);
  });

  it("stops at the backend's limit", () => {
    const sessions = Array.from({ length: 70 }, (_, index) => ({ cwd: `C:\\p${String(index)}` }));
    expect(branchPaths(sessions)).toHaveLength(MAX_BRANCH_PATHS);
  });
});

describe("parseBranches", () => {
  it("keeps branch names and null", () => {
    const branches = parseBranches({ "C:\\a": "main", "C:\\b": null, "C:\\c": "1a2b3c4" });
    expect([...branches]).toEqual([
      ["C:\\a", "main"],
      ["C:\\b", null],
      ["C:\\c", "1a2b3c4"],
    ]);
  });

  it("drops malformed values", () => {
    expect(parseBranches({ a: 3, b: "", c: { x: 1 } }).size).toBe(0);
    expect(parseBranches(null).size).toBe(0);
    expect(parseBranches(["main"]).size).toBe(0);
    expect(parseBranches("main").size).toBe(0);
  });
});

describe("sameBranches", () => {
  it("compares content", () => {
    const a = new Map<string, string | null>([["x", "main"]]);
    expect(sameBranches(a, new Map([["x", "main"]]))).toBe(true);
    expect(sameBranches(a, new Map([["x", "dev"]]))).toBe(false);
    expect(sameBranches(a, new Map([["x", null]]))).toBe(false);
    expect(sameBranches(a, new Map())).toBe(false);
  });
});
