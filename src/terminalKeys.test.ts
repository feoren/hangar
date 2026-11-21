import { describe, expect, it } from "vitest";

import { AGENT_NEWLINE, dictationTail, terminalKeyAction } from "./terminalKeys";

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

  it("offers to delete a selection with Backspace or Delete in an agent session", () => {
    for (const name of ["Backspace", "Delete"]) {
      const erase = { ...key(name), key: name };
      expect(terminalKeyAction(erase, true, false, true)).toEqual({ kind: "deleteSelection" });
      // Without a selection, in a shell, on keyup and with a modifier: the usual key.
      expect(terminalKeyAction(erase, false, false, true)).toEqual({ kind: "terminal" });
      expect(terminalKeyAction(erase, true)).toEqual({ kind: "terminal" });
      expect(terminalKeyAction({ ...erase, type: "keyup" }, true, false, true)).toEqual({
        kind: "terminal",
      });
      expect(terminalKeyAction({ ...erase, ctrlKey: true }, true, false, true)).toEqual({
        kind: "terminal",
      });
      expect(terminalKeyAction({ ...erase, shiftKey: true }, true, false, true)).toEqual({
        kind: "terminal",
      });
      expect(terminalKeyAction({ ...erase, isComposing: true }, true, false, true)).toEqual({
        kind: "terminal",
      });
      expect(terminalKeyAction({ ...erase, altKey: true }, true, false, true)).toEqual({
        kind: "terminal",
      });
      expect(terminalKeyAction({ ...erase, metaKey: true }, true, false, true)).toEqual({
        kind: "terminal",
      });
    }
  });

  it("still copies with Ctrl+C when an agent session has a selection", () => {
    expect(terminalKeyAction(key("KeyC", { ctrl: true }), true, false, true)).toEqual({
      kind: "copy",
      clearSelection: true,
    });
  });

  it("keeps Ctrl+Shift+Space from the process only while dictation is on", () => {
    const chord = { ...key("Space", { ctrl: true, shift: true }), key: " " };
    expect(terminalKeyAction(chord, false)).toEqual({ kind: "terminal" });
    for (const type of ["keydown", "keypress", "keyup"]) {
      expect(terminalKeyAction({ ...chord, type }, false, false, false, "ctrlShiftSpace")).toEqual({
        kind: "app",
      });
    }
    // Ctrl+Space alone stays with the shell (set mark) and the IME.
    const ctrlSpace = { ...key("Space", { ctrl: true }), key: " " };
    expect(terminalKeyAction(ctrlSpace, false, false, false, "ctrlShiftSpace")).toEqual({
      kind: "terminal",
    });
    // Another key chosen: the chord is the program's again.
    expect(terminalKeyAction(chord, false, false, false, "f8")).toEqual({ kind: "terminal" });
  });

  it("keeps the chosen function key from the process only while dictation is on", () => {
    const f9 = { ...key("F9"), key: "F9" };
    for (const type of ["keydown", "keypress", "keyup"]) {
      expect(terminalKeyAction({ ...f9, type }, false, false, false, "f9")).toEqual({
        kind: "app",
      });
      // Off, or another key chosen: F9 goes to the program as before.
      expect(terminalKeyAction({ ...f9, type }, false)).toEqual({ kind: "terminal" });
      expect(terminalKeyAction({ ...f9, type }, false, false, false, "f10")).toEqual({
        kind: "terminal",
      });
    }
    // With a modifier it is another key for the program (Shift+F9 in Midnight Commander).
    expect(
      terminalKeyAction({ ...key("F9", { shift: true }), key: "F9" }, false, false, false, "f9"),
    ).toEqual({ kind: "terminal" });
  });

  it("leaves right Ctrl and every shortcut typed with it to the terminal", () => {
    const rightCtrl = { ...key("ControlRight", { ctrl: true }), key: "Control" };
    expect(terminalKeyAction(rightCtrl, false, false, false, "controlRight")).toEqual({
      kind: "terminal",
    });
    // Ctrl+C with the right Ctrl and no selection still interrupts the program.
    expect(
      terminalKeyAction(key("KeyC", { ctrl: true }), false, false, false, "controlRight"),
    ).toEqual({
      kind: "terminal",
    });
    expect(
      terminalKeyAction(key("KeyV", { ctrl: true }), false, false, false, "controlRight"),
    ).toEqual({
      kind: "paste",
    });
  });
});

describe("dictation tail", () => {
  const space = (type: string, mods: { ctrl?: boolean; shift?: boolean }, repeat = false) => ({
    ...key("Space", mods),
    key: " ",
    type,
    repeat,
  });

  it("swallows the repeats of Space after Shift went up first, until Space is released", () => {
    let armed = false;
    const step = (event: ReturnType<typeof space>) => {
      const result = dictationTail(armed, event, true);
      armed = result.armed;
      return result.swallow;
    };
    expect(step(space("keydown", { ctrl: true, shift: true }))).toBe(false);
    expect(step(space("keydown", { ctrl: true, shift: true }, true))).toBe(false);
    // Shift is up, Space still held: Ctrl+Space repeats, which xterm would send as NUL.
    expect(step(space("keydown", { ctrl: true }, true))).toBe(true);
    expect(step(space("keypress", { ctrl: true }, true))).toBe(true);
    // Ctrl up too: plain repeated spaces are swallowed as well.
    expect(step(space("keydown", {}, true))).toBe(true);
    expect(step(space("keyup", {}))).toBe(false);
    // Space released: the next press goes to the program again.
    expect(step(space("keydown", { ctrl: true }, true))).toBe(false);
  });

  it("leaves Space alone without a hold, off, or on a fresh press", () => {
    expect(dictationTail(false, space("keydown", { ctrl: true }, true), true)).toEqual({
      armed: false,
      swallow: false,
    });
    expect(dictationTail(true, space("keydown", { ctrl: true }, true), false)).toEqual({
      armed: false,
      swallow: false,
    });
    // A press without repeat means Space was released meanwhile (its keyup got lost).
    expect(dictationTail(true, space("keydown", { ctrl: true }), true)).toEqual({
      armed: false,
      swallow: false,
    });
    expect(dictationTail(true, { ...space("keydown", {}, true), code: "KeyA" }, true)).toEqual({
      armed: true,
      swallow: false,
    });
  });
});
