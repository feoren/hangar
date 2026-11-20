import { describe, expect, it } from "vitest";

import { hasSessionGpu, platformOf, revealLabelKey, type Platform } from "./platform";

const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/140";
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";

describe("platformOf", () => {
  it("tells the three webviews apart", () => {
    expect(platformOf(WINDOWS)).toBe("windows");
    expect(platformOf(MAC)).toBe("mac");
    expect(platformOf(LINUX)).toBe("linux");
  });
});

describe("only what works is offered", () => {
  const all: Platform[] = ["windows", "mac", "linux"];

  it("gives sessions GPU figures on Windows and Linux, not on macOS", () => {
    expect(all.filter(hasSessionGpu)).toEqual(["windows", "linux"]);
  });

  it("names the file manager of each platform", () => {
    expect(all.map(revealLabelKey)).toEqual([
      "notes.revealExplorer",
      "notes.revealFinder",
      "notes.revealFileManager",
    ]);
  });
});
