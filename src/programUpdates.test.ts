import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  availableVersion,
  isPlanChanged,
  offerShowsInstalled,
  outcomeNotice,
  parseProgramUpdates,
  parseUpdateOutcome,
  reasonText,
  runnableUpdate,
  UPDATE_REASONS,
  updateNotes,
  type ProgramUpdate,
} from "./programUpdates";

const codex = {
  installed: "0.166.0",
  latest: "0.167.0",
  updateAvailable: true,
  updateHint: "npm install -g @openai/codex@latest",
  canRun: true,
  checkedAt: 1_762_000_000_000,
  plan: "run",
  reason: null,
  needsAdmin: false,
  command: "npm install -g @openai/codex@latest",
  installer: "npm",
  warnings: ["second-copy"],
  copies: [
    "C:\\npm\\codex.cmd",
    "C:\\Users\\ada\\AppData\\Local\\Programs\\OpenAI\\Codex\\codex.exe",
  ],
  planId: "abc123",
};

const t = createTranslator("en");

describe("parseProgramUpdates", () => {
  it("takes complete entries", () => {
    const updates = parseProgramUpdates({ codex });
    expect(updates.get("codex")).toEqual({
      installed: "0.166.0",
      latest: "0.167.0",
      updateAvailable: true,
      plan: "run",
      reason: null,
      needsAdmin: false,
      command: "npm install -g @openai/codex@latest",
      warnings: ["second-copy"],
      copies: codex.copies,
      planId: "abc123",
      checkedAt: codex.checkedAt,
    });
    expect(availableVersion(updates, "codex")).toBe("0.167.0");
    expect(runnableUpdate(updates, "codex")).toBe(true);
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

  it("reads copy and none plans with their reason", () => {
    const updates = parseProgramUpdates({
      aider: { ...codex, plan: "copy", reason: "tool-missing", command: "uv tool upgrade aider" },
      git: { ...codex, plan: "none", reason: "ambiguous", command: null, updateHint: null },
      odd: { ...codex, plan: "copy", reason: "made-up" },
    });
    expect(updates.get("aider")).toMatchObject({ plan: "copy", reason: "tool-missing" });
    expect(runnableUpdate(updates, "aider")).toBe(false);
    expect(updates.get("git")).toMatchObject({ plan: "none", reason: "ambiguous", command: null });
    expect(updates.get("odd")?.reason).toBeNull();
  });

  it("derives the plan from older backends and never runs without a command", () => {
    const old = { ...codex, plan: undefined, command: undefined, planId: undefined };
    const updates = parseProgramUpdates({
      run: old,
      copy: { ...old, canRun: false },
      text: { ...old, canRun: "true" },
      none: { ...old, canRun: true, updateHint: null },
      empty: { ...codex, plan: "run", command: null, updateHint: null },
    });
    expect(updates.get("run")).toMatchObject({ plan: "run", planId: null });
    expect(updates.get("copy")?.plan).toBe("copy");
    expect(updates.get("text")?.plan).toBe("copy");
    expect(updates.get("none")?.plan).toBe("none");
    expect(updates.get("empty")?.plan).toBe("none");
  });

  it("filters warnings and copies", () => {
    const updates = parseProgramUpdates({
      codex: {
        ...codex,
        warnings: ["running", "running", "other", 3],
        copies: ["C:\\a", "C:\\a", "", "bad\u0007path", 5],
        needsAdmin: "yes",
      },
    });
    expect(updates.get("codex")).toMatchObject({
      warnings: ["running"],
      copies: ["C:\\a"],
      needsAdmin: false,
    });
  });
});

describe("reasonText", () => {
  it("has a text for every reason", () => {
    for (const reason of UPDATE_REASONS) {
      expect(reasonText(reason, t)).not.toBe(reasonText(null, t));
    }
  });
});

describe("updateNotes", () => {
  const base: ProgramUpdate = {
    installed: "7.6.9",
    latest: "7.7.0",
    updateAvailable: true,
    plan: "run",
    reason: null,
    needsAdmin: false,
    command: "winget upgrade Microsoft.PowerShell",
    warnings: [],
    copies: [],
    planId: "plan",
    checkedAt: 0,
  };

  it("has no notes for a plain update", () => {
    expect(updateNotes(base, t)).toEqual([]);
  });

  it("names administrator rights and other copies of a runnable update", () => {
    const update: ProgramUpdate = {
      ...base,
      needsAdmin: true,
      warnings: ["second-copy"],
      copies: ["C:\\a\\pwsh.exe", "C:\\b\\pwsh.exe"],
    };
    expect(updateNotes(update, t)).toEqual([
      t("update.needsAdmin"),
      t("update.copiesFound", { count: 2 }),
    ]);
  });

  it("gives the reason when the update cannot run", () => {
    const update: ProgramUpdate = { ...base, plan: "copy", reason: "no-package", needsAdmin: true };
    expect(updateNotes(update, t)).toEqual([reasonText("no-package", t)]);
  });

  it("does not count a single copy as several", () => {
    const update: ProgramUpdate = {
      ...base,
      warnings: ["second-copy"],
      copies: ["C:\\a\\pwsh.exe"],
    };
    expect(updateNotes(update, t)).toEqual([]);
  });
});

describe("parseUpdateOutcome", () => {
  const payload = {
    profileId: "codex",
    sessionId: 7,
    code: 0,
    before: "0.166.0",
    after: "0.167.0",
    changed: true,
    reason: null,
    copies: [],
  };

  it("reads a complete payload", () => {
    expect(parseUpdateOutcome(payload)).toEqual(payload);
  });

  it("tolerates missing fields and rejects foreign ones", () => {
    expect(parseUpdateOutcome({ profileId: "codex" })).toEqual({
      profileId: "codex",
      sessionId: null,
      code: null,
      before: null,
      after: null,
      changed: null,
      reason: null,
      copies: [],
    });
    expect(parseUpdateOutcome({ ...payload, reason: "boom" })?.reason).toBeNull();
    expect(parseUpdateOutcome({ ...payload, sessionId: 1.5 })?.sessionId).toBeNull();
    expect(parseUpdateOutcome({ sessionId: 7 })).toBeNull();
    expect(parseUpdateOutcome("codex")).toBeNull();
  });
});

describe("outcomeNotice", () => {
  const base = {
    profileId: "codex",
    sessionId: 7,
    code: 0,
    before: "1.0",
    after: "1.1",
    changed: true,
    reason: null,
    copies: [],
  } as const;

  it("names both versions after a change", () => {
    const notice = outcomeNotice(base, "Codex", t);
    expect(notice.tone).toBe("ok");
    expect(notice.message).toContain("1.0");
    expect(notice.message).toContain("1.1");
  });

  it("explains every reason", () => {
    const failed = outcomeNotice({ ...base, reason: "failed", code: 3 }, "Codex", t);
    expect(failed.tone).toBe("problem");
    expect(failed.message).toContain("3");
    const shadowed = outcomeNotice(
      { ...base, reason: "shadowed", after: "1.0", copies: ["C:\\a", "C:\\b"] },
      "Codex",
      t,
    );
    expect(shadowed.paths).toEqual(["C:\\a", "C:\\b"]);
    expect(outcomeNotice({ ...base, reason: "running" }, "Codex", t).tone).toBe("problem");
    expect(outcomeNotice({ ...base, reason: "unchanged" }, "Codex", t).message).toContain("1.1");
    expect(outcomeNotice({ ...base, reason: "failed", code: null }, "Codex", t).tone).toBe(
      "problem",
    );
  });

  it("stays calm without versions to compare", () => {
    expect(outcomeNotice({ ...base, before: null, changed: null }, "Codex", t).tone).toBe("ok");
    expect(
      outcomeNotice({ ...base, before: null, after: null, changed: null }, "Codex", t).message,
    ).toContain("Codex");
  });
});

describe("isPlanChanged", () => {
  it("recognizes the backend's refusal", () => {
    expect(
      isPlanChanged("The update of Codex changed since it was shown; check for updates again"),
    ).toBe(true);
    expect(isPlanChanged("program not found")).toBe(false);
  });
});

describe("offerShowsInstalled", () => {
  const update = parseProgramUpdates({ codex }).get("codex");

  it("is true when the offer shows installed and new version", () => {
    expect(update).toBeDefined();
    expect(offerShowsInstalled(update)).toBe(true);
  });

  it("is false without an offer or when the offer only names the new version", () => {
    expect(offerShowsInstalled(undefined)).toBe(false);
    if (update === undefined) return;
    expect(offerShowsInstalled({ ...update, updateAvailable: false })).toBe(false);
    expect(offerShowsInstalled({ ...update, latest: null })).toBe(false);
    expect(offerShowsInstalled({ ...update, plan: "copy" })).toBe(false);
  });
});
