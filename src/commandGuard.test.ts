import { describe, expect, it } from "vitest";

import { fewerRules, parseGuardInfo, problemPlace, shippedRules } from "./commandGuard";
import { createTranslator } from "./i18n";

const t = createTranslator("en");

const raw = {
  enabled: true,
  file: "C:/Users/me/AppData/Roaming/hangar/command-guard.toml",
  rules: [{ id: "git-push-force", description: "force push" }],
  problems: [
    { rule: null, id: null, line: null, message: "unknown setting `colour`" },
    { rule: 2, id: "typo", line: null, message: "unknown field `subcomand`" },
    { rule: 3, id: null, line: null, message: "missing field `id`" },
    { rule: null, id: null, line: 5, message: "invalid table header" },
  ],
  defaults: false,
  defaultCount: 8,
};

describe("parseGuardInfo", () => {
  it("takes the backend's summary", () => {
    const info = parseGuardInfo(raw);
    expect(info?.rules).toEqual([{ id: "git-push-force", description: "force push" }]);
    expect(info?.problems).toHaveLength(4);
    expect(info?.enabled).toBe(true);
  });

  it("refuses a malformed summary", () => {
    for (const value of [
      null,
      { ...raw, enabled: "yes" },
      { ...raw, file: 3 },
      { ...raw, rules: [{ id: "", description: "x" }] },
      { ...raw, problems: [{ rule: -1, id: null, line: null, message: "x" }] },
      { ...raw, problems: [{ rule: null, id: 4, line: null, message: "x" }] },
      { ...raw, defaults: undefined },
      { ...raw, defaultCount: -1 },
    ]) {
      expect(parseGuardInfo(value)).toBeNull();
    }
  });
});

describe("fewerRules", () => {
  it("points out a file with fewer rules than Hangar ships, unless the defaults apply anyway", () => {
    const info = parseGuardInfo(raw);
    if (info === null) throw new Error("not parsed");
    expect(fewerRules(info)).toBe(true);
    expect(fewerRules({ ...info, defaultCount: 1 })).toBe(false);
    expect(fewerRules({ ...info, defaults: true })).toBe(false);
    // The user's own rules do not stand in for removed shipped ones.
    const own = Array.from({ length: 8 }, (_, n) => ({ id: `own-${String(n)}`, description: "x" }));
    expect(shippedRules({ ...info, rules: own })).toBe(0);
    expect(fewerRules({ ...info, rules: own })).toBe(true);
    expect(shippedRules({ ...info, rules: [...info.rules, ...own] })).toBe(1);
  });
});

describe("problemPlace", () => {
  it("names the rule, its id or the line", () => {
    const info = parseGuardInfo(raw);
    if (info === null) throw new Error("not parsed");
    expect(info.problems.map((problem) => problemPlace(problem, t))).toEqual([
      null,
      "Rule 2 (typo)",
      "Rule 3",
      "Line 5",
    ]);
  });
});
