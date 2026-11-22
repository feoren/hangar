import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import { createTranslator } from "./i18n";
import {
  formatBytes,
  BADGE_COLORS,
  badgeColor,
  badgeText,
  exitDetail,
  formatShortTime,
  shortPath,
  statusText,
  relativeTime,
  shortAge,
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
    expect(shortPath("/home/me/projects/hangar")).toBe("projects/hangar");
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
    expect(statusText({ status: "running", exitCode: null }, de)).toBe("macht Pause");
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

  it("says a running agent needs a decision while the gate holds it", () => {
    const running = { status: "running", exitCode: null } as const;
    expect(statusText(running, en, "attention", "gate")).toBe("needs your decision");
    expect(statusText(running, de, "attention", "gate")).toBe("braucht deine Entscheidung");
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

describe("shortAge", () => {
  const now = new Date(2026, 10, 9, 12, 0).getTime();
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;

  it("names the age short, in the UI language", () => {
    expect(shortAge(now - 5_000, now, "de")).toBe("jetzt");
    expect(shortAge(now + minute, now, "en")).toBe("now");
    expect(shortAge(now - 5 * minute, now, "de")).toBe("vor 5 Min.");
    expect(shortAge(now - 5 * minute, now, "en")).toBe("5 min. ago");
    expect(shortAge(now - 3 * hour, now, "de")).toBe("vor 3 Std.");
    expect(shortAge(now - 26 * hour, now, "de")).toBe("gestern");
    expect(shortAge(now - 26 * hour, now, "en")).toBe("yesterday");
  });

  it("counts weeks, months and years further back", () => {
    expect(shortAge(now - 8 * day, now, "en")).toBe("last wk.");
    expect(shortAge(now - 21 * day, now, "en")).toBe("3 wk. ago");
    expect(shortAge(now - 70 * day, now, "en")).toBe("2 mo. ago");
    expect(shortAge(now - 800 * day, now, "en")).toBe("2 yr. ago");
  });
});

describe("formatBytes", () => {
  it("shows bytes, then binary units with one decimal below 10", () => {
    expect(formatBytes(0, "en")).toBe("0 B");
    expect(formatBytes(1023, "en")).toBe("1,023 B");
    expect(formatBytes(1024, "en")).toBe("1 KiB");
    expect(formatBytes(1536, "en")).toBe("1.5 KiB");
    expect(formatBytes(20 * 1024 + 300, "en")).toBe("20 KiB");
    expect(formatBytes(2 * 1024 * 1024, "en")).toBe("2 MiB");
    expect(formatBytes(5.25 * 1024 ** 3, "en")).toBe("5.3 GiB");
  });

  it("uses the language's number format", () => {
    expect(formatBytes(1536, "de")).toBe("1,5 KiB");
    expect(formatBytes(1500, "de")).toBe("1,5 KiB");
  });
});
