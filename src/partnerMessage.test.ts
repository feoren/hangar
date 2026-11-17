import { describe, expect, it } from "vitest";

import {
  MESSAGE_MAX_CHARS,
  canSendMessage,
  deliverMessage,
  deliveryPlan,
  holdUntilPaused,
  parsePartnerTargets,
  type DeliveryDeps,
  type PartnerSession,
} from "./partnerMessage";

const commands: Record<string, string> = { claude: "claude", codex: "codex", pwsh: "pwsh.exe" };
const commandOf = (id: string) => commands[id];
const all = new Set(["claude", "codex", "pwsh"]);

function session(id: number, profileId: string, running = true): PartnerSession {
  return { id, profileId, running };
}

describe("cross-project message delivery", () => {
  it("goes to the one running agent session, never to a shell", () => {
    const sessions = [session(1, "pwsh"), session(2, "claude"), session(3, "codex", false)];
    expect(deliveryPlan(sessions, commandOf, all, null)).toEqual({ kind: "one", id: 2 });
  });

  it("lets the person choose among several", () => {
    const sessions = [session(1, "claude"), session(2, "codex")];
    expect(deliveryPlan(sessions, commandOf, all, null)).toEqual({ kind: "choose", ids: [1, 2] });
  });

  it("starts the program that worked there last", () => {
    const sessions = [session(1, "claude", false), session(2, "codex", false), session(3, "pwsh")];
    expect(deliveryPlan(sessions, commandOf, all, "claude")).toEqual({
      kind: "start",
      profileId: "codex",
    });
    // A program that cannot start any more is skipped.
    expect(deliveryPlan(sessions, commandOf, new Set(["claude"]), null)).toEqual({
      kind: "start",
      profileId: "claude",
    });
  });

  it("falls back to the next program, but never to a shell", () => {
    expect(deliveryPlan([], commandOf, all, "codex")).toEqual({
      kind: "start",
      profileId: "codex",
    });
    expect(deliveryPlan([], commandOf, all, "pwsh")).toEqual({ kind: "start", profileId: null });
    expect(deliveryPlan([], commandOf, new Set(), "codex")).toEqual({
      kind: "start",
      profileId: null,
    });
  });

  it("sends text that is not blank and not too long", () => {
    expect(canSendMessage("  \n ")).toBe(false);
    expect(canSendMessage("x".repeat(MESSAGE_MAX_CHARS))).toBe(true);
    expect(canSendMessage("x".repeat(MESSAGE_MAX_CHARS + 1))).toBe(false);
  });

  it("holds the message while the target works", async () => {
    let checks = 0;
    const waits: number[] = [];
    const result = await holdUntilPaused(
      { working: () => ++checks < 3, gone: () => false },
      (ms) => {
        waits.push(ms);
        return Promise.resolve();
      },
    );
    expect(result).toBe("paused");
    expect(waits).toHaveLength(2);
    await expect(
      holdUntilPaused({ working: () => true, gone: () => true }, () => Promise.resolve()),
    ).resolves.toBe("gone");
  });

  it("reads the backend's targets and refuses malformed ones", () => {
    const answer = {
      sender: "app",
      partner: { name: "web", folder: "/w" },
      sessions: [{ id: 3, profileId: "claude", running: true }],
    };
    expect(parsePartnerTargets(answer)).toEqual(answer);
    expect(parsePartnerTargets({ ...answer, sessions: [{ id: -1 }] })).toBeNull();
    expect(parsePartnerTargets(null)).toBeNull();
  });
});

describe("deliverMessage", () => {
  function deps(over: Partial<DeliveryDeps> = {}) {
    const calls: string[] = [];
    const base: DeliveryDeps = {
      working: () => false,
      gone: () => false,
      onHeld: () => calls.push("held"),
      ready: () => Promise.resolve("ready"),
      bracketedPaste: () => true,
      prepare: () => {
        calls.push("prepare");
        return Promise.resolve("HEAD\ntext");
      },
      paste: (text) => {
        calls.push(`paste:${text}`);
        return Promise.resolve();
      },
      submit: () => calls.push("submit"),
      wait: () => Promise.resolve(),
    };
    return { calls, deps: { ...base, ...over } };
  }

  it("pastes the prepared text and presses Enter only on request", async () => {
    const plain = deps();
    expect(await deliverMessage(plain.deps, false, 0)).toEqual({ kind: "pasted" });
    expect(plain.calls).toEqual(["prepare", "paste:HEAD\ntext"]);
    const sent = deps();
    await deliverMessage(sent.deps, true, 0);
    expect(sent.calls).toEqual(["prepare", "paste:HEAD\ntext", "submit"]);
  });

  it("holds while the target works and says so once", async () => {
    let busy = 2;
    const run = deps({
      working: () => busy-- > 0,
    });
    expect(await deliverMessage(run.deps, false, 0)).toEqual({ kind: "pasted" });
    expect(run.calls[0]).toBe("held");
    expect(run.calls.filter((c) => c === "held")).toHaveLength(1);
  });

  it("leaves the text undelivered and pastes nothing when the target is gone or not ready", async () => {
    const gone = deps({ gone: () => true });
    expect(await deliverMessage(gone.deps, true, 0)).toEqual({ kind: "gone" });
    expect(gone.calls).toEqual([]);
    const late = deps({ ready: () => Promise.resolve("notReady") });
    expect(await deliverMessage(late.deps, true, 0)).toEqual({ kind: "notReady" });
    expect(late.calls).toEqual([]);
    const vanished = deps({ ready: () => Promise.resolve("gone") });
    expect(await deliverMessage(vanished.deps, true, 0)).toEqual({ kind: "gone" });
  });

  it("pastes nothing when bracketed paste went off during the backend's round trip", async () => {
    let on = true;
    const run = deps({
      bracketedPaste: () => on,
      prepare: () => {
        on = false;
        return Promise.resolve("HEAD\ntext");
      },
    });
    expect(await deliverMessage(run.deps, true, 0)).toEqual({ kind: "notReady" });
    expect(run.calls.some((call) => call.startsWith("paste:"))).toBe(false);
    expect(run.calls).not.toContain("submit");
  });

  it("reports a refusal and a failed paste, and presses no Enter then", async () => {
    const refusal = new Error("not a partner");
    const refused = deps({ prepare: () => Promise.reject(refusal) });
    expect(await deliverMessage(refused.deps, true, 0)).toEqual({
      kind: "refused",
      error: refusal,
    });
    const failed = deps({ paste: () => Promise.reject(new Error("no")) });
    expect(await deliverMessage(failed.deps, true, 0)).toEqual({ kind: "pasteFailed" });
    expect(failed.calls).toEqual(["prepare"]);
  });

  it("presses no Enter when the target went away during the delay", async () => {
    let gone = false;
    const run = deps({ gone: () => gone, wait: () => ((gone = true), Promise.resolve()) });
    expect(await deliverMessage(run.deps, true, 0)).toEqual({ kind: "pasted" });
    expect(run.calls).not.toContain("submit");
  });
});
