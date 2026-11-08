import { describe, expect, it } from "vitest";

import {
  buildWorkspace,
  EMPTY_WORKSPACE,
  isSessionKey,
  mergeWorkspace,
  newSessionKey,
  parseWorkspace,
  restorable,
  sameWorkspace,
  scrollbackDue,
  trimScrollback,
  utf8Length,
} from "./workspace";

const A = "0f8fad5b-d9cb-469f-a165-70867728950e";
const B = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
const C = "16fd2706-8baf-433b-82eb-8c7fada847da";

const entry = (key: string, title = "Claude Code") => ({
  key,
  profileId: "claude",
  cwd: "C:\\Users\\x\\project",
  title,
});

describe("session keys", () => {
  it("are UUIDs in upper or lower case", () => {
    expect(isSessionKey(A)).toBe(true);
    expect(isSessionKey(A.toUpperCase())).toBe(true);
    expect(isSessionKey("..\\x")).toBe(false);
    expect(isSessionKey(42)).toBe(false);
    expect(isSessionKey(newSessionKey())).toBe(true);
  });
});

describe("buildWorkspace", () => {
  it("numbers in sidebar order and remembers the active session", () => {
    const workspace = buildWorkspace([entry(B, "b"), entry(A, "a")], A);
    expect(workspace).toEqual({
      version: 1,
      sessions: [
        { ...entry(B, "b"), order: 0 },
        { ...entry(A, "a"), order: 1 },
      ],
      activeKey: A,
    });
  });

  it("remembers no update sessions, not even as the active one", () => {
    const workspace = buildWorkspace(
      [
        { ...entry(A, "Claude Code"), kind: "agent" as const },
        { ...entry(B, "Update: Claude Code"), kind: "update" as const },
        entry(C, "no kind"),
      ],
      B,
    );
    expect(workspace.sessions.map((s) => s.key)).toEqual([A, C]);
    expect(workspace.sessions.map((s) => s.order)).toEqual([0, 1]);
    expect(workspace.activeKey).toBeNull();
    // Only what the backend knows is saved: no `kind` field.
    expect(Object.keys(workspace.sessions[0] ?? {}).sort()).toEqual([
      "cwd",
      "key",
      "order",
      "profileId",
      "title",
    ]);
    expect(restorable({ kind: "update" })).toBe(false);
    expect(restorable({ kind: "agent" })).toBe(true);
    expect(restorable({})).toBe(true);
  });

  it("leaves out invalid and duplicate entries instead of dropping everything", () => {
    const workspace = buildWorkspace(
      [
        entry("broken"),
        entry(A),
        entry(A.toUpperCase()),
        { ...entry(B), profileId: " " },
        entry(C),
      ],
      B,
    );
    expect(workspace.sessions.map((s) => [s.key, s.order])).toEqual([
      [A, 0],
      [C, 1],
    ]);
    // The active session is gone: no active one rather than an invalid one.
    expect(workspace.activeKey).toBeNull();
  });

  it("writes keys in lower case, as the backend stores them", () => {
    const workspace = buildWorkspace([entry(A.toUpperCase())], A.toUpperCase());
    expect(workspace.sessions[0]?.key).toBe(A);
    expect(workspace.activeKey).toBe(A);
  });
});

describe("parseWorkspace", () => {
  it("reads the saved state and sorts by order", () => {
    const parsed = parseWorkspace({
      version: 1,
      sessions: [
        { ...entry(B, "b"), order: 5 },
        { ...entry(A, "a"), order: 2 },
      ],
      activeKey: B,
    });
    expect(parsed?.sessions.map((s) => [s.title, s.order])).toEqual([
      ["a", 0],
      ["b", 1],
    ]);
    expect(parsed?.activeKey).toBe(B);
  });

  it("returns null for missing or foreign content", () => {
    expect(parseWorkspace(null)).toBeNull();
    expect(parseWorkspace("x")).toBeNull();
    expect(parseWorkspace([])).toBeNull();
    expect(parseWorkspace({ version: 2, sessions: [] })).toBeNull();
    expect(parseWorkspace({ version: 1 })).toBeNull();
  });

  it("skips broken entries and an unknown active session", () => {
    const parsed = parseWorkspace({
      version: 1,
      sessions: [
        { ...entry(A), order: 0 },
        { ...entry(B), order: -1 },
        { key: C, profileId: "claude", cwd: 3, title: "x", order: 1 },
        "nonsense",
      ],
      activeKey: C,
    });
    expect(parsed?.sessions.map((s) => s.key)).toEqual([A]);
    expect(parsed?.activeKey).toBeNull();
  });

  it("survives a round trip unchanged", () => {
    const workspace = buildWorkspace([entry(A), entry(B)], B);
    const parsed = parseWorkspace(JSON.parse(JSON.stringify(workspace)));
    expect(parsed && sameWorkspace(parsed, workspace)).toBe(true);
    expect(sameWorkspace(EMPTY_WORKSPACE, workspace)).toBe(false);
  });
});

describe("mergeWorkspace", () => {
  it("keeps the remembered state after reloading and appends new sessions", () => {
    const remembered = buildWorkspace([entry(A, "old 1"), entry(B, "old 2")], B);
    const current = buildWorkspace([entry(C, "new")], C);
    const merged = mergeWorkspace(current, remembered);
    expect(merged.sessions.map((s) => [s.key, s.order])).toEqual([
      [A, 0],
      [B, 1],
      [C, 2],
    ]);
    expect(merged.activeKey).toBe(C);
  });

  it("takes the remembered active session while no new one is open", () => {
    const remembered = buildWorkspace([entry(A), entry(B)], B);
    expect(mergeWorkspace(EMPTY_WORKSPACE, remembered)).toEqual(remembered);
  });

  it("duplicates no session and changes nothing without a remembered state", () => {
    const remembered = buildWorkspace([entry(A, "remembered")], A);
    const current = buildWorkspace([entry(A, "current"), entry(B)], null);
    const merged = mergeWorkspace(current, remembered);
    expect(merged.sessions.map((s) => s.key)).toEqual([A, B]);
    expect(merged.sessions[0]?.title).toBe("remembered");
    expect(mergeWorkspace(current, null)).toBe(current);
    expect(mergeWorkspace(current, EMPTY_WORKSPACE)).toBe(current);
  });
});

describe("scrollbackDue", () => {
  it("saves only changed sessions after enough quiet", () => {
    expect(scrollbackDue({ dirty: false, lastOutputAt: 0 }, 10_000, 1500, false)).toBe(false);
    expect(scrollbackDue({ dirty: true, lastOutputAt: 9000 }, 10_000, 1500, false)).toBe(false);
    expect(scrollbackDue({ dirty: true, lastOutputAt: 8500 }, 10_000, 1500, false)).toBe(true);
  });

  it("takes every changed session when quitting", () => {
    expect(scrollbackDue({ dirty: true, lastOutputAt: 10_000 }, 10_000, 1500, true)).toBe(true);
    expect(scrollbackDue({ dirty: false, lastOutputAt: 0 }, 10_000, 1500, true)).toBe(false);
  });
});

describe("trimScrollback", () => {
  it("keeps short scrollback unchanged", () => {
    expect(trimScrollback("a\r\nb", 100)).toBe("a\r\nb");
  });

  it("trims from the front at line ends and resets the colors", () => {
    const text = ["one", "two", "three", "four"].join("\r\n");
    const trimmed = trimScrollback(text, 16);
    expect(trimmed).toBe("\x1b[0mthree\r\nfour");
    expect(utf8Length(trimmed)).toBeLessThanOrEqual(16);
  });

  it("counts bytes, not characters", () => {
    const text = `${"ä".repeat(10)}\nok`;
    // 20 bytes for the umlauts: does not fit in 12, so only the last line remains.
    expect(trimScrollback(text, 12)).toBe("\x1b[0mok");
  });

  it("returns empty when even the last line is too long", () => {
    expect(trimScrollback("x".repeat(50), 10)).toBe("");
  });
});
