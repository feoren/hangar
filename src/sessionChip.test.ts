import { describe, expect, it } from "vitest";

import { chipWaits, exitedWithError, rowLook, sessionChip } from "./sessionChip";

const running = { status: "running", exitCode: null } as const;
const exited = (exitCode: number | null, stoppedByUser = false) =>
  ({ status: "exited", exitCode, stoppedByUser }) as const;

describe("sessionChip", () => {
  it("shows nothing for a calm running session", () => {
    expect(sessionChip(running, "idle", null)).toBeNull();
    expect(sessionChip({ status: "starting", exitCode: null }, "idle", null)).toBeNull();
  });

  it("shows the working dots while the program works", () => {
    expect(sessionChip(running, "working", null)).toBe("working");
  });

  it("tells done from a bell", () => {
    expect(sessionChip(running, "attention", "quiet")).toBe("done");
    expect(sessionChip(running, "attention", "bell")).toBe("asks");
    expect(sessionChip(exited(0), "attention", "exit")).toBe("done");
  });

  it("shows a pending decision as a question", () => {
    expect(sessionChip(running, "attention", "gate")).toBe("asks");
  });

  it("marks a failed exit, also once it was seen", () => {
    expect(sessionChip(exited(1), "attention", "exit")).toBe("failed");
    expect(sessionChip(exited(1), "idle", null)).toBe("failed");
  });

  it("keeps an ordinary or unknown exit neutral", () => {
    expect(sessionChip(exited(0), "idle", null)).toBe("ended");
    expect(sessionChip(exited(null), "idle", null)).toBe("ended");
  });

  it("a process the user stopped is no error", () => {
    expect(exitedWithError(exited(1, true))).toBe(false);
    expect(sessionChip(exited(1, true), "idle", null)).toBe("ended");
  });

  it("only done and asks count as waiting", () => {
    expect(chipWaits("done")).toBe(true);
    expect(chipWaits("asks")).toBe(true);
    expect(chipWaits("failed")).toBe(false);
    expect(chipWaits("working")).toBe(false);
    expect(chipWaits(null)).toBe(false);
  });
});

describe("rowLook", () => {
  it("tells working from pause in a running session", () => {
    expect(rowLook(running, "working", "working")).toBe("working");
    expect(rowLook(running, "idle", null)).toBe("idle");
  });

  it("gives questions and decisions the asks look, any other waiting the done look", () => {
    expect(rowLook(running, "attention", "asks")).toBe("asks");
    expect(rowLook(running, "attention", "done")).toBe("done");
    expect(rowLook(exited(1), "attention", "failed")).toBe("done");
  });

  it("keeps starting and ended sessions plain", () => {
    expect(rowLook({ status: "starting" }, "idle", null)).toBeNull();
    expect(rowLook(exited(0), "idle", "ended")).toBeNull();
  });
});
