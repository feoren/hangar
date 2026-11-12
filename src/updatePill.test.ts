import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { pillProgress, updatePill, updatePillHint } from "./updatePill";

const de = createTranslator("de");

const update = (version: string) => ({ version });

describe("updatePill", () => {
  it("shows only while an update is available or installing", () => {
    expect(updatePill({ kind: "idle" })).toBeNull();
    expect(updatePill({ kind: "checking" })).toBeNull();
    expect(updatePill({ kind: "current" })).toBeNull();
    expect(updatePill({ kind: "error" })).toBeNull();
    expect(updatePill({ kind: "available", update: update("1.2.0") })).toEqual({
      installing: false,
      version: "1.2.0",
    });
    expect(updatePill({ kind: "installing", update: update("1.2.0"), percent: 40 })).toEqual({
      installing: true,
      version: "1.2.0",
      percent: 40,
    });
  });
});

describe("updatePillHint", () => {
  const ready = { installing: false, version: "1.2.0" } as const;

  it("says what happens to running sessions", () => {
    expect(updatePillHint(ready, 0, true, de)).toBe(de("update.pillHint", { version: "1.2.0" }));
    expect(updatePillHint(ready, 2, true, de)).toBe(
      de("update.pillHintRestores", { version: "1.2.0" }),
    );
    expect(updatePillHint(ready, 2, false, de)).toBe(
      de("update.pillHintEnds", { version: "1.2.0" }),
    );
  });

  it("shows the progress while installing", () => {
    expect(updatePillHint({ installing: true, version: "1.2.0", percent: 40 }, 2, true, de)).toBe(
      de("update.installingProgress", { version: "1.2.0", percent: 40 }),
    );
    expect(updatePillHint({ installing: true, version: "1.2.0", percent: null }, 2, true, de)).toBe(
      de("update.installing", { version: "1.2.0" }),
    );
  });
});

describe("pillProgress", () => {
  it("fills by the known share and shimmers without one", () => {
    expect(pillProgress({ installing: false, version: "1" })).toBeNull();
    expect(pillProgress({ installing: true, version: "1", percent: null })).toBeNull();
    expect(pillProgress({ installing: true, version: "1", percent: 42 })).toBe(42);
    expect(pillProgress({ installing: true, version: "1", percent: 140 })).toBe(100);
  });
});
