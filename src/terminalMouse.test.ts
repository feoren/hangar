import { describe, expect, it, vi } from "vitest";

import { menuSelection, ProgramSelection, watchPresses } from "./programSelection";
import { guardSecondaryClick, holdsBackFromTerminal } from "./terminalMouse";

/** A mouse event without a DOM: Node has `Event`, but no `MouseEvent`. */
function press(type: "mousedown" | "mouseup", button: number): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "button", { value: button });
  return event;
}

/** An element with the guard and, behind it, a listener that stands for xterm's reporting. */
function guardedTerminal() {
  const element = new EventTarget();
  const focus = vi.fn();
  const remove = guardSecondaryClick(element, focus);
  const reported: string[] = [];
  const report = (event: Event) => {
    const button = "button" in event ? String(event.button) : "?";
    reported.push(`${event.type}:${button}`);
  };
  element.addEventListener("mousedown", report);
  element.addEventListener("mouseup", report);
  return { element, focus, remove, reported };
}

describe("terminal mouse", () => {
  it("holds back only the right button", () => {
    expect(holdsBackFromTerminal({ button: 2 })).toBe(true);
    expect(holdsBackFromTerminal({ button: 0 })).toBe(false);
    expect(holdsBackFromTerminal({ button: 1 })).toBe(false);
  });

  it("never lets a right click reach the program, so it cannot paste", () => {
    const { element, focus, reported } = guardedTerminal();
    const down = press("mousedown", 2);
    element.dispatchEvent(down);
    element.dispatchEvent(press("mouseup", 2));
    expect(reported).toEqual([]);
    expect(down.defaultPrevented).toBe(true);
    expect(focus).toHaveBeenCalledTimes(1);
  });

  it("leaves left and middle clicks to xterm", () => {
    const { element, focus, reported } = guardedTerminal();
    element.dispatchEvent(press("mousedown", 0));
    element.dispatchEvent(press("mouseup", 0));
    element.dispatchEvent(press("mousedown", 1));
    expect(reported).toEqual(["mousedown:0", "mouseup:0", "mousedown:1"]);
    expect(focus).not.toHaveBeenCalled();
  });

  it("keeps the selection through the guarded right click that opens the menu", () => {
    // Wired as in terminal.ts: press watcher, then the guard, then xterm. The xterm stand-in
    // reports the press to the program, which drops its selection, as Claude Code does.
    const element = new EventTarget();
    const selection = new ProgramSelection();
    watchPresses(element, selection);
    guardSecondaryClick(element, vi.fn());
    const program = { selection: "" };
    element.addEventListener("mousedown", () => {
      program.selection = "";
    });
    element.dispatchEvent(press("mousedown", 0));
    program.selection = "selected output";
    selection.clipboardWritten(program.selection);

    element.dispatchEvent(press("mousedown", 2));
    element.dispatchEvent(press("mouseup", 2));

    expect(program.selection).toBe("selected output");
    expect(menuSelection("", selection.current)).toBe("selected output");
  });

  it("can be removed again", () => {
    const { element, remove, reported } = guardedTerminal();
    remove();
    element.dispatchEvent(press("mousedown", 2));
    expect(reported).toEqual(["mousedown:2"]);
  });
});
