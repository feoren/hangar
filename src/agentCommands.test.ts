import { describe, expect, it, vi } from "vitest";

import {
  asksQuestion,
  awaitsAnswer,
  commandBusy,
  commandLabel,
  commandMenuEntries,
  compactCommand,
  compactRepeated,
  COMPACT_REPEAT_MS,
  sendCommand,
  sessionCommands,
  showCompact,
  startsOver,
} from "./agentCommands";
import type { AgentCommand } from "./api";
import type { BroadcastTarget } from "./broadcast";
import { HIGH_SHARE } from "./contextUsage";
import { createTranslator } from "./i18n";

const t = createTranslator("en");

const compact: AgentCommand = {
  labelKey: "compact",
  text: "/compact",
  submit: true,
  confirm: false,
};
const clear: AgentCommand = { labelKey: "clear", text: "/clear", submit: true, confirm: true };
const own: AgentCommand = { label: "Pick a model", text: "/model ", submit: false, confirm: false };

const profiles = [
  { id: "claude", agentCommands: [compact, clear] },
  { id: "pwsh", agentCommands: [] },
  { id: "old" },
];

describe("sessionCommands", () => {
  it("offers the program's commands while the session runs", () => {
    expect(sessionCommands({ profileId: "claude", status: "running" }, profiles)).toEqual([
      compact,
      clear,
    ]);
  });

  it("offers none for ended, starting and update sessions", () => {
    expect(sessionCommands({ profileId: "claude", status: "exited" }, profiles)).toEqual([]);
    expect(sessionCommands({ profileId: "claude", status: "starting" }, profiles)).toEqual([]);
    expect(
      sessionCommands({ profileId: "claude", status: "running", kind: "update" }, profiles),
    ).toEqual([]);
  });

  it("offers none for programs without commands or unknown profiles", () => {
    expect(sessionCommands({ profileId: "pwsh", status: "running" }, profiles)).toEqual([]);
    expect(sessionCommands({ profileId: "old", status: "running" }, profiles)).toEqual([]);
    expect(sessionCommands({ profileId: "gone", status: "running" }, profiles)).toEqual([]);
  });
});

describe("labels and the submenu", () => {
  it("translates label keys and keeps own labels", () => {
    expect(commandLabel(compact, t)).toBe("Compact context");
    expect(commandLabel(compact, createTranslator("de"))).toBe("Kontext komprimieren");
    expect(commandLabel(own, t)).toBe("Pick a model");
  });

  it("lists the commands under the session's name, each with its text", () => {
    const entries = commandMenuEntries([compact, clear], "hangar", t);
    expect(entries[0]).toEqual({ type: "heading", key: "heading", label: "Commands for hangar" });
    expect(entries[2]).toEqual({
      type: "action",
      key: "command:/clear",
      label: "Clear context",
      detail: "/clear",
      command: clear,
    });
    expect(entries).toHaveLength(3);
  });
});

describe("compact button", () => {
  it("needs a compact command", () => {
    expect(compactCommand([clear, compact])).toBe(compact);
    expect(compactCommand([clear])).toBeNull();
    expect(showCompact(0.95, [clear], false)).toBe(false);
  });

  it("shows from the warning share on", () => {
    expect(showCompact(null, [compact], false)).toBe(false);
    expect(showCompact(HIGH_SHARE - 0.01, [compact], false)).toBe(false);
    expect(showCompact(HIGH_SHARE, [compact], false)).toBe(true);
    expect(showCompact(1, [compact], false)).toBe(true);
  });

  it("waits while the agent is busy", () => {
    expect(showCompact(0.9, [compact], true)).toBe(false);
  });

  it("counts a turn, a bell and a gate decision as busy, a quiet stop or an exit not", () => {
    expect(commandBusy("working", null)).toBe(true);
    expect(commandBusy("attention", "bell")).toBe(true);
    expect(commandBusy("attention", "gate")).toBe(true);
    expect(commandBusy("attention", "quiet")).toBe(false);
    expect(commandBusy("attention", "exit")).toBe(false);
    expect(commandBusy("idle", null)).toBe(false);
  });

  it("lets menu and quick access wait only for a possible question", () => {
    expect(awaitsAnswer("bell")).toBe(true);
    expect(awaitsAnswer("gate")).toBe(true);
    expect(awaitsAnswer("quiet")).toBe(false);
    expect(awaitsAnswer(null)).toBe(false);
  });
});

describe("asksQuestion", () => {
  const permission = [
    "  Bash command",
    "    git push",
    " Do you want to proceed?",
    " ❯ 1. Yes",
    "   2. No, and tell Claude what to do differently (esc)",
  ];
  const idlePrompt = ["● Done. The tests pass.", "", "> ", "  ? for shortcuts"];

  it("withholds a command from a permission dialog that rang no bell", () => {
    expect(asksQuestion("quiet", () => permission)).toBe(true);
    expect(asksQuestion(null, () => permission)).toBe(true);
  });

  it("sends at the idle input prompt", () => {
    expect(asksQuestion("quiet", () => idlePrompt)).toBe(false);
    expect(asksQuestion(null, () => [])).toBe(false);
  });

  it("decides on a bell or gate decision without reading the screen", () => {
    const screen = vi.fn(() => idlePrompt);
    expect(asksQuestion("bell", screen)).toBe(true);
    expect(asksQuestion("gate", screen)).toBe(true);
    expect(screen).not.toHaveBeenCalled();
  });
});

describe("compactRepeated", () => {
  it("swallows a second click within the repeat time, per session", () => {
    expect(compactRepeated(undefined, 0)).toBe(false);
    expect(compactRepeated(1000, 1000 + COMPACT_REPEAT_MS - 1)).toBe(true);
    expect(compactRepeated(1000, 1000 + COMPACT_REPEAT_MS)).toBe(false);
  });
});

describe("startsOver", () => {
  it("is true only for the clear and new label keys", () => {
    expect(startsOver(clear)).toBe(true);
    expect(startsOver({ ...clear, labelKey: "new", text: "/new" })).toBe(true);
    expect(startsOver(compact)).toBe(false);
    expect(startsOver({ label: "Wipe", text: "/wipe", submit: true, confirm: true })).toBe(false);
  });
});

describe("sendCommand", () => {
  const target = () => {
    const calls: string[] = [];
    const value: BroadcastTarget = {
      pasteAndWait: (text) => {
        calls.push(`paste:${text}`);
        return Promise.resolve();
      },
      submit: () => {
        calls.push("enter");
      },
    };
    return { calls, value };
  };

  it("pastes, waits and then submits, like Send to several", async () => {
    const { calls, value } = target();
    const pause = vi.fn(() => {
      calls.push("pause");
      return Promise.resolve();
    });
    await sendCommand(value, compact, pause);
    expect(calls).toEqual(["paste:/compact", "pause", "enter"]);
  });

  it("leaves a command without submit in the prompt", async () => {
    const { calls, value } = target();
    const pause = vi.fn(() => Promise.resolve());
    await sendCommand(value, own, pause);
    expect(calls).toEqual(["paste:/model "]);
    expect(pause).not.toHaveBeenCalled();
  });
});
