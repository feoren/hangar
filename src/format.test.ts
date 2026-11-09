import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import { createTranslator } from "./i18n";
import {
  BADGE_COLORS,
  badgeColor,
  badgeText,
  exitDetail,
  formatShortTime,
  shortPath,
  statusText,
  relativeTime,
} from "./format";
import { contrastRatio } from "./theme";

const profile: Profile = {
  id: "claude",
  name: "Claude Code",
  command: "claude",
  args: [],
  cwd: null,
  source: "user",
  installed: true,
  homepage: null,
  installHint: null,
  icon: null,
};

describe("shortPath", () => {
  it("shows the last two folders", () => {
    expect(shortPath("E:\\Projects\\Local\\hangar")).toBe("Local/hangar");
    expect(shortPath("/home/feo/projects/hangar")).toBe("projects/hangar");
  });

  it("keeps short paths and roots unchanged", () => {
    expect(shortPath("C:\\")).toBe("C:\\");
    expect(shortPath("/")).toBe("/");
    expect(shortPath("C:\\Users")).toBe("C:/Users");
  });
});

describe("statusText", () => {
  const de = createTranslator("de");
  const en = createTranslator("en");

  it("shows the status without an exit code, a failure as such", () => {
    expect(statusText({ status: "exited", exitCode: 3 }, de)).toBe("mit Fehler beendet");
    expect(statusText({ status: "exited", exitCode: 0 }, de)).toBe("beendet");
    expect(statusText({ status: "exited", exitCode: null }, de)).toBe("beendet");
    expect(statusText({ status: "running", exitCode: null }, de)).toBe("läuft");
    expect(statusText({ status: "exited", exitCode: 3 }, en)).toBe("exited with an error");
  });

  it("does not call a failure that waits to be seen done", () => {
    const failed = { status: "exited", exitCode: 1 } as const;
    expect(statusText(failed, de, "attention", "exit")).toBe("mit Fehler beendet");
  });

  it("distinguishes sessions ended by the user", () => {
    const stopped = { status: "exited", exitCode: 1, stoppedByUser: true } as const;
    expect(statusText(stopped, de)).toBe("beendet (von dir)");
    expect(exitDetail(stopped, de)).toBe("");
  });

  it("names a non-zero exit code in the tooltip", () => {
    expect(exitDetail({ status: "exited", exitCode: 3 }, de)).toBe("Exit-Code 3");
    expect(exitDetail({ status: "exited", exitCode: -1 }, en)).toBe("Exit code -1");
    expect(exitDetail({ status: "exited", exitCode: 0 }, de)).toBe("");
    expect(exitDetail({ status: "exited", exitCode: null }, de)).toBe("");
    expect(exitDetail({ status: "running", exitCode: 3 }, de)).toBe("");
  });

  it("shows working and waiting sessions", () => {
    const running = { status: "running", exitCode: null } as const;
    expect(statusText(running, de, "working")).toBe("arbeitet");
    expect(statusText(running, de, "attention")).toBe("fertig, wartet auf dich");
    expect(statusText({ status: "exited", exitCode: 0 }, en, "attention")).toBe(
      "done, waiting for you",
    );
  });

  it("says a running program asks when it rang the bell", () => {
    const running = { status: "running", exitCode: null } as const;
    expect(statusText(running, de, "attention", "bell")).toBe("fragt dich etwas");
    expect(statusText(running, de, "attention", "quiet")).toBe("fertig, wartet auf dich");
    expect(statusText({ status: "exited", exitCode: 0 }, de, "attention", "bell")).toBe(
      "fertig, wartet auf dich",
    );
  });
});

describe("badgeText", () => {
  it("takes the icon, otherwise the first letter", () => {
    expect(badgeText({ ...profile, icon: "Cx" }, "x")).toBe("Cx");
    expect(badgeText(profile, "x")).toBe("C");
    expect(badgeText(undefined, "pwsh")).toBe("P");
  });

  it("keeps a three letter abbreviation such as WSL", () => {
    expect(badgeText({ ...profile, icon: "WSL" }, "wsl-ubuntu")).toBe("WSL");
    expect(badgeText({ ...profile, icon: "Wsl!" }, "x")).toBe("Ws");
  });

  it("does not cut an emoji in half", () => {
    expect(badgeText({ ...profile, icon: "🚀🚀🚀" }, "x")).toBe("🚀🚀");
  });
});

describe("badgeColor", () => {
  it("is stable per ID", () => {
    expect(badgeColor("claude")).toBe(badgeColor("claude"));
    expect(badgeColor("claude")).toMatch(/^#[0-9a-f]{6}$/);
  });
});

describe("BADGE_COLORS", () => {
  it("white badge text is readable on every color (AA)", () => {
    for (const color of BADGE_COLORS) {
      expect(contrastRatio("#ffffff", color), color).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("formatShortTime", () => {
  it("shows day, month and time in the language", () => {
    const date = new Date(2026, 10, 7, 16, 40);
    const de = formatShortTime(date, "de");
    expect(de).toContain("07.11.");
    expect(de).toContain("16:40");
    expect(formatShortTime(date, "en")).toContain("11/07");
  });
});

describe("relativeTime", () => {
  const now = new Date(2026, 10, 9, 12, 0).getTime();

  it("names recent times relative to now", () => {
    expect(relativeTime(now - 5_000, now, "en")).toBe("now");
    expect(relativeTime(now + 60_000, now, "en")).toBe("now");
    expect(relativeTime(now - 5 * 60_000, now, "en")).toBe("5 minutes ago");
    expect(relativeTime(now - 3 * 3_600_000, now, "en")).toBe("3 hours ago");
    expect(relativeTime(now - 26 * 3_600_000, now, "en")).toBe("yesterday");
    expect(relativeTime(now - 26 * 3_600_000, now, "de")).toBe("gestern");
  });

  it("shows a date after a week, with the year only for other years", () => {
    expect(relativeTime(new Date(2026, 9, 1).getTime(), now, "en")).toBe("Oct 1");
    expect(relativeTime(new Date(2025, 9, 1).getTime(), now, "en")).toBe("Oct 1, 2025");
  });
});
