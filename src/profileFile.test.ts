import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  describeProfileProblem,
  parseProfileProblem,
  profileErrorText,
  shouldReportReloadError,
} from "./profileFile";

const t = createTranslator("de");

describe("parseProfileProblem", () => {
  it("takes a message with a position", () => {
    expect(parseProfileProblem({ message: "invalid string", line: 5, column: 11 })).toEqual({
      message: "invalid string",
      line: 5,
      column: 11,
    });
  });

  it("takes a message without a position", () => {
    expect(parseProfileProblem({ message: "duplicate", line: null, column: null })).toEqual({
      message: "duplicate",
      line: null,
      column: null,
    });
  });

  it("rejects anything else", () => {
    expect(parseProfileProblem("text error")).toBeNull();
    expect(parseProfileProblem({ message: "duplicate" })).toBeNull();
    expect(parseProfileProblem(new TypeError("invoke missing"))).toBeNull();
    expect(parseProfileProblem(null)).toBeNull();
    expect(parseProfileProblem([])).toBeNull();
    expect(parseProfileProblem({ message: 3 })).toBeNull();
    expect(parseProfileProblem({ message: "x", line: 0, column: 1 })).toBeNull();
    expect(parseProfileProblem({ message: "x", line: 1.5, column: 1 })).toBeNull();
    expect(parseProfileProblem({ message: "x", line: "2", column: 1 })).toBeNull();
  });
});

describe("error texts", () => {
  it("name line and column when known", () => {
    const text = describeProfileProblem({ message: "invalid string", line: 5, column: 11 }, t);
    expect(text).toContain("profiles.toml");
    expect(text).toContain("5");
    expect(text).toContain("11");
    expect(text).toContain("invalid string");
  });

  it("work without a position", () => {
    const text = describeProfileProblem({ message: "profile ID 'a'", line: null, column: null }, t);
    expect(text).toContain("profiles.toml");
    expect(text).toContain("profile ID 'a'");
    expect(text).not.toContain("Zeile");
  });

  it("show other errors unchanged", () => {
    expect(profileErrorText("file locked", t)).toBe("file locked");
    expect(profileErrorText(new TypeError("invoke missing"), t)).toBe("invoke missing");
    expect(profileErrorText({ message: "broken", line: 2, column: 1 }, t)).toContain("broken");
  });
});

describe("shouldReportReloadError", () => {
  it("reports an error only once, and again after a success", () => {
    expect(shouldReportReloadError(null, "A")).toBe(true);
    expect(shouldReportReloadError("A", "A")).toBe(false);
    expect(shouldReportReloadError("A", "B")).toBe(true);
  });
});
