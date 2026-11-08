import { describe, expect, it } from "vitest";

import {
  broadcastCandidates,
  canBroadcast,
  defaultTargets,
  sendToTargets,
  type BroadcastTarget,
} from "./broadcast";
import type { SessionView } from "./sessions";

function session(id: number, profileId: string, extra: Partial<SessionView> = {}): SessionView {
  return {
    id,
    profileId,
    title: profileId,
    cwd: "C:\\",
    status: "running",
    exitCode: null,
    createdAt: 0,
    ...extra,
  };
}

const commands: Record<string, string> = {
  claude: "claude",
  codex: "C:\\Users\\a\\AppData\\Roaming\\npm\\codex.cmd",
  pwsh: "pwsh.exe",
  bash: "/bin/bash",
};
const commandOf = (profileId: string): string | undefined => commands[profileId];

describe("broadcastCandidates", () => {
  it("keeps running sessions and leaves out exited and update sessions", () => {
    const sessions = [
      session(1, "claude"),
      session(2, "codex", { status: "exited" }),
      session(3, "codex", { kind: "update" }),
      session(4, "pwsh", { status: "starting" }),
    ];
    expect(broadcastCandidates(sessions).map((s) => s.id)).toEqual([1, 4]);
  });
});

describe("defaultTargets", () => {
  it("checks the running agents, not the shells", () => {
    const sessions = [
      session(1, "claude"),
      session(2, "pwsh"),
      session(3, "codex"),
      session(4, "bash"),
      session(5, "claude", { status: "exited" }),
    ];
    expect(defaultTargets(sessions, commandOf)).toEqual([1, 3]);
  });

  it("treats unknown programs as agents", () => {
    expect(defaultTargets([session(7, "custom")], commandOf)).toEqual([7]);
  });
});

describe("canBroadcast", () => {
  it("needs text and at least one session", () => {
    expect(canBroadcast("Write tests", [1])).toBe(true);
    expect(canBroadcast("   ", [1])).toBe(false);
    expect(canBroadcast("Write tests", [])).toBe(false);
  });
});

describe("sendToTargets", () => {
  it("sends Enter to each target only after its paste arrived and the pause passed", async () => {
    const events: string[] = [];
    let releaseSlow: () => void = () => undefined;
    const slowPaste = new Promise<void>((resolve) => {
      releaseSlow = resolve;
    });
    const target = (name: string, arrived: Promise<void>): BroadcastTarget => ({
      pasteAndWait: (text) => {
        events.push(`paste ${name} ${text}`);
        return arrived;
      },
      submit: () => {
        events.push(`enter ${name}`);
      },
    });
    const done = sendToTargets(
      [target("a", Promise.resolve()), target("b", slowPaste)],
      "hi",
      () => {
        events.push("pause");
        return Promise.resolve();
      },
    );
    // Both pastes go out at once; only the target whose paste arrived gets Enter.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(events).toEqual(["paste a hi", "paste b hi", "pause", "enter a"]);
    releaseSlow();
    await done;
    expect(events).toEqual(["paste a hi", "paste b hi", "pause", "enter a", "pause", "enter b"]);
  });

  it("does nothing without targets", async () => {
    await expect(sendToTargets([], "hi", () => Promise.resolve())).resolves.toBeUndefined();
  });
});
