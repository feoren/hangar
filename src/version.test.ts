import { describe, expect, it } from "vitest";

import { displayVersion, formatVersion, parseVersion, versionParts, withBuild } from "./version";

describe("parseVersion", () => {
  it("reads the internal form with build metadata", () => {
    expect(parseVersion("0.12.224+1548")).toEqual({
      major: 0,
      minor: 12,
      revision: 224,
      build: 1548,
    });
  });

  it("reads the display form and versions without a build", () => {
    expect(parseVersion("0.12.224-1548")?.build).toBe(1548);
    expect(parseVersion("v1.2.3")).toEqual({ major: 1, minor: 2, revision: 3, build: null });
  });

  it("rejects anything else", () => {
    expect(parseVersion("1.2")).toBeNull();
    expect(parseVersion("1.2.3-beta")).toBeNull();
    expect(parseVersion("")).toBeNull();
  });
});

describe("formatVersion", () => {
  it("shows major.minor.revision-build", () => {
    expect(formatVersion("0.12.224+1548")).toBe("0.12.224-1548");
    expect(formatVersion("0.12.224-1548")).toBe("0.12.224-1548");
  });

  it("shows only three parts without a build and keeps unreadable input unchanged", () => {
    expect(formatVersion("0.1.0")).toBe("0.1.0");
    expect(formatVersion("unknown")).toBe("unknown");
  });
});

describe("displayVersion", () => {
  it("appends the beta suffix for major version 0, with and without a build", () => {
    expect(displayVersion("0.12.125+5655", "beta")).toBe("0.12.125-5655 beta");
    expect(displayVersion("0.12.125", "beta")).toBe("0.12.125 beta");
  });

  it("omits the suffix from 1.0.0 on", () => {
    expect(displayVersion("1.0.0", "beta")).toBe("1.0.0");
    expect(displayVersion("2.3.4+77", "beta")).toBe("2.3.4-77");
  });

  it("keeps unreadable input unchanged", () => {
    expect(displayVersion("nightly", "beta")).toBe("nightly");
    expect(displayVersion("", "beta")).toBe("");
  });
});

describe("withBuild", () => {
  it("adds the build number to dev builds without one", () => {
    expect(withBuild("0.1.0", 1245)).toBe("0.1.0+1245");
    expect(formatVersion(withBuild("0.1.0", 1245))).toBe("0.1.0-1245");
  });

  it("keeps an existing build number", () => {
    expect(withBuild("0.12.125+5655", 1245)).toBe("0.12.125+5655");
  });

  it("changes nothing without a build number or for unreadable input", () => {
    expect(withBuild("0.1.0", null)).toBe("0.1.0");
    expect(withBuild("nightly", 7)).toBe("nightly");
  });
});

describe("versionParts", () => {
  it("separates the number and the beta flag", () => {
    expect(versionParts("0.12.125+5655")).toEqual({ number: "0.12.125-5655", beta: true });
    expect(versionParts("1.0.0+9")).toEqual({ number: "1.0.0-9", beta: false });
  });

  it("shows unreadable input without beta", () => {
    expect(versionParts("nightly")).toEqual({ number: "nightly", beta: false });
  });
});
