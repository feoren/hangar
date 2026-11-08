import { describe, expect, it } from "vitest";

import { parseExportResult } from "./api";

describe("parseExportResult", () => {
  it("returns the saved path", () => {
    expect(parseExportResult("C:\\Users\\Ada\\log.txt")).toBe("C:\\Users\\Ada\\log.txt");
  });

  it("treats null as cancelled", () => {
    expect(parseExportResult(null)).toBeNull();
    expect(parseExportResult(undefined)).toBeNull();
  });

  it("rejects anything else", () => {
    expect(() => parseExportResult(42)).toThrow();
    expect(() => parseExportResult("")).toThrow();
    expect(() => parseExportResult({ path: "x" })).toThrow();
  });
});
