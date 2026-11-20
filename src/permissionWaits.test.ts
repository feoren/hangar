import { describe, expect, it } from "vitest";

import { asksQuestion } from "./agentCommands";
import type { AttentionReason, AttentionState } from "./attention";
import { finishedWork } from "./doneNotice";
import { statusPhrase } from "./format";
import { phaseEnd } from "./messageQueue";
import { answersDialog, parsePermissionWaiting, withPermission } from "./permissionWaits";
import { sessionChip } from "./sessionChip";

const running = { status: "running", exitCode: null, stoppedByUser: false } as const;

describe("parsePermissionWaiting", () => {
  it("keeps positive session ids and drops everything else", () => {
    expect([...parsePermissionWaiting([3, 1, -1, 0, 1.5, "2", null])]).toEqual([3, 1]);
    expect(parsePermissionWaiting({ 0: 1 }).size).toBe(0);
    expect(parsePermissionWaiting(undefined).size).toBe(0);
  });
});

describe("withPermission", () => {
  it("shows a session with a dialog as waiting for the person, a gate decision first", () => {
    const view = {
      attention: new Map<number, AttentionState>([
        [1, "working"],
        [2, "attention"],
      ]),
      reasons: new Map<number, AttentionReason>([[2, "gate"]]),
    };
    const shown = withPermission(view, new Set([1, 2, 3]));
    expect(shown.attention.get(1)).toBe("attention");
    expect(shown.reasons.get(1)).toBe("permission");
    expect(shown.reasons.get(2)).toBe("gate");
    expect(shown.reasons.get(3)).toBe("permission");
    // Unchanged without such sessions, and the input stays as it was.
    expect(withPermission(view, new Set())).toBe(view);
    expect(view.reasons.get(1)).toBeUndefined();
  });
});

describe("the permission reason", () => {
  it("is a sure question: no command, no queued message, no done notice", () => {
    let screenRead = false;
    expect(
      asksQuestion("permission", () => {
        screenRead = true;
        return [];
      }),
    ).toBe(true);
    // The signal decides without reading the screen.
    expect(screenRead).toBe(false);
    expect(phaseEnd("working", "attention", "permission", "quiet", false)).toBe("asks");
    expect(phaseEnd("working", "idle", null, "quiet", true)).toBe("asks");
    expect(finishedWork("working", "attention", "permission")).toBe(false);
    expect(sessionChip(running, "attention", "permission", true)).toBe("asks");
    expect(statusPhrase(running, "attention", "permission")).toBe("permission");
  });
});

describe("answersDialog", () => {
  it("takes only Enter and Esc as an answer, digits, arrows and letters not", () => {
    for (const key of ["\r", "\x1b"]) expect(answersDialog(key), key).toBe(true);
    // A digit may be no choice of the dialog or text in a form; clearing then would be too early.
    for (const key of ["1", "3", "9", "\x1b[A", "\x1b[B", "a", "0", "\t", "12"]) {
      expect(answersDialog(key), key).toBe(false);
    }
  });
});
