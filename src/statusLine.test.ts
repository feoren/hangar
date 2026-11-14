import { describe, expect, it } from "vitest";

import { SHORT_PHRASES } from "./format";
import { createTranslator, LANGUAGES } from "./i18n";
import { detailLine, formatDuration, NO_ACTIVITY, statusLine } from "./statusLine";
import type { SessionView } from "./sessions";

const MINUTE = 60_000;
const t = createTranslator("en");
const NOW = 1_700_000_000_000;

function session(extra: Partial<SessionView> = {}): SessionView {
  return {
    id: 1,
    profileId: "claude",
    title: "Claude Code",
    cwd: "C:\\src\\hangar",
    status: "running",
    exitCode: null,
    createdAt: NOW - 90 * MINUTE,
    ...extra,
  };
}

describe("formatDuration", () => {
  it("says nothing below a minute", () => {
    expect(formatDuration(59_000, "en")).toBeNull();
    expect(formatDuration(-5, "en")).toBeNull();
    expect(formatDuration(Number.NaN, "en")).toBeNull();
  });

  it("uses the largest whole unit", () => {
    expect(formatDuration(3 * MINUTE + 59_000, "en")).toBe("3 min");
    expect(formatDuration(150 * MINUTE, "en")).toBe("2 hr");
    expect(formatDuration(3 * 24 * 60 * MINUTE, "en")).toBe("3 days");
  });

  it("follows the UI language", () => {
    expect(formatDuration(5 * MINUTE, "de")).toBe("5 Min.");
  });
});

describe("statusLine", () => {
  const line = (
    state: Parameters<typeof statusLine>[2],
    activity = NO_ACTIVITY,
    extra: Partial<SessionView> = {},
    reason: Parameters<typeof statusLine>[7] = null,
  ) => statusLine("Claude Code", session(extra), state, activity, NOW, t, "en", reason).full;

  it("gives the row one word and the peek the whole sentence with the duration", () => {
    const working = statusLine(
      "Claude Code",
      session(),
      "working",
      { workingSince: NOW - 3 * MINUTE, lastOutput: NOW },
      NOW,
      t,
      "en",
    );
    // The group header above the row names the program; the row says only what it does.
    expect(working).toEqual({
      word: "working",
      full: "Claude Code has been working for 3 min",
    });
    const resting = statusLine(
      "Claude Code",
      session(),
      "idle",
      { workingSince: null, lastOutput: NOW - 5 * MINUTE },
      NOW,
      t,
      "en",
    );
    expect(resting.word).toBe("paused");
    expect(resting.full).toContain("5 min");
  });

  it("has no duration below a minute or while waiting", () => {
    const fresh = statusLine(
      "Claude Code",
      session(),
      "working",
      { workingSince: NOW - 10_000, lastOutput: NOW },
      NOW,
      t,
      "en",
    );
    expect(fresh).toEqual({
      word: "working",
      full: "Claude Code is working",
    });
    const waiting = statusLine(
      "Claude Code",
      session(),
      "attention",
      { workingSince: NOW - 9 * MINUTE, lastOutput: NOW - 9 * MINUTE },
      NOW,
      t,
      "en",
    );
    expect(waiting.word).toBe("done");
    expect(waiting.full).not.toMatch(/\d+ min/);
  });

  it("names the program and how long it has been working", () => {
    expect(line("working", { workingSince: NOW - 3 * MINUTE, lastOutput: NOW })).toBe(
      "Claude Code has been working for 3 min",
    );
  });

  it("keeps the plain phrase for the first minute of work", () => {
    expect(line("working", { workingSince: NOW - 10_000, lastOutput: NOW })).toBe(
      "Claude Code is working",
    );
  });

  it("says how long a resting session has been quiet", () => {
    expect(line("idle", { workingSince: null, lastOutput: NOW - 5 * MINUTE })).toBe(
      "Claude Code has been on a break for 5 min",
    );
    expect(line("idle")).toBe("Claude Code is taking a break");
  });

  it("names the program in every waiting and ended state", () => {
    const activity = { workingSince: NOW - 9 * MINUTE, lastOutput: NOW - 9 * MINUTE };
    expect(line("attention", activity)).toBe("Claude Code is waiting for you");
    expect(line("attention", activity, {}, "bell")).toBe("Claude Code has a question");
    expect(line("attention", activity, {}, "gate")).toBe("Claude Code needs your decision");
    expect(line("idle", activity, { status: "starting" })).toBe("Claude Code is starting");
    expect(line("idle", activity, { status: "exited", exitCode: 0 })).toBe(
      "Claude Code has exited",
    );
    expect(line("idle", activity, { status: "exited", exitCode: 0, stoppedByUser: true })).toBe(
      "Claude Code was stopped by you",
    );
  });

  it("keeps an error an error, also while it waits to be seen", () => {
    const failed = { status: "exited", exitCode: 2 } as const;
    expect(line("attention", NO_ACTIVITY, failed, "exit")).toBe("Claude Code hit an error");
    expect(line("idle", NO_ACTIVITY, failed)).toBe("Claude Code hit an error");
  });

  it("builds a sentence in German with the program as its subject", () => {
    const de = createTranslator("de");
    const pause = { workingSince: null, lastOutput: NOW - 5 * MINUTE };
    expect(statusLine("Codex CLI", session(), "idle", pause, NOW, de, "de").full).toBe(
      "Codex CLI macht seit 5 Min. Pause",
    );
    expect(statusLine("Codex CLI", session(), "idle", NO_ACTIVITY, NOW, de, "de").full).toBe(
      "Codex CLI macht Pause",
    );
    expect(statusLine("PowerShell", session(), "attention", NO_ACTIVITY, NOW, de, "de").full).toBe(
      "PowerShell wartet auf dich",
    );
  });
});

describe("stall hint in the status line", () => {
  const working = { workingSince: NOW - 20 * MINUTE, lastOutput: NOW };
  const line = (
    state: Parameters<typeof statusLine>[2],
    stalledSince: number | null,
    extra: Partial<SessionView> = {},
    reason: Parameters<typeof statusLine>[7] = null,
  ) =>
    statusLine("Claude Code", session(extra), state, working, NOW, t, "en", reason, stalledSince);

  it("names the stall in one word and says how long in the sentence", () => {
    const stalled = line("working", NOW - 7 * MINUTE);
    expect(stalled.word).toBe("quiet");
    expect(stalled.full).toBe("Claude Code has shown nothing new for 7 min");
    expect(stalled.stalled).toBe(true);
    const german = statusLine(
      "Claude Code",
      session(),
      "working",
      working,
      NOW,
      createTranslator("de"),
      "de",
      null,
      NOW - 7 * MINUTE,
    );
    expect(german.word).toBe("still");
    expect(german.full).toBe("Claude Code zeigt seit 7 Min. nichts Neues");
  });

  it("never shows for done, asks, failed or ended sessions", () => {
    const since = NOW - 7 * MINUTE;
    expect(line("attention", since, {}, "quiet").stalled).toBeUndefined();
    expect(line("attention", since, {}, "bell").stalled).toBeUndefined();
    expect(line("idle", since, { status: "exited", exitCode: 1 }).stalled).toBeUndefined();
    expect(line("idle", since).stalled).toBeUndefined();
    expect(line("working", null).word).toBe(t("status.short.working"));
  });
});

describe("detailLine", () => {
  it("names the folder, the branch and the run time", () => {
    expect(detailLine(session(), "main", NOW, "en")).toEqual({
      folder: "hangar",
      branch: "main",
      runtime: "1 hr",
    });
  });

  it("leaves out the run time of an ended session", () => {
    expect(detailLine(session({ status: "exited" }), null, NOW, "en").runtime).toBeNull();
  });
});

describe("the one-word status", () => {
  it("is a single word for every state in every language", () => {
    for (const language of LANGUAGES) {
      const translate = createTranslator(language);
      for (const key of Object.values(SHORT_PHRASES)) {
        const word = translate(key);
        expect(word).not.toBe(key);
        expect(word).not.toMatch(/\s/);
      }
    }
  });
});
