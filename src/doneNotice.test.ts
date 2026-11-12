import { describe, expect, it } from "vitest";

import { changedFileCount, finishedWork, offersDoneNotice } from "./doneNotice";

describe("finishedWork", () => {
  it("is true when work ends in rest or waiting", () => {
    expect(finishedWork("working", "idle", null)).toBe(true);
    expect(finishedWork("working", "attention", "quiet")).toBe(true);
    expect(finishedWork("working", "attention", "bell")).toBe(true);
    expect(finishedWork("working", "attention", "exit")).toBe(true);
  });

  it("is false without prior work or while waiting at the gate", () => {
    expect(finishedWork("idle", "attention", "quiet")).toBe(false);
    expect(finishedWork("attention", "idle", null)).toBe(false);
    expect(finishedWork("idle", "working", null)).toBe(false);
    expect(finishedWork("working", "attention", "gate")).toBe(false);
  });
});

describe("changedFileCount", () => {
  const file = (path: string) => ({ path, status: " M", additions: 1, deletions: 0 });

  it("counts the files of a repository", () => {
    expect(
      changedFileCount({ isRepo: true, files: [file("a"), file("b")], diff: "", truncated: false }),
    ).toBe(2);
  });

  it("is 0 outside a repository and for malformed answers", () => {
    expect(changedFileCount({ isRepo: false, files: [], diff: "" })).toBe(0);
    expect(changedFileCount(null)).toBe(0);
    expect(changedFileCount("nope")).toBe(0);
  });
});

describe("offersDoneNotice", () => {
  const base: Parameters<typeof offersDoneNotice>[0] = {
    count: 3,
    sessionId: 1,
    state: "idle",
    activeId: 2,
    changesShown: true,
  };

  it("offers changes of another session", () => {
    expect(offersDoneNotice(base)).toBe(true);
  });

  it("stays away without changes or when the session works again", () => {
    expect(offersDoneNotice({ ...base, count: 0 })).toBe(false);
    expect(offersDoneNotice({ ...base, state: "working" })).toBe(false);
  });

  it("stays away when the session's changes are on screen", () => {
    expect(offersDoneNotice({ ...base, activeId: 1 })).toBe(false);
    expect(offersDoneNotice({ ...base, activeId: 1, changesShown: false })).toBe(true);
  });
});
