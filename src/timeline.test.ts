import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  canRestore,
  changesViews,
  concernsSession,
  headMoved,
  parseRestoreOutcome,
  parseSnapshotList,
  PROMPT_PREVIEW_MAX,
  restoreErrorText,
  shownChangesView,
  SNAPSHOT_PROMPT_MAX,
  snapshotPrompt,
  snapshotTitle,
  StepPrompt,
  takesSnapshots,
  timelineAvailability,
  timelineErrorText,
  type Snapshot,
  type SnapshotList,
} from "./timeline";

const SHA = "0123456789abcdef0123456789abcdef01234567";
const HEAD = "89abcdef0123456789abcdef0123456789abcdef";
const ROOT = "C:\\Users\\dev\\project";
const t = createTranslator("de");
const en = createTranslator("en");

function list(overrides: Partial<SnapshotList> = {}): SnapshotList {
  return {
    isRepo: true,
    root: ROOT,
    head: HEAD,
    snapshots: [],
    skipped: null,
    others: 0,
    busy: false,
    ...overrides,
  };
}

function entry(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    number: 1,
    sha: SHA,
    createdAt: 1_700_000_000,
    files: 2,
    kind: "step",
    prompt: "fix the login",
    parent: HEAD,
    ...overrides,
  };
}

describe("parseSnapshotList", () => {
  it("reads the backend's answer and leaves malformed entries out", () => {
    const parsed = parseSnapshotList({
      isRepo: true,
      root: ROOT,
      head: HEAD,
      snapshots: [
        entry({ number: 2, kind: "undo", prompt: "", parent: null }),
        entry(),
        { ...entry(), sha: "HEAD" },
        { ...entry(), parent: "main" },
        { ...entry(), parent: undefined },
        { ...entry(), number: 0 },
        { ...entry(), kind: "jump" },
        { ...entry(), files: -1 },
        null,
      ],
      skipped: { reason: "tooManyFiles", limit: 2000 },
      others: 1,
      busy: true,
    });
    expect(parsed).toEqual({
      isRepo: true,
      root: ROOT,
      head: HEAD,
      snapshots: [entry({ number: 2, kind: "undo", prompt: "", parent: null }), entry()],
      skipped: { reason: "tooManyFiles", limit: 2000 },
      others: 1,
      busy: true,
    });
  });

  it("keeps a failed or too large step and drops an unknown reason", () => {
    const base = { isRepo: true, root: ROOT, head: null, snapshots: [], others: 0, busy: false };
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
    const good = list();
    expect(parseSnapshotList(good)).toEqual(good);
    expect(parseSnapshotList({ ...good, isRepo: false, root: "", head: null })).toEqual({
      ...good,
      isRepo: false,
      root: "",
      head: null,
    });
    for (const bad of [
      null,
      [],
      "x",
      { isRepo: true },
      { ...good, isRepo: "yes" },
      { ...good, others: -1 },
      { ...good, others: undefined },
      { ...good, busy: "no" },
      { ...good, root: undefined },
      { ...good, root: 3 },
      { ...good, head: undefined },
      { ...good, head: "HEAD" },
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
  it("is off while this session or another one in the work tree works", () => {
    expect(canRestore(list(), false)).toBe(true);
    expect(canRestore(list(), true)).toBe(false);
    expect(canRestore(list({ busy: true }), false)).toBe(false);
  });
});

describe("headMoved", () => {
  it("warns when another commit is checked out than when the snapshot was taken", () => {
    expect(headMoved(list(), entry())).toBe(false);
    expect(headMoved(list(), entry({ parent: SHA }))).toBe(true);
    expect(headMoved(list({ head: null }), entry())).toBe(true);
    expect(headMoved(list(), entry({ parent: null }))).toBe(true);
  });

  it("stays quiet on a branch that had no commit then and has none now", () => {
    expect(headMoved(list({ head: null }), entry({ parent: null }))).toBe(false);
  });
});

describe("timelineAvailability", () => {
  it("hides the timeline where no snapshot is ever taken", () => {
    expect(timelineAvailability(false, true)).toBe("hidden");
    expect(timelineAvailability(false, false)).toBe("hidden");
  });

  it("explains instead of listing while working-state detection is off", () => {
    expect(timelineAvailability(true, false)).toBe("detectionOff");
    expect(timelineAvailability(true, true)).toBe("shown");
  });
});

describe("takesSnapshots", () => {
  it("follows the agent profile, not the running process", () => {
    expect(takesSnapshots(3, true)).toBe(true);
    expect(takesSnapshots(3, false)).toBe(false);
  });

  it("leaves placeholders out", () => {
    expect(takesSnapshots(-1, true)).toBe(false);
  });
});

describe("changesViews", () => {
  it("offers the timeline only where it is available", () => {
    expect(changesViews("shown")).toEqual(["open", "commits", "timeline"]);
    expect(changesViews("detectionOff")).toEqual(["open", "commits", "timeline"]);
    expect(changesViews("hidden")).toEqual(["open", "commits"]);
  });

  it("falls back to the open changes for a remembered hidden timeline", () => {
    expect(shownChangesView("timeline", "hidden")).toBe("open");
    expect(shownChangesView("timeline", "detectionOff")).toBe("timeline");
    expect(shownChangesView("commits", "hidden")).toBe("commits");
  });
});

describe("StepPrompt", () => {
  it("keeps the first line submitted while the agent waits, until the snapshot takes it", () => {
    const prompt = new StepPrompt();
    prompt.submitted("fix  the\tlogin", "idle", "> fix the login");
    prompt.submitted("and the logout", "attention", "> and the logout");
    expect(prompt.take()).toBe("fix the login");
    expect(prompt.take()).toBeNull();
    prompt.submitted("next task", "attention", "> next task");
    expect(prompt.take()).toBe("next task");
  });

  it("never keeps a line submitted while the agent works", () => {
    const prompt = new StepPrompt();
    prompt.submitted("hunter-marker", "working", "[sudo] password for dev: ");
    prompt.submitted("y", "working", "Continue? y");
    expect(prompt.take()).toBeNull();
  });

  it("never keeps a line typed at a prompt that hides its input", () => {
    const prompt = new StepPrompt();
    for (const row of [
      "Password: ",
      "[sudo] password for dev:",
      "Enter passphrase for key '/home/dev/.ssh/id':",
      "Passwort:",
      "PIN：",
      "Personal access token: ",
    ]) {
      prompt.submitted("quiet-marker", "idle", row);
    }
    expect(prompt.take()).toBeNull();
    prompt.submitted("rename the password field", "idle", "> rename the password field");
    expect(prompt.take()).toBe("rename the password field");
  });

  it("skips blank lines and cuts long ones on arrival", () => {
    const prompt = new StepPrompt();
    prompt.submitted("   ", "idle", ">");
    prompt.submitted("ä".repeat(500), "idle", "> ä");
    expect(prompt.take()).toHaveLength(SNAPSHOT_PROMPT_MAX);
  });
});

describe("restoreErrorText", () => {
  it("names each refusal in the window's own words, with its path", () => {
    expect(restoreErrorText("[snapshot_working] A session in this work tree is working", t)).toBe(
      t("timeline.errorWorking"),
    );
    expect(restoreErrorText("[snapshot_filtered] A file uses a git filter", en)).toBe(
      en("timeline.errorFiltered"),
    );
    for (const [code, key] of [
      ["snapshot_folder_in_the_way", "timeline.errorFolderInTheWay"],
      ["snapshot_ignored_in_the_way", "timeline.errorIgnoredInTheWay"],
      ["snapshot_linked", "timeline.errorLinked"],
      ["snapshot_symlink", "timeline.errorSymlink"],
      ["snapshot_too_large_to_check", "timeline.errorTooLargeToCheck"],
      ["snapshot_outside", "timeline.errorOutside"],
    ] as const) {
      expect(restoreErrorText(`[${code}] English text\nsrc/a b.ts`, t)).toBe(
        t(key, { path: "src/a b.ts" }),
      );
    }
  });

  it("passes git's reason on when the current state could not be saved", () => {
    expect(
      restoreErrorText(
        "[snapshot_cannot_save_current] Could not save the current state\nfatal: bad\nobject",
        t,
      ),
    ).toBe(t("timeline.errorCannotSaveCurrent", { reason: "fatal: bad object" }));
  });

  it("shows characters a path could hide", () => {
    expect(restoreErrorText("[snapshot_linked] Link\nsrc/\u202ex.ts", en)).not.toContain("\u202e");
  });

  it("falls back to the general sentence for unknown codes and plain errors", () => {
    expect(restoreErrorText("[snapshot_later] Something new\nsecret/path", t)).toBe(
      t("timeline.restoreFailed", { error: "Something new" }),
    );
    expect(restoreErrorText("Unknown session 3", t)).toBe(
      t("timeline.restoreFailed", { error: "Unknown session 3" }),
    );
    expect(restoreErrorText("[snapshot_linked] Link without a path", t)).toBe(
      t("timeline.restoreFailed", { error: "Link without a path" }),
    );
    expect(restoreErrorText(new Error("broken"), t)).toBe(
      t("timeline.restoreFailed", { error: "broken" }),
    );
    expect(restoreErrorText(42, t)).toBe(
      t("timeline.restoreFailed", { error: t("error.unknown") }),
    );
  });
});

describe("timelineErrorText", () => {
  it("uses the call's own sentence for an error without a known code", () => {
    expect(timelineErrorText("[unbound_session] No binding", "timeline.failed", t)).toBe(
      t("timeline.failed", { error: "No binding" }),
    );
    expect(timelineErrorText("git failed", "timeline.diffFailed", t)).toBe(
      t("timeline.diffFailed", { error: "git failed" }),
    );
  });
});
