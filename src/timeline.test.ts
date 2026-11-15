import { describe, expect, it } from "vitest";

import {
  canRestore,
  concernsSession,
  parseRestoreOutcome,
  parseSnapshotList,
  PROMPT_PREVIEW_MAX,
  SNAPSHOT_PROMPT_MAX,
  snapshotPrompt,
  snapshotTitle,
  type Snapshot,
} from "./timeline";

const SHA = "0123456789abcdef0123456789abcdef01234567";

function entry(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    number: 1,
    sha: SHA,
    createdAt: 1_700_000_000,
    files: 2,
    kind: "step",
    prompt: "fix the login",
    ...overrides,
  };
}

describe("parseSnapshotList", () => {
  it("reads the backend's answer and leaves malformed entries out", () => {
    const list = parseSnapshotList({
      isRepo: true,
      snapshots: [
        entry({ number: 2, kind: "undo", prompt: "" }),
        entry(),
        { ...entry(), sha: "HEAD" },
        { ...entry(), number: 0 },
        { ...entry(), kind: "jump" },
        { ...entry(), files: -1 },
        null,
      ],
      skipped: { reason: "tooManyFiles", limit: 2000 },
      others: 1,
      busy: true,
    });
    expect(list).toEqual({
      isRepo: true,
      snapshots: [entry({ number: 2, kind: "undo", prompt: "" }), entry()],
      skipped: { reason: "tooManyFiles", limit: 2000 },
      others: 1,
      busy: true,
    });
  });

  it("keeps a failed or too large step and drops an unknown reason", () => {
    const base = { isRepo: true, snapshots: [], others: 0, busy: false };
    expect(
      parseSnapshotList({
        ...base,
        skipped: { reason: "failed", message: "git did not finish in time" },
      })?.skipped,
    ).toEqual({ reason: "failed", message: "git did not finish in time" });
    expect(
      parseSnapshotList({ ...base, skipped: { reason: "tooLarge", fileMib: 50, totalMib: 200 } })
        ?.skipped,
    ).toEqual({ reason: "tooLarge", fileMib: 50, totalMib: 200 });
    expect(
      parseSnapshotList({ ...base, skipped: { reason: "tooLarge", fileMib: 50 } })?.skipped,
    ).toBeNull();
    expect(parseSnapshotList({ ...base, skipped: { reason: "other" } })?.skipped).toBeNull();
    expect(parseSnapshotList({ ...base, isRepo: false, skipped: null })).toEqual({
      ...base,
      isRepo: false,
      skipped: null,
    });
  });

  it("refuses answers of the wrong shape", () => {
    const good = { isRepo: true, snapshots: [], skipped: null, others: 0, busy: false };
    for (const bad of [
      null,
      [],
      "x",
      { isRepo: true },
      { ...good, isRepo: "yes" },
      { ...good, others: -1 },
      { ...good, others: undefined },
      { ...good, busy: "no" },
    ]) {
      expect(parseSnapshotList(bad)).toBeNull();
    }
  });
});

describe("parseRestoreOutcome", () => {
  it("reads both outcomes", () => {
    expect(parseRestoreOutcome({ kind: "restored", files: 3, undo: 4 })).toEqual({
      kind: "restored",
      files: 3,
      undo: 4,
    });
    expect(parseRestoreOutcome({ kind: "alreadyThere" })).toEqual({ kind: "alreadyThere" });
    expect(parseRestoreOutcome({ kind: "incomplete", undo: 5, message: "denied" })).toEqual({
      kind: "incomplete",
      undo: 5,
      message: "denied",
    });
  });

  it("refuses anything else", () => {
    for (const bad of [
      null,
      {},
      { kind: "restored" },
      { kind: "restored", files: 1, undo: -1 },
      { kind: "incomplete", undo: 5 },
      { kind: "incomplete", message: "denied" },
    ]) {
      expect(parseRestoreOutcome(bad)).toBeNull();
    }
  });
});

describe("snapshotPrompt", () => {
  it("sends the last prompt on one line, cut to the backend's length", () => {
    expect(snapshotPrompt({ text: "  fix\n the  bug " })).toBe("fix the bug");
    expect(snapshotPrompt({ text: "ä".repeat(500) })).toHaveLength(SNAPSHOT_PROMPT_MAX);
  });

  it("sends nothing without a prompt", () => {
    expect(snapshotPrompt(null)).toBeNull();
    expect(snapshotPrompt({ text: "   " })).toBeNull();
  });
});

describe("snapshotTitle", () => {
  it("names a step by its prompt, shortened to one row", () => {
    expect(snapshotTitle(entry())).toEqual({ kind: "prompt", text: "fix the login" });
    const long = snapshotTitle(entry({ prompt: "x".repeat(300) }));
    expect(long.kind === "prompt" && long.text).toHaveLength(PROMPT_PREVIEW_MAX);
    expect(long.kind === "prompt" && long.text.endsWith("…")).toBe(true);
  });

  it("names an undo point and a step without prompt", () => {
    expect(snapshotTitle(entry({ kind: "undo", prompt: "ignored" }))).toEqual({ kind: "undo" });
    expect(snapshotTitle(entry({ prompt: " " }))).toEqual({ kind: "step" });
  });
});

describe("concernsSession", () => {
  it("matches the session ID", () => {
    expect(concernsSession(7, 7)).toBe(true);
    expect(concernsSession(8, 7)).toBe(false);
    expect(concernsSession("7", 7)).toBe(false);
  });
});

describe("canRestore", () => {
  const list = { isRepo: true, snapshots: [], skipped: null, others: 0, busy: false };
  it("is off while this session or another one in the work tree works", () => {
    expect(canRestore(list, false)).toBe(true);
    expect(canRestore(list, true)).toBe(false);
    expect(canRestore({ ...list, busy: true }, false)).toBe(false);
  });
});
