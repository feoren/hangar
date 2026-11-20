import { describe, expect, it } from "vitest";

import { crashNoticeLines, crashSummary, hasProblems, type RestoredSession } from "./crashRestore";
import { createTranslator } from "./i18n";

const de = createTranslator("de");
const en = createTranslator("en");

const session = (id: number, title: string, resumed?: boolean) => ({ id, title, resumed });

describe("crashSummary", () => {
  it("counts resumed agents and plain starts", () => {
    const starts: RestoredSession[] = [
      { id: 1, start: "resumed" },
      { id: 2, start: "resumed" },
      { id: 3, start: "started" },
    ];
    const summary = crashSummary(starts, [
      session(1, "Claude", true),
      session(2, "Codex", true),
      session(3, "PowerShell"),
    ]);
    expect(summary).toEqual({ restored: 3, resumed: 2, notStarted: [], fellBack: [] });
  });

  it("names placeholders and sessions that fell back after their start", () => {
    const starts: RestoredSession[] = [
      { id: 1, start: "resumed" },
      { id: -1, start: "placeholder" },
      { id: 2, start: "resumed" },
    ];
    // Session 2 reported `resumeFallback` after the restore: the reducer set `resumed` to false.
    const summary = crashSummary(starts, [
      session(1, "Claude", true),
      session(-1, "Aider"),
      session(2, "Gemini", false),
    ]);
    expect(summary).toEqual({
      restored: 2,
      resumed: 1,
      notStarted: ["Aider"],
      fellBack: ["Gemini"],
    });
    expect(summary !== null && hasProblems(summary)).toBe(true);
  });

  it("forgets sessions closed or restarted since and ends when none are left", () => {
    const starts: RestoredSession[] = [
      { id: 1, start: "resumed" },
      { id: -1, start: "placeholder" },
    ];
    // The placeholder was restarted (new id), session 1 is still there.
    expect(crashSummary(starts, [session(1, "Claude", true), session(7, "Aider")])).toEqual({
      restored: 1,
      resumed: 1,
      notStarted: [],
      fellBack: [],
    });
    expect(crashSummary(starts, [session(7, "Aider")])).toBeNull();
    expect(crashSummary([], [])).toBeNull();
  });
});

describe("crashNoticeLines", () => {
  const none = { notStarted: [], fellBack: [] };

  it("says the agents continue when at least one resumed", () => {
    expect(crashNoticeLines({ restored: 4, resumed: 4, ...none }, de, "de")).toEqual([
      "Hangar wurde nicht sauber beendet. 4 Sessions wiederhergestellt, die Agenten setzen ihre Unterhaltung fort.",
    ]);
    expect(crashNoticeLines({ restored: 1, resumed: 1, ...none }, en, "en")).toEqual([
      "Hangar did not quit cleanly. One session restored, its agent continues the conversation.",
    ]);
  });

  it("only counts the sessions when no conversation resumed", () => {
    expect(crashNoticeLines({ restored: 2, resumed: 0, ...none }, de, "de")).toEqual([
      "Hangar wurde nicht sauber beendet. 2 Sessions wiederhergestellt.",
    ]);
    expect(crashNoticeLines({ restored: 1, resumed: 0, ...none }, de, "de")).toEqual([
      "Hangar wurde nicht sauber beendet. Eine Session wiederhergestellt.",
    ]);
  });

  it("lists what did not come back whole, one line per kind", () => {
    const lines = crashNoticeLines(
      { restored: 1, resumed: 0, notStarted: ["Aider", "Gemini"], fellBack: ["Codex"] },
      de,
      "de",
    );
    expect(lines).toEqual([
      "Hangar wurde nicht sauber beendet. Eine Session wiederhergestellt.",
      "Nicht gestartet: Aider und Gemini",
      "Ohne frühere Unterhaltung neu gestartet: Codex",
    ]);
  });

  it("says so when nothing came back", () => {
    expect(
      crashNoticeLines({ restored: 0, resumed: 0, notStarted: ["Aider"], fellBack: [] }, en, "en"),
    ).toEqual(["Hangar did not quit cleanly. No session could be restored.", "Not started: Aider"]);
  });

  it("has no problems to name after a whole restore", () => {
    expect(hasProblems({ restored: 3, resumed: 3, ...none })).toBe(false);
  });
});
