import { describe, expect, it } from "vitest";

import { AGENT_NEWLINE, terminalKeyAction } from "./terminalKeys";

function key(code: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {}) {
  const letter = code.startsWith("Key") ? code.slice(3).toLowerCase() : code;
  return {
    type: "keydown",
    key: mods.shift === true ? letter.toUpperCase() : letter,
    code,
    ctrlKey: mods.ctrl === true,
    shiftKey: mods.shift === true,
    altKey: mods.alt === true,
    metaKey: false,
    isComposing: false,
  };
}

const enter = (mods: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {}) => ({
  ...key("Enter", mods),
  key: "Enter",
});

describe("terminal keys", () => {
  it("copies with Ctrl+C only when something is selected and clears the selection", () => {
    expect(terminalKeyAction(key("KeyC", { ctrl: true }), true)).toEqual({
      kind: "copy",
      clearSelection: true,
    });
    expect(terminalKeyAction(key("KeyC", { ctrl: true }), false)).toEqual({ kind: "terminal" });
  });

  it("copies with Ctrl+Shift+C and keeps the selection", () => {
    expect(terminalKeyAction(key("KeyC", { ctrl: true, shift: true }), false)).toEqual({
      kind: "copy",
      clearSelection: false,
    });
    expect(terminalKeyAction(key("KeyC", { ctrl: true, shift: true }), true)).toEqual({
      kind: "copy",
      clearSelection: false,
    });
  });

  it("leaves Ctrl+V and Ctrl+Shift+V to the native paste event", () => {
    expect(terminalKeyAction(key("KeyV", { ctrl: true }), false)).toEqual({ kind: "paste" });
    expect(terminalKeyAction(key("KeyV", { ctrl: true, shift: true }), true)).toEqual({
      kind: "paste",
    });
  });

  it("passes plain keys and other Ctrl combinations to the process", () => {
    expect(terminalKeyAction(key("KeyV"), false)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(key("KeyC"), true)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(key("KeyR", { ctrl: true }), false)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(key("KeyV", { ctrl: true, alt: true }), false)).toEqual({
      kind: "terminal",
    });
    expect(terminalKeyAction({ ...key("KeyV", { ctrl: true }), type: "keyup" }, false)).toEqual({
      kind: "terminal",
    });
  });

  it("hands app shortcuts to the app", () => {
    expect(terminalKeyAction(key("KeyB", { ctrl: true, shift: true }), false)).toEqual({
      kind: "app",
    });
    expect(terminalKeyAction(key("KeyF", { ctrl: true, shift: true }), false)).toEqual({
      kind: "app",
    });
    // Search across sessions and the overview.
    expect(terminalKeyAction(key("KeyS", { ctrl: true, shift: true }), false)).toEqual({
      kind: "app",
    });
    expect(terminalKeyAction(key("KeyM", { ctrl: true, shift: true }), false)).toEqual({
      kind: "app",
    });
    expect(terminalKeyAction(key("KeyS", { ctrl: true }), false)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(key("KeyM", { ctrl: true }), false)).toEqual({ kind: "terminal" });
  });

  it("keeps F6 away from the process only while it moves the focus to the notes", () => {
    const f6 = { ...key("F6"), key: "F6" };
    expect(terminalKeyAction(f6, false)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(f6, false, true)).toEqual({ kind: "app" });
    expect(terminalKeyAction({ ...f6, shiftKey: true }, false, true)).toEqual({ kind: "app" });
    expect(terminalKeyAction({ ...f6, ctrlKey: true }, false, true)).toEqual({ kind: "terminal" });
  });

  it("turns Shift+Enter into a new line in an agent session, sent once per press", () => {
    expect(AGENT_NEWLINE).toBe("\x1b\r");
    expect(terminalKeyAction(enter({ shift: true }), false, false, true)).toEqual({
      kind: "newline",
      send: AGENT_NEWLINE,
    });
    // xterm asks again for keypress and keyup: swallowed, nothing sent a second time.
    for (const type of ["keypress", "keyup"]) {
      expect(terminalKeyAction({ ...enter({ shift: true }), type }, false, false, true)).toEqual({
        kind: "newline",
        send: null,
      });
    }
  });

  it("leaves Enter, Shift+Enter in a shell and composing input to xterm", () => {
    expect(terminalKeyAction(enter(), false, false, true)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(enter({ shift: true }), false)).toEqual({ kind: "terminal" });
    expect(
      terminalKeyAction({ ...enter({ shift: true }), isComposing: true }, false, false, true),
    ).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(enter({ shift: true, ctrl: true }), false, false, true)).toEqual({
      kind: "terminal",
    });
    expect(terminalKeyAction(enter({ shift: true, alt: true }), false, false, true)).toEqual({
      kind: "terminal",
    });
  });
});
