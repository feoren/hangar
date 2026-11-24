// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";

// Outside Tauri a real channel cannot be made; the test calls `onmessage` as the backend would.
vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  class TestChannel {
    onmessage: (event: SessionEvent) => void = () => undefined;
  }
  return { ...actual, Channel: TestChannel };
});

import { pasteWarning } from "./appPaste";
import type { SessionEvent } from "./api";
import { blockedFor } from "./broadcast";
import type { ShellKind } from "./pathQuote";
import { SessionTerminal } from "./terminal";
import { trustsBracketedPaste } from "./terminalModes";

/** Feeds `event` to the terminal as the backend would, and waits until xterm parsed it. */
async function feed(terminal: SessionTerminal, event: SessionEvent): Promise<void> {
  const channel = terminal.createChannel();
  channel.onmessage(event);
  await flush(terminal);
}

/** Resolves once xterm has parsed everything written so far. */
function flush(terminal: SessionTerminal): Promise<void> {
  return new Promise((resolve) => {
    terminal.term.write("", resolve);
  });
}

function output(data: string): SessionEvent {
  return { type: "output", data, bytes: data.length };
}

function session(kind: ShellKind): SessionTerminal {
  const terminal = new SessionTerminal();
  terminal.shellKind = () => kind;
  return terminal;
}

const TWO_LINES = "echo marker-one\necho marker-two";

describe("bracketed paste trust", () => {
  it("trusts the mode only for programs that turn it on themselves", () => {
    expect(trustsBracketedPaste("agent")).toBe(true);
    expect(trustsBracketedPaste("posix")).toBe(true);
    expect(trustsBracketedPaste("wsl")).toBe(true);
    expect(trustsBracketedPaste("cmd")).toBe(false);
    expect(trustsBracketedPaste("powershell")).toBe(false);
  });

  it("keeps warning in cmd when program output turned bracketed paste on", async () => {
    const terminal = session("cmd");
    await feed(terminal, output("marker\x1b[?2004h"));
    expect(terminal.term.modes.bracketedPasteMode).toBe(true);
    expect(terminal.bracketedPaste()).toBe(false);
    expect(pasteWarning(TWO_LINES, terminal.bracketedPaste())?.runsLines).toBe(true);
    expect(blockedFor(terminal, TWO_LINES)).toBe(true);
    terminal.dispose();
  });

  it("keeps warning in PowerShell when program output turned bracketed paste on", async () => {
    const terminal = session("powershell");
    await feed(terminal, output("\x1b[?2004h"));
    expect(terminal.bracketedPaste()).toBe(false);
    expect(blockedFor(terminal, TWO_LINES)).toBe(true);
    terminal.dispose();
  });

  it("follows the mode for an agent", async () => {
    const terminal = session("agent");
    await feed(terminal, output("\x1b[?2004h"));
    expect(terminal.bracketedPaste()).toBe(true);
    terminal.dispose();
  });
});

describe("input modes after the program ends", () => {
  const CHILD_MODES = "\x1b[?2004h\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[?1h\x1b=";

  it("clears the modes a crashed program left on when it exits", async () => {
    const terminal = session("agent");
    await feed(terminal, output(CHILD_MODES));
    expect(terminal.term.modes.bracketedPasteMode).toBe(true);
    expect(terminal.term.modes.mouseTrackingMode).toBe("any");
    await feed(terminal, { type: "exit", code: 1 });
    const modes = terminal.term.modes;
    expect(modes.bracketedPasteMode).toBe(false);
    expect(modes.mouseTrackingMode).toBe("none");
    expect(modes.sendFocusMode).toBe(false);
    expect(modes.applicationCursorKeysMode).toBe(false);
    expect(modes.applicationKeypadMode).toBe(false);
    expect(terminal.bracketedPaste()).toBe(false);
    terminal.dispose();
  });

  it("clears them when a failed resume starts a new process on the same channel", async () => {
    const terminal = session("agent");
    const channel = terminal.createChannel();
    channel.onmessage(output(CHILD_MODES));
    channel.onmessage({ type: "resumeFallback", code: 1 });
    await flush(terminal);
    expect(terminal.term.modes.bracketedPasteMode).toBe(false);
    expect(terminal.term.modes.mouseTrackingMode).toBe("none");
    channel.onmessage(output("\x1b[?2004h"));
    await flush(terminal);
    expect(terminal.bracketedPaste()).toBe(true);
    terminal.dispose();
  });

  it("clears them on restart before the new process starts", async () => {
    const terminal = session("agent");
    await feed(terminal, output(CHILD_MODES));
    terminal.markRestarted();
    await flush(terminal);
    expect(terminal.term.modes.bracketedPasteMode).toBe(false);
    expect(terminal.term.modes.mouseTrackingMode).toBe("none");
    // The new process turns the mode on again itself.
    await feed(terminal, output("\x1b[?2004h"));
    expect(terminal.bracketedPaste()).toBe(true);
    terminal.dispose();
  });
});
