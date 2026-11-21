import { describe, expect, it } from "vitest";

import { formatShortTime } from "./format";
import { createTranslator } from "./i18n";
import { hangarUpdateStatus, programUpdateStatus } from "./updateStatus";

const de = createTranslator("de");

describe("hangarUpdateStatus", () => {
  const checkedAt = Date.UTC(2026, 10, 21, 9, 30);

  it("stays empty while idle before the first successful check", () => {
    expect(hangarUpdateStatus({ kind: "idle" }, null, "de", de)).toBe("");
  });

  it("names the time of the last check while idle, as after a silent automatic check", () => {
    const time = formatShortTime(new Date(checkedAt), "de");
    expect(hangarUpdateStatus({ kind: "idle" }, checkedAt, "de", de)).toBe(
      de("settings.lastChecked", { time }),
    );
  });

  it("reports checking, the result and errors", () => {
    expect(hangarUpdateStatus({ kind: "checking" }, checkedAt, "de", de)).toBe(
      de("update.checking"),
    );
    expect(hangarUpdateStatus({ kind: "current", version: "1.2.0" }, checkedAt, "de", de)).toBe(
      de("update.current", { version: "1.2.0" }),
    );
    expect(
      hangarUpdateStatus({ kind: "available", update: { version: "1.3.0" } }, null, "de", de),
    ).toBe(de("update.inSidebar", { version: "1.3.0" }));
    expect(hangarUpdateStatus({ kind: "error", message: "offline" }, checkedAt, "de", de)).toBe(
      "offline",
    );
  });
});

describe("programUpdateStatus", () => {
  const base = { updates: new Map(), checking: false, error: null, checkedAt: null };
  const checkedAt = Date.UTC(2026, 10, 21, 9, 30);
  const time = formatShortTime(new Date(checkedAt), "de");

  it("says when nothing has been checked yet", () => {
    expect(programUpdateStatus(base, "de", de)).toBe(de("settings.notChecked"));
  });

  it("prefers checking over an old error, and an error over an old result", () => {
    expect(programUpdateStatus({ ...base, checking: true, error: "x", checkedAt }, "de", de)).toBe(
      de("settings.programChecking"),
    );
    expect(programUpdateStatus({ ...base, error: "offline", checkedAt }, "de", de)).toBe(
      de("settings.programCheckFailed", { error: "offline" }),
    );
  });

  it("counts only programs with an update and adds the time of the check", () => {
    const updates = new Map([
      ["claude", { updateAvailable: true }],
      ["codex", { updateAvailable: false }],
      ["aider", { updateAvailable: true }],
    ]);
    expect(programUpdateStatus({ ...base, updates, checkedAt }, "de", de)).toBe(
      `${de("settings.programUpdatesSome", { count: 2 })} ${de("settings.programCheckedAt", { time })}`,
    );
    expect(programUpdateStatus({ ...base, checkedAt }, "de", de)).toBe(
      `${de("settings.programUpdatesNone")} ${de("settings.programCheckedAt", { time })}`,
    );
  });
});
