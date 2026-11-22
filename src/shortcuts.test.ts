import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator } from "./i18n";
import {
  CONTROL_HOLD_DELAY_MS,
  DICTATION_KEYS,
  defaultDictationKey,
  DICTATION_SHORTCUT,
  dictationKeyName,
  endsDictation,
  focusCycleStep,
  isDictationChord,
  isDictationKey,
  isDictationKeyEvent,
  KeyHold,
  startsDictation,
  type DictationKey,
  nextFocusZone,
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
  rowMoveStep,
  SHORTCUT_LABELS,
} from "./shortcuts";

function key(
  keyName: string,
  code: string,
  mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean; meta: boolean }> = {},
) {
  return {
    key: keyName,
    code,
    ctrlKey: mods.ctrl ?? true,
    shiftKey: mods.shift ?? false,
    altKey: mods.alt ?? false,
    metaKey: mods.meta ?? false,
  };
}

describe("matchShortcut", () => {
  it("switches sessions with Ctrl+Tab and Ctrl+Shift+Tab", () => {
    expect(matchShortcut(key("Tab", "Tab"))).toBe("nextSession");
    expect(matchShortcut(key("Tab", "Tab", { shift: true }))).toBe("prevSession");
  });

  it("sidebar and close only with Shift", () => {
    expect(matchShortcut(key("B", "KeyB", { shift: true }))).toBe("toggleSidebar");
    expect(matchShortcut(key("W", "KeyW", { shift: true }))).toBe("closeSession");
  });

  it("notes with Ctrl+Shift+N, Ctrl+N stays with the process", () => {
    expect(matchShortcut(key("N", "KeyN", { shift: true }))).toBe("quickNote");
    expect(matchShortcut(key("n", "KeyN"))).toBeNull();
    expect(matchShortcut(key("N", "KeyN", { shift: true, alt: true }))).toBeNull();
    expect(formatShortcut(SHORTCUT_LABELS.quickNote, createTranslator("de"))).toBe(
      "Strg+Umschalt+N",
    );
  });

  it("changes with Ctrl+Shift+D, Ctrl+D (end of input) stays with the shell", () => {
    expect(matchShortcut(key("D", "KeyD", { shift: true }))).toBe("toggleChanges");
    expect(matchShortcut(key("d", "KeyD"))).toBeNull();
    expect(matchShortcut(key("D", "KeyD", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("toggleChanges")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.toggleChanges, createTranslator("en"))).toBe(
      "Ctrl+Shift+D",
    );
  });

  it("waiting session and new session only with Shift", () => {
    expect(matchShortcut(key("A", "KeyA", { shift: true }))).toBe("nextWaiting");
    expect(matchShortcut(key("T", "KeyT", { shift: true }))).toBe("newSession");
    expect(matchShortcut(key("a", "KeyA"))).toBeNull(); // start of line
    expect(matchShortcut(key("t", "KeyT"))).toBeNull(); // transpose characters, fzf
    expect(matchShortcut(key("A", "KeyA", { shift: true, alt: true }))).toBeNull();
    const de = createTranslator("de");
    expect(formatShortcut(SHORTCUT_LABELS.nextWaiting, de)).toBe("Strg+Umschalt+A");
    expect(formatShortcut(SHORTCUT_LABELS.newSession, de)).toBe("Strg+Umschalt+T");
  });

  it("reopens the last opened session with Ctrl+Shift+O, Ctrl+O stays with the shell", () => {
    expect(matchShortcut(key("O", "KeyO", { shift: true }))).toBe("reopenRecent");
    expect(matchShortcut(key("o", "KeyO"))).toBeNull(); // operate-and-get-next
    expect(matchShortcut(key("O", "KeyO", { shift: true, alt: true }))).toBeNull();
    // Ctrl+Shift+T stays "New session".
    expect(matchShortcut(key("T", "KeyT", { shift: true }))).toBe("newSession");
    expect(ignoresRepeat("reopenRecent")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.reopenRecent, createTranslator("de"))).toBe(
      "Strg+Umschalt+O",
    );
    expect(formatShortcut(SHORTCUT_LABELS.reopenRecent, createTranslator("en"))).toBe(
      "Ctrl+Shift+O",
    );
  });

  it("search with Ctrl+Shift+F, Ctrl+F stays with the shell", () => {
    expect(matchShortcut(key("F", "KeyF", { shift: true }))).toBe("find");
    expect(matchShortcut(key("f", "KeyF"))).toBeNull(); // cursor forward
    expect(matchShortcut(key("F", "KeyF", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("find")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.find, createTranslator("de"))).toBe("Strg+Umschalt+F");
  });

  it("quick access with Ctrl+Shift+P, Ctrl+P stays with the shell", () => {
    expect(matchShortcut(key("P", "KeyP", { shift: true }))).toBe("launcher");
    expect(matchShortcut(key("p", "KeyP"))).toBeNull(); // history back
    expect(matchShortcut(key("P", "KeyP", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("launcher")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.launcher, createTranslator("en"))).toBe("Ctrl+Shift+P");
  });

  it("search across sessions with Ctrl+Shift+S, Ctrl+S stays with the shell", () => {
    expect(matchShortcut(key("S", "KeyS", { shift: true }))).toBe("searchAll");
    expect(matchShortcut(key("s", "KeyS"))).toBeNull(); // forward history search
    expect(matchShortcut(key("S", "KeyS", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("searchAll")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.searchAll, createTranslator("de"))).toBe(
      "Strg+Umschalt+S",
    );
  });

  it("home view with Ctrl+Shift+H, Ctrl+H (Backspace) stays with the terminal", () => {
    expect(matchShortcut(key("H", "KeyH", { shift: true }))).toBe("home");
    expect(matchShortcut(key("h", "KeyH"))).toBeNull();
    expect(ignoresRepeat("home")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.home, createTranslator("en"))).toBe("Ctrl+Shift+H");
  });

  it("overview with Ctrl+Shift+M, Ctrl+M (Enter) and Ctrl+Tab keep their meaning", () => {
    expect(matchShortcut(key("M", "KeyM", { shift: true }))).toBe("overview");
    expect(matchShortcut(key("m", "KeyM"))).toBeNull(); // Enter
    expect(matchShortcut(key("Tab", "Tab"))).toBe("nextSession");
    expect(ignoresRepeat("overview")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.overview, createTranslator("en"))).toBe("Ctrl+Shift+M");
  });

  it("repeats held keys only where repeating is wanted", () => {
    expect(ignoresRepeat("newSession")).toBe(true);
    expect(ignoresRepeat("toggleSidebar")).toBe(true);
    expect(ignoresRepeat("nextSession")).toBe(false);
    expect(ignoresRepeat("fontBigger")).toBe(false);
  });

  it("assigns no key combination twice", () => {
    const labels = Object.values(SHORTCUT_LABELS).map((s) => `${s.mods.join("+")}+${s.key}`);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("passes shortcuts of agent CLIs and shells through", () => {
    expect(matchShortcut(key("b", "KeyB"))).toBeNull(); // Claude Code, tmux
    expect(matchShortcut(key("c", "KeyC"))).toBeNull(); // interrupt
    expect(matchShortcut(key("w", "KeyW"))).toBeNull(); // delete word
    expect(matchShortcut(key("r", "KeyR"))).toBeNull(); // history search
    expect(matchShortcut(key("Tab", "Tab", { ctrl: false }))).toBeNull(); // completion
  });

  it("font size on German and US keyboards", () => {
    expect(matchShortcut(key("+", "BracketRight"))).toBe("fontBigger"); // DE
    expect(matchShortcut(key("=", "Equal"))).toBe("fontBigger"); // US
    expect(matchShortcut(key("+", "NumpadAdd"))).toBe("fontBigger");
    expect(matchShortcut(key("-", "Slash"))).toBe("fontSmaller"); // DE
    expect(matchShortcut(key("-", "NumpadSubtract"))).toBe("fontSmaller");
    expect(matchShortcut(key("0", "Digit0"))).toBe("fontReset");
  });

  it("settings with Ctrl+Comma", () => {
    expect(matchShortcut(key(",", "Comma"))).toBe("openSettings");
    expect(matchShortcut(key(",", "Comma", { ctrl: false }))).toBeNull();
    expect(matchShortcut(key(";", "Comma", { shift: true }))).toBeNull();
  });

  it("ignores Alt and Meta", () => {
    expect(matchShortcut(key("Tab", "Tab", { alt: true }))).toBeNull();
    expect(matchShortcut(key("B", "KeyB", { shift: true, meta: true }))).toBeNull();
  });
});

describe("formatShortcut", () => {
  it("names modifiers in the UI language", () => {
    const de = createTranslator("de");
    const en = createTranslator("en");
    expect(formatShortcut(SHORTCUT_LABELS.toggleSidebar, de)).toBe("Strg+Umschalt+B");
    expect(formatShortcut(SHORTCUT_LABELS.toggleSidebar, en)).toBe("Ctrl+Shift+B");
    expect(formatShortcut(SHORTCUT_LABELS.openSettings, de)).toBe("Strg+,");
    expect(formatShortcut(SHORTCUT_LABELS.openSettings, en)).toBe("Ctrl+,");
  });
});

describe("isReloadKey", () => {
  it("recognizes the reload keys of the WebView", () => {
    expect(isReloadKey(key("F5", "F5", { ctrl: false }))).toBe(true);
    expect(isReloadKey(key("F5", "F5"))).toBe(true);
    expect(isReloadKey(key("r", "KeyR"))).toBe(true);
    expect(isReloadKey(key("R", "KeyR", { shift: true }))).toBe(true);
  });

  it("leaves other keys alone", () => {
    expect(isReloadKey(key("r", "KeyR", { ctrl: false }))).toBe(false);
    expect(isReloadKey(key("r", "KeyR", { alt: true }))).toBe(false);
    expect(isReloadKey(key("F6", "F6", { ctrl: false }))).toBe(false);
  });

  it("is never also an app shortcut", () => {
    expect(matchShortcut(key("r", "KeyR"))).toBeNull();
    expect(matchShortcut(key("R", "KeyR", { shift: true }))).toBeNull();
  });
});

describe("rowMoveStep", () => {
  const alt = { ctrl: false, alt: true };

  it("moves a session row with Alt+Up and Alt+Down", () => {
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", alt))).toBe(-1);
    expect(rowMoveStep(key("ArrowDown", "ArrowDown", alt))).toBe(1);
  });

  it("ignores plain arrows and other modifiers", () => {
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", { ctrl: false }))).toBeNull();
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", { ctrl: true, alt: true }))).toBeNull();
    expect(
      rowMoveStep(key("ArrowDown", "ArrowDown", { ctrl: false, alt: true, shift: true })),
    ).toBeNull();
    expect(rowMoveStep(key("ArrowLeft", "ArrowLeft", alt))).toBeNull();
  });

  it("is never also an app shortcut", () => {
    expect(matchShortcut(key("ArrowUp", "ArrowUp", alt))).toBeNull();
  });
});

describe("focus cycle", () => {
  it("is F6 forward and Shift+F6 back, without other modifiers", () => {
    expect(focusCycleStep(key("F6", "F6", { ctrl: false }))).toBe(1);
    expect(focusCycleStep(key("F6", "F6", { ctrl: false, shift: true }))).toBe(-1);
    expect(focusCycleStep(key("F6", "F6"))).toBeNull();
    expect(focusCycleStep(key("F6", "F6", { ctrl: false, alt: true }))).toBeNull();
    expect(focusCycleStep(key("F5", "F5", { ctrl: false }))).toBeNull();
  });

  it("goes round terminal, list and editor", () => {
    const zones = ["terminal", "list", "editor"] as const;
    expect(nextFocusZone(zones, "terminal", 1)).toBe("list");
    expect(nextFocusZone(zones, "editor", 1)).toBe("terminal");
    expect(nextFocusZone(zones, "terminal", -1)).toBe("editor");
    expect(nextFocusZone(zones, null, 1)).toBe("terminal");
    expect(nextFocusZone(["terminal", "list"], "editor", 1)).toBe("terminal");
    expect(nextFocusZone([], "list", 1)).toBeNull();
  });
});

/** A hold of `dictationKey` that records its calls in order. */
function recorder(dictationKey: DictationKey) {
  const calls: string[] = [];
  const hold = new KeyHold(dictationKey, {
    onStart: () => calls.push("start"),
    onEnd: () => calls.push("end"),
    onDiscard: () => calls.push("discard"),
  });
  return { hold, calls };
}

describe("dictation hold", () => {
  const chord = (repeat = false) => ({ ...key(" ", "Space", { shift: true }), repeat });

  it("recognizes only Ctrl+Shift+Space", () => {
    expect(isDictationChord(key(" ", "Space", { shift: true }))).toBe(true);
    expect(isDictationChord(key(" ", "Space"))).toBe(false);
    expect(isDictationChord(key(" ", "Space", { shift: true, alt: true }))).toBe(false);
    expect(isDictationChord(key(" ", "Space", { shift: true, meta: true }))).toBe(false);
    expect(isDictationChord(key("S", "KeyS", { shift: true }))).toBe(false);
    // Not an app shortcut that fires on press: it only works as a hold.
    expect(matchShortcut(key(" ", "Space", { shift: true }))).toBeNull();
    expect(endsDictation("ctrlShiftSpace", { key: "Control", code: "ControlRight" })).toBe(true);
    expect(endsDictation("ctrlShiftSpace", { key: "a", code: "KeyA" })).toBe(false);
  });

  it("starts once despite key repeat and ends on release", () => {
    const { hold, calls } = recorder("ctrlShiftSpace");
    expect(hold.keydown(chord())).toBe(true);
    expect(hold.keydown(chord(true))).toBe(true);
    expect(hold.keydown(chord(true))).toBe(true);
    expect(hold.active).toBe(true);
    expect(hold.keyup({ key: " ", code: "Space" })).toBe(true);
    expect(hold.active).toBe(false);
    expect(calls).toEqual(["start", "end"]);
    // The keyup of the other keys afterwards does nothing.
    expect(hold.keyup({ key: "Shift", code: "ShiftLeft" })).toBe(false);
    expect(calls).toEqual(["start", "end"]);
  });

  it("ends on the release of Ctrl or Shift, and on blur", () => {
    for (const release of [
      { key: "Control", code: "ControlLeft" },
      { key: "Shift", code: "ShiftRight" },
    ]) {
      const { hold, calls } = recorder("ctrlShiftSpace");
      hold.keydown(chord());
      expect(hold.keyup({ key: "a", code: "KeyA" })).toBe(false);
      expect(hold.keyup(release)).toBe(true);
      expect(calls).toEqual(["start", "end"]);
    }
    const { hold, calls } = recorder("ctrlShiftSpace");
    hold.keydown(chord());
    hold.release();
    hold.release();
    expect(calls).toEqual(["start", "end"]);
  });

  it("does not start from a repeat alone and ignores other keys", () => {
    const { hold, calls } = recorder("ctrlShiftSpace");
    expect(hold.keydown(chord(true))).toBe(true);
    expect(hold.keydown({ ...key("a", "KeyA"), repeat: false })).toBe(false);
    // Typing meanwhile never discards a chord hold: only right Ctrl shares its key.
    hold.keydown(chord());
    hold.keydown({ ...key("a", "KeyA"), repeat: false });
    hold.pointer();
    expect(calls).toEqual(["start"]);
  });

  it("starts with right Ctrl, on the Mac with the chord", () => {
    expect(defaultDictationKey("windows")).toBe("controlRight");
    expect(defaultDictationKey("linux")).toBe("controlRight");
    expect(defaultDictationKey("mac")).toBe("ctrlShiftSpace");
  });

  it("knows the keys the settings offer and nothing else", () => {
    expect(DICTATION_KEYS).toEqual(["controlRight", "f8", "f9", "f10", "ctrlShiftSpace"]);
    for (const entry of DICTATION_KEYS) expect(isDictationKey(entry)).toBe(true);
    for (const entry of ["F8", "f11", "ControlRight", "", null, 8]) {
      expect(isDictationKey(entry)).toBe(false);
    }
  });

  it("names the key in the UI language", () => {
    expect(formatShortcut(DICTATION_SHORTCUT, createTranslator("de"))).toBe(
      "Strg+Umschalt+Leertaste",
    );
    expect(formatShortcut(DICTATION_SHORTCUT, createTranslator("en"))).toBe("Ctrl+Shift+Space");
    const de = createTranslator("de");
    expect(dictationKeyName("controlRight", de)).toBe("Rechte Strg-Taste");
    expect(dictationKeyName("controlRight", de, true)).toBe("die rechte Strg-Taste");
    expect(dictationKeyName("f9", de)).toBe("F9");
    expect(dictationKeyName("ctrlShiftSpace", de)).toBe("Strg+Umschalt+Leertaste");
    expect(dictationKeyName("controlRight", createTranslator("en"), true)).toBe(
      "the right Ctrl key",
    );
  });
});

describe("dictation with a function key", () => {
  const press = (code: string, mods: Parameters<typeof key>[2] = {}, repeat = false) => ({
    ...key(code, code, { ctrl: false, ...mods }),
    repeat,
  });

  it("takes only the chosen F key, and only without a modifier", () => {
    expect(startsDictation("f8", press("F8"))).toBe(true);
    expect(startsDictation("f9", press("F9"))).toBe(true);
    expect(startsDictation("f10", press("F10"))).toBe(true);
    expect(startsDictation("f8", press("F9"))).toBe(false);
    expect(startsDictation("f10", press("F1"))).toBe(false);
    for (const mods of [{ ctrl: true }, { shift: true }, { alt: true }, { meta: true }]) {
      expect(startsDictation("f8", press("F8", mods))).toBe(false);
    }
    // The terminal leaves the chosen key to the app, and only that one.
    expect(isDictationKeyEvent("f8", press("F8"))).toBe(true);
    expect(isDictationKeyEvent("f9", press("F8"))).toBe(false);
  });

  it("records at once while held, once despite repeat, and stops on release", () => {
    const { hold, calls } = recorder("f9");
    expect(hold.keydown(press("F9"))).toBe(true);
    expect(hold.keydown(press("F9", {}, true))).toBe(true);
    expect(hold.active).toBe(true);
    // Typing meanwhile neither ends nor discards: F9 belongs to dictation alone.
    expect(hold.keydown(press("KeyA"))).toBe(false);
    hold.pointer();
    expect(hold.keyup({ key: "a", code: "KeyA" })).toBe(false);
    expect(hold.keyup({ key: "F9", code: "F9" })).toBe(true);
    expect(calls).toEqual(["start", "end"]);
  });
});

describe("dictation with the right Ctrl key", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const rightCtrl = (repeat = false) => ({ ...key("Control", "ControlRight"), repeat });
  const rightCtrlUp = { key: "Control", code: "ControlRight" };
  const letter = (code: string) => ({ ...key(code.slice(3).toLowerCase(), code), repeat: false });

  it("records after a short hold and transcribes on release", () => {
    const { hold, calls } = recorder("controlRight");
    // Never its default: right Ctrl stays a normal key for the page and the terminal.
    expect(hold.keydown(rightCtrl())).toBe(false);
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS - 1);
    expect(calls).toEqual([]);
    expect(hold.keydown(rightCtrl(true))).toBe(false);
    vi.advanceTimersByTime(1);
    expect(calls).toEqual(["start"]);
    expect(hold.active).toBe(true);
    // Key repeat while held starts nothing more.
    hold.keydown(rightCtrl(true));
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 3);
    expect(hold.keyup(rightCtrlUp)).toBe(false);
    expect(calls).toEqual(["start", "end"]);
    expect(hold.active).toBe(false);
  });

  it("does nothing for a short tap", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS - 50);
    hold.keyup(rightCtrlUp);
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 2);
    expect(calls).toEqual([]);
  });

  it("leaves Ctrl+C to the shortcut before the recording starts", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(100);
    // The C goes on to the terminal or the app as usual.
    expect(hold.keydown(letter("KeyC"))).toBe(false);
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 3);
    hold.keydown(rightCtrl(true));
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 3);
    hold.keyup(rightCtrlUp);
    expect(calls).toEqual([]);
  });

  it("discards a running recording when another key follows, and only once", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    expect(hold.keydown(letter("KeyV"))).toBe(false);
    expect(calls).toEqual(["start", "discard"]);
    expect(hold.active).toBe(false);
    hold.keydown(letter("KeyV"));
    hold.keyup(rightCtrlUp);
    hold.release();
    expect(calls).toEqual(["start", "discard"]);
  });

  it("discards on a click or the wheel with right Ctrl down", () => {
    const first = recorder("controlRight");
    first.hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    first.hold.pointer();
    first.hold.keyup(rightCtrlUp);
    expect(first.calls).toEqual(["start", "discard"]);
    // Before the start, a click only keeps it from starting.
    const second = recorder("controlRight");
    second.hold.keydown(rightCtrl());
    second.hold.pointer();
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 2);
    second.hold.keyup(rightCtrlUp);
    expect(second.calls).toEqual([]);
  });

  it("works again on the next press after a shortcut", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    hold.keydown(letter("KeyC"));
    hold.keyup(rightCtrlUp);
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    hold.keyup(rightCtrlUp);
    expect(calls).toEqual(["start", "end"]);
  });

  it("recovers from a lost release of a spoiled hold on the next fresh press", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    hold.keydown(letter("KeyC"));
    // The keyup never came; a fresh press (no repeat) starts over.
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    expect(calls).toEqual(["start"]);
  });

  it("never starts from AltGr, the left Ctrl, or right Ctrl with another modifier", () => {
    const { hold, calls } = recorder("controlRight");
    // AltGr on Windows: a left Ctrl, then the right Alt with Ctrl and Alt set.
    hold.keydown({ ...key("Control", "ControlLeft"), repeat: false });
    hold.keydown({ ...key("AltGraph", "AltRight", { alt: true }), repeat: false });
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 2);
    hold.keyup({ key: "AltGraph", code: "AltRight" });
    hold.keyup({ key: "Control", code: "ControlLeft" });
    for (const mods of [{ shift: true }, { alt: true }, { meta: true }]) {
      expect(startsDictation("controlRight", key("Control", "ControlRight", mods))).toBe(false);
      hold.keydown({ ...key("Control", "ControlRight", mods), repeat: false });
      vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 2);
      hold.keyup(rightCtrlUp);
    }
    expect(calls).toEqual([]);
    // AltGr pressed while right Ctrl records: a shortcut, the recording is dropped.
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    hold.keydown({ ...key("Control", "ControlLeft"), repeat: false });
    expect(calls).toEqual(["start", "discard"]);
  });

  it("ends a running recording on blur and drops a waiting one", () => {
    const { hold, calls } = recorder("controlRight");
    hold.keydown(rightCtrl());
    hold.release();
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS * 2);
    expect(calls).toEqual([]);
    hold.keydown(rightCtrl());
    vi.advanceTimersByTime(CONTROL_HOLD_DELAY_MS);
    hold.release();
    expect(calls).toEqual(["start", "end"]);
  });

  it("is never taken from the terminal", () => {
    expect(isDictationKeyEvent("controlRight", key("Control", "ControlRight"))).toBe(false);
    expect(isDictationKeyEvent("controlRight", key("c", "KeyC"))).toBe(false);
  });
});
