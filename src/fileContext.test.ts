import { describe, expect, it } from "vitest";

import { fileOf, forgetFile, rememberFile } from "./fileContext";

describe("fileContext", () => {
  it("keeps the last file per session", () => {
    expect(fileOf(901)).toBeNull();
    rememberFile(901, "src/a.ts");
    rememberFile(901, "src/b.ts");
    rememberFile(902, "README.md");
    rememberFile(902, "  ");
    expect(fileOf(901)).toBe("src/b.ts");
    expect(fileOf(902)).toBe("README.md");
    forgetFile(901);
    expect(fileOf(901)).toBeNull();
  });
});
