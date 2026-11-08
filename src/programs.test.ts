import { describe, expect, it } from "vitest";

import { labelWithVersion, parseIcons, parseVersions } from "./programs";

const PNG = "data:image/png;base64,iVBORw0KGgo=";

describe("parseVersions", () => {
  it("takes texts and leaves out null", () => {
    const versions = parseVersions({ pwsh: "7.6.9", cmd: null, claude: " 2.1.0 " });
    expect([...versions]).toEqual([
      ["pwsh", "7.6.9"],
      ["claude", "2.1.0"],
    ]);
  });

  it("drops wrong types, empty and overlong texts", () => {
    const versions = parseVersions({ a: 7, b: "", c: "   ", d: "x".repeat(41), e: ["1"] });
    expect(versions.size).toBe(0);
  });

  it("survives responses that are not an object", () => {
    for (const value of [null, undefined, "7.6.9", 42, ["7.6.9"]]) {
      expect(parseVersions(value).size).toBe(0);
    }
  });
});

describe("parseIcons", () => {
  it("takes only PNG data URLs", () => {
    const icons = parseIcons({
      ok: PNG,
      none: null,
      remote: "https://example.org/icon.png",
      svg: "data:image/svg+xml;base64,PHN2Zz4=",
      broken: "data:image/png;base64,<script>",
      number: 1,
    });
    expect([...icons]).toEqual([["ok", PNG]]);
  });

  it("survives responses that are not an object", () => {
    expect(parseIcons(null).size).toBe(0);
    expect(parseIcons([PNG]).size).toBe(0);
  });
});

describe("labelWithVersion", () => {
  it("appends the version with a dot", () => {
    expect(labelWithVersion("PowerShell 7", "7.6.9")).toBe("PowerShell 7 · 7.6.9");
  });

  it("shows only the name without a version", () => {
    expect(labelWithVersion("PowerShell 7", undefined)).toBe("PowerShell 7");
  });
});
