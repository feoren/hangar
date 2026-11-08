import { describe, expect, it } from "vitest";

import { availableVersion, parseProgramUpdates, runnableUpdate } from "./programUpdates";

const codex = {
  installed: "0.166.0",
  latest: "0.167.0",
  updateAvailable: true,
  updateHint: "npm install -g @openai/codex@latest",
  canRun: true,
  checkedAt: 1_762_000_000_000,
};

describe("parseProgramUpdates", () => {
  it("takes complete entries", () => {
    const updates = parseProgramUpdates({ codex });
    expect(updates.get("codex")).toEqual(codex);
    expect(availableVersion(updates, "codex")).toBe("0.167.0");
  });

  it("reports no update without a known new version", () => {
    const updates = parseProgramUpdates({
      claude: { ...codex, latest: null },
      gemini: { ...codex, updateAvailable: false },
    });
    expect(updates.get("claude")?.updateAvailable).toBe(false);
    expect(availableVersion(updates, "claude")).toBeNull();
    expect(availableVersion(updates, "gemini")).toBeNull();
    expect(availableVersion(updates, "missing")).toBeNull();
  });

  it("drops implausible entries and foreign responses", () => {
    const updates = parseProgramUpdates({
      a: { ...codex, checkedAt: "yesterday" },
      b: { ...codex, updateAvailable: "yes" },
      c: "nonsense",
      d: { ...codex, latest: "x".repeat(41) },
    });
    expect([...updates.keys()]).toEqual(["d"]);
    expect(updates.get("d")?.updateAvailable).toBe(false);
    expect(parseProgramUpdates(null).size).toBe(0);
    expect(parseProgramUpdates([codex]).size).toBe(0);
  });

  it("runs an update only with an explicit canRun", () => {
    const updates = parseProgramUpdates({
      codex,
      claude: { ...codex, canRun: false },
      alt: { ...codex, canRun: undefined },
      text: { ...codex, canRun: "true" },
      current: { ...codex, updateAvailable: false },
    });
    expect(runnableUpdate(updates, "codex")).toBe(true);
    expect(updates.get("claude")?.canRun).toBe(false);
    expect(runnableUpdate(updates, "claude")).toBe(false);
    // Older backends without the field, and unknown values, count as "copy only".
    expect(updates.get("alt")?.canRun).toBe(false);
    expect(updates.get("text")?.canRun).toBe(false);
    expect(runnableUpdate(updates, "current")).toBe(false);
    expect(runnableUpdate(updates, "missing")).toBe(false);
  });
});
