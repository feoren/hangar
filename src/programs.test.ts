import { describe, expect, it } from "vitest";

import {
  MAX_VERSION_RETRIES,
  VERSION_RETRY_MS,
  labelWithVersion,
  parseIcons,
  parseVersions,
  retryVersions,
} from "./programs";

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

describe("retryVersions", () => {
  const installed = ["claude", "opencode", "wsl-ubuntu"].join("\n");

  it("asks again while an installed program has no version yet", () => {
    // A query during an install (npm swaps the package, the launcher's binary is not there yet)
    // finds nothing; the next one a minute later finds the version.
    const versions = new Map([["claude", "2.1.330"]]);
    expect(retryVersions(installed, versions, 0)).toBe(true);
    expect(retryVersions(installed, versions, MAX_VERSION_RETRIES - 1)).toBe(true);
  });

  it("stops after a few tries, so a program without a version is not started every minute", () => {
    expect(retryVersions(installed, new Map(), MAX_VERSION_RETRIES)).toBe(false);
  });

  it("does nothing once every installed program has a version", () => {
    const versions = new Map([
      ["claude", "2.1.330"],
      ["opencode", "2.3.2"],
      ["wsl-ubuntu", "26.04"],
      ["gone", "1.0"],
    ]);
    expect(retryVersions(installed, versions, 0)).toBe(false);
    expect(retryVersions("", new Map(), 0)).toBe(false);
  });

  it("waits until the backend forgot the failed query", () => {
    // version.rs keeps a failed query for 60 s (NEGATIVE_TTL); asking earlier finds it again.
    expect(VERSION_RETRY_MS).toBeGreaterThan(60_000);
  });
});
