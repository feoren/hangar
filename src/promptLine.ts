// The lines the user submits in a session, rebuilt from the raw keyboard input (what xterm sends
// to the PTY), not from the screen, because agent CLIs redraw their input box freely. Pure, so it
// can be tested without xterm. Sessions are not named after them: a row shows its project folder
// unless the person gave it a name.

/** The line buffer stops growing here; the readers need only its start. */
const LINE_LIMIT = 2000;
const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

type Mode = "text" | "escape" | "csi" | "ss3" | "osc" | "paste";

/**
 * Rebuilds the lines the user submits with Enter from raw terminal input: printable characters
 * append, Backspace removes, Ctrl+U and Ctrl+C clear, Ctrl+W removes a word, bracketed paste
 * counts as typed text (its line breaks as spaces), every other escape sequence (arrows, focus
 * reports, xterm's automatic replies) is skipped. Cursor movement inside the line is not
 * followed; the result is meant for short summaries, not for replaying the input. Arrow keys that
 * click-to-move (terminalClick.ts) sends straight through `send()` never reach the tracker; that
 * only shifts where in the line typed text lands, which a summary can live with.
 */
export class PromptLineTracker {
  private line = "";
  private mode: Mode = "text";
  private sequence = "";
  private pasted = "";

  /** Feeds one chunk of input; returns the lines submitted in it, in order. */
  feed(data: string): string[] {
    const submitted: string[] = [];
    for (const char of data) {
      switch (this.mode) {
        case "text":
          this.text(char, submitted);
          break;
        case "escape":
          this.escape(char);
          break;
        case "csi":
          this.csi(char);
          break;
        case "ss3":
          this.mode = "text";
          break;
        case "osc":
          // Ends with BEL or ESC \; the ESC also leaves as an escape that the next char ends.
          if (char === "\x07") this.mode = "text";
          else if (char === "\x1b") this.mode = "escape";
          break;
        case "paste":
          this.paste(char);
          break;
      }
    }
    // xterm hands over each key's sequence whole, so an ESC that ends the input is the Escape key
    // itself; left pending, it would swallow the next typed character.
    if (this.mode === "escape") this.mode = "text";
    return submitted;
  }

  private text(char: string, submitted: string[]): void {
    switch (char) {
      case "\x1b":
        this.mode = "escape";
        return;
      case "\r":
      case "\n":
        submitted.push(this.line);
        this.line = "";
        return;
      case "\x7f":
      case "\b":
        this.line = Array.from(this.line).slice(0, -1).join("");
        return;
      case "\x03":
      case "\x15":
        this.line = "";
        return;
      case "\x17":
        this.line = this.line.replace(/\S*\s*$/u, "");
        return;
    }
    if (isControl(char)) return;
    this.append(char);
  }

  private escape(char: string): void {
    this.sequence = "";
    if (char === "[") this.mode = "csi";
    else if (char === "O") this.mode = "ss3";
    else if (char === "]") this.mode = "osc";
    else {
      // Alt+Enter (and Shift+Enter in some agent CLIs) starts a new line in the same prompt.
      if (char === "\r") this.append(" ");
      this.mode = "text";
    }
  }

  private csi(char: string): void {
    this.sequence += char;
    const code = char.codePointAt(0) ?? 0;
    // Parameter and intermediate bytes continue, a final byte (0x40 to 0x7e) ends.
    if (code < 0x40 || code > 0x7e) {
      if (this.sequence.length > 32) this.mode = "text";
      return;
    }
    this.mode = this.sequence === "200~" ? "paste" : "text";
    this.pasted = "";
  }

  private paste(char: string): void {
    this.pasted += char;
    if (!this.pasted.endsWith(PASTE_END)) return;
    const text = this.pasted.slice(0, -PASTE_END.length).replaceAll(PASTE_START, "");
    this.pasted = "";
    this.mode = "text";
    for (const part of text) {
      if (part === "\r" || part === "\n" || part === "\t") this.append(" ");
      else if (!isControl(part)) this.append(part);
    }
  }

  private append(char: string): void {
    if (this.line.length < LINE_LIMIT) this.line += char;
  }
}

function isControl(char: string): boolean {
  const code = char.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}
