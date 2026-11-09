import { describe, expect, it } from "vitest";

import { terminalKeyAction } from "./terminalKeys";

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
  };
}

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
  });

  it("keeps F6 away from the process only while it moves the focus to the notes", () => {
    const f6 = { ...key("F6"), key: "F6" };
    expect(terminalKeyAction(f6, false)).toEqual({ kind: "terminal" });
    expect(terminalKeyAction(f6, false, true)).toEqual({ kind: "app" });
    expect(terminalKeyAction({ ...f6, shiftKey: true }, false, true)).toEqual({ kind: "app" });
    expect(terminalKeyAction({ ...f6, ctrlKey: true }, false, true)).toEqual({ kind: "terminal" });
  });
});
