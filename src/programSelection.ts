// Text a program selected itself, for the terminal menu's "Copy" and "Add selection to note".
//
// With mouse tracking on (Claude Code's full screen mode, for example), a drag in the terminal
// goes to the program, not to xterm: the program draws its own highlight and xterm has no
// selection at all. Such programs copy their selection on release through OSC 52. Hangar keeps
// that text as the program's selection, but only when a mouse press in the terminal led to it,
// so a copy started by a command (Claude Code's /copy) does not count as a selection.

import { holdsBackFromTerminal } from "./terminalMouse";

/** OSC 52 payloads above this length are ignored (about 3 MB of text). */
const MAX_PAYLOAD = 4 * 1024 * 1024;

/** `Pc;Pd`: selection targets (may be empty), then base64 data, or `?` for a query. */
const OSC52 = /^[cpqs0-7]*;([A-Za-z0-9+/]*={0,2})$/;

/**
 * The text of an OSC 52 clipboard write (`ESC ] 52 ; Pc ; Pd BEL`, here `Pc;Pd`), decoded as
 * UTF-8. An empty string means the program cleared the clipboard; `null` for queries, invalid or
 * oversized data.
 */
export function clipboardWriteText(data: string): string | null {
  if (data.length > MAX_PAYLOAD) return null;
  const match = OSC52.exec(data);
  const encoded = match?.[1];
  if (encoded === undefined) return null;
  try {
    const binary = atob(encoded);
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

/**
 * The selection a program made itself, as far as Hangar can know it. A left or middle press in
 * the terminal starts a new gesture (the program drops its old selection), a typed key ends it.
 * The right button opens Hangar's menu and never reaches the program, so it changes nothing.
 */
export class ProgramSelection {
  private armed = false;
  private text = "";

  /** Mouse press in the terminal, with `MouseEvent.button`. */
  pointerDown(button: number): void {
    if (holdsBackFromTerminal({ button })) return;
    this.armed = true;
    this.text = "";
  }

  /** A key that went to the program. */
  keyTyped(): void {
    this.armed = false;
    this.text = "";
  }

  /** The program wrote `text` to the clipboard (OSC 52). */
  clipboardWritten(text: string): void {
    if (this.armed) this.text = text;
  }

  /** Program exited or restarted. */
  reset(): void {
    this.armed = false;
    this.text = "";
  }

  get current(): string {
    return this.text;
  }
}

/**
 * Feeds the presses on xterm's element into `selection`. Capture phase and attached before
 * `guardSecondaryClick`, so it sees every press before xterm or the guard can stop it.
 */
export function watchPresses(element: EventTarget, selection: ProgramSelection): void {
  element.addEventListener(
    "mousedown",
    (event) => {
      if ("button" in event && typeof event.button === "number") {
        selection.pointerDown(event.button);
      }
    },
    { capture: true },
  );
}

/** What the terminal menu works with: xterm's own selection first, else the program's. */
export function menuSelection(terminalSelection: string, programSelection: string): string {
  return terminalSelection !== "" ? terminalSelection : programSelection;
}
