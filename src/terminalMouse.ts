// The right mouse button in a terminal belongs to the app: it opens Hangar's terminal menu.
//
// When a program turns on mouse tracking (DECSET 1000 and up), xterm reports every button press
// to it, the right one included. Programs and ConPTY may answer a right click by pasting the
// clipboard, so the click pasted and opened the menu at once. Pasting is Ctrl+V's job (or the
// menu's "Paste"); the right button is never reported.

/** `MouseEvent.button` of the secondary (usually right) button. */
const SECONDARY_BUTTON = 2;

/** Whether a mouse event must not reach xterm's mouse reporting. */
export function holdsBackFromTerminal(event: Pick<MouseEvent, "button">): boolean {
  return event.button === SECONDARY_BUTTON;
}

/**
 * Stops right button presses before xterm sees them. Attach to xterm's element in the capture
 * phase, so it runs before xterm's own listeners on that element. The `contextmenu` event still
 * fires, so the app's menu opens as before. Since xterm no longer focuses the terminal on such a
 * press, `focus` does that instead. Returns a function that removes the guard.
 */
export function guardSecondaryClick(element: EventTarget, focus: () => void): () => void {
  const onPress = (event: Event) => {
    if (!("button" in event) || typeof event.button !== "number") return;
    if (!holdsBackFromTerminal({ button: event.button })) return;
    event.stopImmediatePropagation();
    if (event.type === "mousedown") {
      // Like xterm: no text selection by the browser, the keyboard stays in the terminal.
      event.preventDefault();
      focus();
    }
  };
  element.addEventListener("mousedown", onPress, { capture: true });
  element.addEventListener("mouseup", onPress, { capture: true });
  return () => {
    element.removeEventListener("mousedown", onPress, { capture: true });
    element.removeEventListener("mouseup", onPress, { capture: true });
  };
}
