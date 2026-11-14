import { describe, expect, it, vi } from "vitest";

import { doneFileCount, finishedWork, offersDoneNotice, parseChangedCount } from "./doneNotice";

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

describe("parseChangedCount", () => {
  it("takes the number of changed files", () => {
    expect(parseChangedCount(2)).toBe(2);
  });

  it("is 0 outside a repository and for malformed answers", () => {
    expect(parseChangedCount(null)).toBe(0);
    expect(parseChangedCount(0)).toBe(0);
    expect(parseChangedCount(-1)).toBe(0);
    expect(parseChangedCount(1.5)).toBe(0);
    expect(parseChangedCount("3")).toBe(0);
    expect(parseChangedCount({ isRepo: true, files: [] })).toBe(0);
  });
});

describe("doneFileCount", () => {
  it("asks the backend for the count of the session's folder only", async () => {
    const ask = vi.fn((cwd: string) => Promise.resolve<unknown>(cwd === "C:\\app" ? 3 : null));
    await expect(doneFileCount("C:\\app", ask)).resolves.toBe(3);
    expect(ask).toHaveBeenCalledWith("C:\\app");
  });

  it("offers nothing outside a repository or without changes", async () => {
    for (const answer of [null, 0]) {
      const count = await doneFileCount("/home/ada/notes", () => Promise.resolve<unknown>(answer));
      expect(count).toBe(0);
      expect(
        offersDoneNotice({
          count,
          sessionId: 1,
          state: "idle",
          activeId: 2,
          changesShown: false,
        }),
      ).toBe(false);
    }
  });

  it("passes a git error on, so the caller shows nothing", async () => {
    await expect(
      doneFileCount("/x", () => Promise.reject(new Error("git not found"))),
    ).rejects.toThrow("git not found");
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
