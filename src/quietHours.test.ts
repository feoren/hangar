import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_QUIET_HOURS,
  formatClockTime,
  inWindow,
  isQuietTime,
  minuteOfDay,
  noticesAt,
  parseClockTime,
  parseQuietHours,
} from "./quietHours";

const at = (hours: number, minutes: number) => hours * 60 + minutes;

describe("inWindow", () => {
  it("covers a window within one day, start inclusive and end exclusive", () => {
    expect(inWindow(at(9, 0), at(17, 0), at(8, 59))).toBe(false);
    expect(inWindow(at(9, 0), at(17, 0), at(9, 0))).toBe(true);
    expect(inWindow(at(9, 0), at(17, 0), at(16, 59))).toBe(true);
    expect(inWindow(at(9, 0), at(17, 0), at(17, 0))).toBe(false);
  });

  it("runs over midnight when the end lies before the start", () => {
    expect(inWindow(at(22, 0), at(7, 0), at(21, 59))).toBe(false);
    expect(inWindow(at(22, 0), at(7, 0), at(22, 0))).toBe(true);
    expect(inWindow(at(22, 0), at(7, 0), at(23, 59))).toBe(true);
    expect(inWindow(at(22, 0), at(7, 0), 0)).toBe(true);
    expect(inWindow(at(22, 0), at(7, 0), at(6, 59))).toBe(true);
    expect(inWindow(at(22, 0), at(7, 0), at(7, 0))).toBe(false);
    expect(inWindow(at(22, 0), at(7, 0), at(12, 0))).toBe(false);
  });

  it("treats equal start and end as an empty window", () => {
    expect(inWindow(at(8, 0), at(8, 0), at(8, 0))).toBe(false);
    expect(inWindow(at(8, 0), at(8, 0), at(20, 0))).toBe(false);
  });

  it("handles a window that starts at midnight", () => {
    expect(inWindow(0, at(6, 0), 0)).toBe(true);
    expect(inWindow(0, at(6, 0), at(23, 59))).toBe(false);
  });
});

describe("isQuietTime", () => {
  const quiet = { enabled: true, from: at(22, 0), to: at(7, 0) };

  it("reads the local wall clock", () => {
    expect(isQuietTime(quiet, new Date(2026, 10, 12, 23, 30))).toBe(true);
    expect(isQuietTime(quiet, new Date(2026, 10, 13, 6, 59))).toBe(true);
    expect(isQuietTime(quiet, new Date(2026, 10, 13, 7, 0))).toBe(false);
  });

  it("is never quiet while turned off", () => {
    expect(isQuietTime({ ...quiet, enabled: false }, new Date(2026, 10, 12, 23, 30))).toBe(false);
  });
});

describe("daylight saving time", () => {
  let previous: string | undefined;
  beforeEach(() => {
    previous = process.env.TZ;
    process.env.TZ = "Europe/Berlin";
  });
  afterEach(() => {
    if (previous === undefined) delete process.env.TZ;
    else process.env.TZ = previous;
  });

  it("follows the wall clock on the night the clocks spring forward", () => {
    // 2026-03-29: 02:00 local jumps to 03:00. 00:30 UTC is 01:30 CET, 01:30 UTC is 03:30 CEST.
    const before = new Date(Date.UTC(2026, 2, 29, 0, 30));
    const after = new Date(Date.UTC(2026, 2, 29, 1, 30));
    expect(minuteOfDay(before)).toBe(at(1, 30));
    expect(minuteOfDay(after)).toBe(at(3, 30));
    const quiet = { enabled: true, from: at(22, 0), to: at(3, 0) };
    expect(isQuietTime(quiet, before)).toBe(true);
    expect(isQuietTime(quiet, after)).toBe(false);
  });

  it("stays quiet through the repeated hour when the clocks fall back", () => {
    // 2026-10-25: 03:00 local falls back to 02:00, so 02:30 happens twice.
    const first = new Date(Date.UTC(2026, 9, 25, 0, 30));
    const second = new Date(Date.UTC(2026, 9, 25, 1, 30));
    expect(minuteOfDay(first)).toBe(at(2, 30));
    expect(minuteOfDay(second)).toBe(at(2, 30));
    const quiet = { enabled: true, from: at(23, 0), to: at(6, 0) };
    expect(isQuietTime(quiet, first)).toBe(true);
    expect(isQuietTime(quiet, second)).toBe(true);
  });
});

describe("parseQuietHours", () => {
  it("falls back to the defaults for anything but an object", () => {
    expect(parseQuietHours(undefined)).toEqual(DEFAULT_QUIET_HOURS);
    expect(parseQuietHours(null)).toEqual(DEFAULT_QUIET_HOURS);
    expect(parseQuietHours([1, 2])).toEqual(DEFAULT_QUIET_HOURS);
    expect(parseQuietHours("22:00")).toEqual(DEFAULT_QUIET_HOURS);
  });

  it("keeps valid fields and replaces invalid ones", () => {
    expect(parseQuietHours({ enabled: true, from: 60, to: 120 })).toEqual({
      enabled: true,
      from: 60,
      to: 120,
    });
    expect(parseQuietHours({ enabled: "yes", from: 1440, to: 1.5 })).toEqual(DEFAULT_QUIET_HOURS);
    expect(parseQuietHours({ from: -1, to: 0 })).toEqual({ ...DEFAULT_QUIET_HOURS, to: 0 });
  });
});

describe("clock time", () => {
  it("formats with two digits each", () => {
    expect(formatClockTime(0)).toBe("00:00");
    expect(formatClockTime(at(7, 5))).toBe("07:05");
    expect(formatClockTime(at(23, 59))).toBe("23:59");
  });

  it("parses the time input's values", () => {
    expect(parseClockTime("22:00")).toBe(at(22, 0));
    expect(parseClockTime("07:05:30")).toBe(at(7, 5));
    expect(parseClockTime("24:00")).toBeNull();
    expect(parseClockTime("7:05")).toBeNull();
    expect(parseClockTime("")).toBeNull();
  });
});

describe("noticesAt", () => {
  const settings = {
    taskbarFlash: true,
    notifyDesktop: true,
    chime: true,
    quietHours: { enabled: true, from: at(22, 0), to: at(7, 0) },
    language: "de",
  };

  it("turns the interrupting notices off during quiet hours and keeps the rest", () => {
    expect(noticesAt(settings, new Date(2026, 10, 12, 23, 0))).toEqual({
      ...settings,
      taskbarFlash: false,
      notifyDesktop: false,
      chime: false,
    });
  });

  it("returns the same settings outside quiet hours", () => {
    expect(noticesAt(settings, new Date(2026, 10, 12, 12, 0))).toBe(settings);
  });
});
