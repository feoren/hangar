import { describe, expect, it } from "vitest";

import {
  blockedFor,
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
    // Control characters are stripped before sending: what is left is blank.
    expect(canBroadcast("\x1b\x1b", [1])).toBe(false);
    expect(canBroadcast(" \x1b[A ", [1])).toBe(true);
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
      bracketedPaste: () => true,
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
    await expect(sendToTargets([], "hi", () => Promise.resolve())).resolves.toEqual([]);
  });

  const recorder = (name: string, bracketed: boolean, events: string[]): BroadcastTarget => ({
    bracketedPaste: () => bracketed,
    pasteAndWait: (text) => {
      events.push(`paste ${name} ${text}`);
      return Promise.resolve();
    },
    submit: () => {
      events.push(`enter ${name}`);
    },
  });
  const noPause = () => Promise.resolve();

  it("skips a target without bracketed paste when the text has a line break", async () => {
    const events: string[] = [];
    const safe = recorder("a", true, events);
    const raw = recorder("b", false, events);
    const text = "summary\necho marker";
    await expect(sendToTargets([safe, raw], text, noPause)).resolves.toEqual([raw]);
    expect(events).toEqual([`paste a ${text}`, "enter a"]);
  });

  it("skips a target without bracketed paste when the text hides characters", async () => {
    const events: string[] = [];
    const raw = recorder("b", false, events);
    await expect(sendToTargets([raw], "marker\u202etxt", noPause)).resolves.toEqual([raw]);
    expect(events).toEqual([]);
  });

  it("sends no lone Enter when nothing is left of the text after sanitizing", async () => {
    const events: string[] = [];
    const fake = recorder("a", true, events);
    await expect(sendToTargets([fake], "\x1b\x1b", noPause)).resolves.toEqual([]);
    await expect(sendToTargets([fake], " \x07 \n", noPause)).resolves.toEqual([]);
    expect(events).toEqual([]);
  });

  it("sends one plain line to a target without bracketed paste", async () => {
    const events: string[] = [];
    const raw = recorder("b", false, events);
    await expect(sendToTargets([raw], "/compact", noPause)).resolves.toEqual([]);
    expect(events).toEqual(["paste b /compact", "enter b"]);
  });
});

describe("blockedFor", () => {
  it("blocks only targets without bracketed paste, and only risky text", () => {
    const off = { bracketedPaste: () => false };
    const on = { bracketedPaste: () => true };
    expect(blockedFor(off, "one\ntwo")).toBe(true);
    expect(blockedFor(off, "one\rtwo")).toBe(true);
    expect(blockedFor(off, "one line")).toBe(false);
    expect(blockedFor(on, "one\ntwo")).toBe(false);
  });
});

describe("sendToTargets with a stalled paste", () => {
  it("sends no Enter where the paste did not arrive and rejects", async () => {
    const events: string[] = [];
    const target = (name: string, arrived: Promise<void>): BroadcastTarget => ({
      bracketedPaste: () => true,
      pasteAndWait: () => arrived,
      submit: () => {
        events.push(`enter ${name}`);
      },
    });
    const done = sendToTargets(
      [target("a", Promise.resolve()), target("b", Promise.reject(new Error("stalled")))],
      "hi",
      () => Promise.resolve(),
    );
    await expect(done).rejects.toThrow("stalled");
    await Promise.resolve();
    expect(events).toEqual(["enter a"]);
  });

  it("holds back a blocked target and still stops before the stalled one's Enter", async () => {
    const events: string[] = [];
    const target = (
      name: string,
      bracketed: boolean,
      arrived: () => Promise<void>,
    ): BroadcastTarget => ({
      bracketedPaste: () => bracketed,
      pasteAndWait: () => {
        events.push(`paste ${name}`);
        return arrived();
      },
      submit: () => {
        events.push(`enter ${name}`);
      },
    });
    const done = sendToTargets(
      [
        target("plain", false, () => Promise.resolve()),
        target("stalled", true, () => Promise.reject(new Error("stalled"))),
        target("ok", true, () => Promise.resolve()),
      ],
      "one\ntwo",
      () => Promise.resolve(),
    );
    await expect(done).rejects.toThrow("stalled");
    await Promise.resolve();
    expect(events).toEqual(["paste stalled", "paste ok", "enter ok"]);
  });
});
