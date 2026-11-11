import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
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
  it("says how long a session has been working", () => {
    const line = statusLine(
      session(),
      "working",
      { workingSince: NOW - 3 * MINUTE, lastOutput: NOW },
      NOW,
      t,
      "en",
    );
    expect(line).toBe("working for 3 min");
  });

  it("keeps the plain text for the first minute of work", () => {
    const line = statusLine(
      session(),
      "working",
      { workingSince: NOW - 10_000, lastOutput: NOW },
      NOW,
      t,
      "en",
    );
    expect(line).toBe(t("status.working"));
  });

  it("says when an idle session was last active", () => {
    const line = statusLine(
      session(),
      "idle",
      { workingSince: null, lastOutput: NOW - 5 * MINUTE },
      NOW,
      t,
      "en",
    );
    expect(line).toBe("active 5 min ago");
  });

  it("shows running without any output yet", () => {
    expect(statusLine(session(), "idle", NO_ACTIVITY, NOW, t, "en")).toBe(t("status.idle"));
  });

  it("keeps waiting and ended sessions plain", () => {
    const activity = { workingSince: NOW - 9 * MINUTE, lastOutput: NOW - 9 * MINUTE };
    expect(statusLine(session(), "attention", activity, NOW, t, "en")).toBe(t("status.attention"));
    expect(statusLine(session({ status: "exited" }), "idle", activity, NOW, t, "en")).toBe(
      t("status.exited"),
    );
    expect(
      statusLine(
        session({ status: "exited", stoppedByUser: true }),
        "idle",
        activity,
        NOW,
        t,
        "en",
      ),
    ).toBe(t("status.exitedByUser"));
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
