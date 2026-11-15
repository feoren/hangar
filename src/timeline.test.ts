import { describe, expect, it } from "vitest";

import {
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
    });
    expect(list).toEqual({
      isRepo: true,
      snapshots: [entry({ number: 2, kind: "undo", prompt: "" }), entry()],
      skipped: { reason: "tooManyFiles", limit: 2000 },
    });
  });

  it("keeps a failed step's message and drops an unknown reason", () => {
    expect(
      parseSnapshotList({
        isRepo: true,
        snapshots: [],
        skipped: { reason: "failed", message: "git did not finish in time" },
      })?.skipped,
    ).toEqual({ reason: "failed", message: "git did not finish in time" });
    expect(
      parseSnapshotList({ isRepo: true, snapshots: [], skipped: { reason: "other" } })?.skipped,
    ).toBeNull();
    expect(parseSnapshotList({ isRepo: false, snapshots: [], skipped: null })).toEqual({
      isRepo: false,
      snapshots: [],
      skipped: null,
    });
  });

  it("refuses answers of the wrong shape", () => {
    for (const bad of [null, [], "x", { isRepo: true }, { isRepo: "yes", snapshots: [] }]) {
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
  });

  it("refuses anything else", () => {
    for (const bad of [null, {}, { kind: "restored" }, { kind: "restored", files: 1, undo: -1 }]) {
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
  it("matches the session key in lower case", () => {
    const key = "0F8FAD5B-D9CB-469F-A165-70867728950E";
    expect(concernsSession(key.toLowerCase(), key)).toBe(true);
    expect(concernsSession("11111111-2222-3333-4444-555555555555", key)).toBe(false);
    expect(concernsSession(key.toLowerCase(), null)).toBe(false);
    expect(concernsSession(42, key)).toBe(false);
  });
});
