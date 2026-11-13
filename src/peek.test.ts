import { describe, expect, it } from "vitest";

import {
  PEEK_GAP,
  PEEK_MAX_WIDTH,
  PEEK_MIN_WIDTH,
  peekNotes,
  peekShows,
  peekPosition,
  peekWidth,
  sameLines,
} from "./peek";

const viewport = { width: 1200, height: 800 };
const box = { width: 440, height: 240 };

describe("peekPosition", () => {
  it("sits right of the sidebar, level with the row", () => {
    expect(peekPosition({ top: 120, sidebarRight: 280 }, box, viewport)).toEqual({
      left: 280 + PEEK_GAP,
      top: 120,
    });
  });

  it("moves up near the bottom of the window", () => {
    expect(peekPosition({ top: 700, sidebarRight: 280 }, box, viewport).top).toBe(
      800 - 240 - PEEK_GAP,
    );
  });

  it("never leaves the top of the window", () => {
    expect(peekPosition({ top: -30, sidebarRight: 280 }, box, viewport).top).toBe(PEEK_GAP);
    // Window lower than the peek: the top edge stays visible.
    expect(
      peekPosition({ top: 50, sidebarRight: 280 }, box, { width: 1200, height: 200 }).top,
    ).toBe(PEEK_GAP);
  });

  it("keeps inside a narrow window", () => {
    expect(peekPosition({ top: 0, sidebarRight: 600 }, box, { width: 900, height: 800 }).left).toBe(
      900 - 440 - PEEK_GAP,
    );
    expect(peekPosition({ top: 0, sidebarRight: 600 }, box, { width: 300, height: 800 }).left).toBe(
      PEEK_GAP,
    );
  });
});

describe("peekWidth", () => {
  it("uses the full width when there is room", () => {
    expect(peekWidth(280, 1200)).toBe(PEEK_MAX_WIDTH);
  });

  it("shrinks to the room right of the sidebar", () => {
    expect(peekWidth(280, 720)).toBe(720 - 280 - 2 * PEEK_GAP);
  });

  it("shows nothing rather than covering the sidebar", () => {
    expect(peekWidth(560, 720)).toBeNull();
    expect(peekWidth(720 - 2 * PEEK_GAP - PEEK_MIN_WIDTH, 720)).toBe(PEEK_MIN_WIDTH);
    expect(peekWidth(720 - 2 * PEEK_GAP - PEEK_MIN_WIDTH + 1, 720)).toBeNull();
  });
});

describe("sameLines", () => {
  it("compares line by line", () => {
    expect(sameLines(["a", "b"], ["a", "b"])).toBe(true);
    expect(sameLines(["a", "b"], ["a", "c"])).toBe(false);
    expect(sameLines(["a"], ["a", "b"])).toBe(false);
  });

  it("treats a missing preview as its own value", () => {
    expect(sameLines(null, null)).toBe(true);
    expect(sameLines(null, [])).toBe(false);
    expect(sameLines([], null)).toBe(false);
  });
});

describe("peekNotes", () => {
  it("lists overlap, gate note and folder in that order", () => {
    expect(
      peekNotes({ overlap: "Also changed by #2", gateOff: "Gate is off", cwd: "C:\\work\\app" }),
    ).toEqual(["Also changed by #2", "Gate is off", "C:\\work\\app"]);
  });

  it("leaves out missing, blank and repeated texts", () => {
    expect(peekNotes({})).toEqual([]);
    expect(peekNotes({ overlap: null, gateOff: "  ", cwd: "/home/me" })).toEqual(["/home/me"]);
    expect(peekNotes({ overlap: "same", gateOff: "same" })).toEqual(["same"]);
  });

  it("names the program before the folder", () => {
    expect(
      peekNotes({ program: "PowerShell 7 · 7.6.9", devServer: "http://localhost:5173", cwd: "/a" }),
    ).toEqual(["http://localhost:5173", "PowerShell 7 · 7.6.9", "/a"]);
  });

  it("spells out what a row only marks, action first and the folder last", () => {
    expect(
      peekNotes({
        cwd: "/home/me/app",
        devServer: "http://localhost:5173",
        outlier: "Outlier",
        exit: "Exit code 1",
        gateOff: "Gate is off",
        overlap: "Also changed by #2",
      }),
    ).toEqual([
      "Also changed by #2",
      "Gate is off",
      "Exit code 1",
      "Outlier",
      "http://localhost:5173",
      "/home/me/app",
    ]);
  });
});

describe("peekShows", () => {
  it("waits for lines when it shows the terminal", () => {
    expect(peekShows(true, null)).toBe(false);
    expect(peekShows(true, [])).toBe(false);
    expect(peekShows(true, ["$ ls"])).toBe(true);
  });

  it("always shows for a session on screen, without lines", () => {
    expect(peekShows(false, null)).toBe(true);
  });
});
