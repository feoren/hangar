import { describe, expect, it } from "vitest";

import {
  chipWaits,
  edgeClass,
  exitedWithError,
  rowEdge,
  rowLook,
  runsAgent,
  sessionChip,
} from "./sessionChip";

const running = { status: "running", exitCode: null } as const;
const exited = (exitCode: number | null, stoppedByUser = false) =>
  ({ status: "exited", exitCode, stoppedByUser }) as const;

describe("sessionChip", () => {
  it("shows nothing for a resting shell or a starting session", () => {
    expect(sessionChip(running, "idle", null)).toBeNull();
    expect(sessionChip(running, "idle", null, false)).toBeNull();
    expect(sessionChip({ status: "starting", exitCode: null }, "idle", null, true)).toBeNull();
  });

  it("shows the pause bars for a resting agent only", () => {
    expect(sessionChip(running, "idle", null, true)).toBe("paused");
    expect(sessionChip(running, "working", null, true)).toBe("working");
    expect(sessionChip(running, "attention", "quiet", true)).toBe("done");
    expect(sessionChip(exited(0), "idle", null, true)).toBe("ended");
    expect(sessionChip(exited(2), "idle", null, true)).toBe("failed");
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
    expect(chipWaits("paused")).toBe(false);
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
  });

  it("gives an unseen error the failed look and a seen one none", () => {
    expect(rowLook(exited(1), "attention", "failed")).toBe("failed");
    expect(rowLook(exited(1), "idle", "failed")).toBeNull();
  });

  it("rests in the pause look with or without the pause chip", () => {
    expect(rowLook(running, "idle", "paused")).toBe("idle");
  });

  it("keeps starting and ended sessions plain", () => {
    expect(rowLook({ status: "starting" }, "idle", null)).toBeNull();
    expect(rowLook(exited(0), "idle", "ended")).toBeNull();
  });
});

describe("rowEdge", () => {
  it("follows the row's look while the session runs or waits", () => {
    expect(rowEdge(running, "working", "working")).toBe("working");
    expect(rowEdge(running, "idle", null)).toBe("idle");
    expect(rowEdge(running, "attention", "done")).toBe("done");
    expect(rowEdge(running, "attention", "asks")).toBe("asks");
    expect(rowEdge(exited(0), "attention", "done")).toBe("done");
  });

  it("marks a failed exit red, also while it still waits to be seen", () => {
    expect(rowEdge(exited(1), "attention", "failed")).toBe("failed");
    expect(rowEdge(exited(1), "idle", "failed")).toBe("failed");
  });

  it("gives starting and ended sessions their own edge", () => {
    expect(rowEdge({ status: "starting", exitCode: null }, "idle", null)).toBe("starting");
    expect(rowEdge(exited(0), "idle", "ended")).toBe("ended");
    expect(rowEdge(exited(1, true), "idle", "ended")).toBe("ended");
  });

  it("names the CSS class of an edge", () => {
    expect(edgeClass("working")).toBe("edge-working");
    expect(edgeClass("failed")).toBe("edge-failed");
  });
});

describe("runsAgent", () => {
  it("tells agents and other programs from plain shells", () => {
    expect(runsAgent("claude")).toBe(true);
    expect(runsAgent("C:\\tools\\codex.cmd")).toBe(true);
    expect(runsAgent("pwsh.exe")).toBe(false);
    expect(runsAgent("cmd")).toBe(false);
    expect(runsAgent("/bin/bash")).toBe(false);
    expect(runsAgent("wsl.exe")).toBe(false);
  });

  it("leaves out update sessions and unknown profiles", () => {
    expect(runsAgent("claude", "update")).toBe(false);
    expect(runsAgent("claude", "agent")).toBe(true);
    expect(runsAgent(undefined)).toBe(false);
  });
});
