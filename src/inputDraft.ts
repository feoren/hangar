// Whether a session's input line may hold a draft: text the person typed or pasted there and has
// not sent yet. An automatic delivery (link groups, ADR 0014) presses Enter after its paste, and
// that Enter would send the draft along, so it waits while one stands. Reads the raw input that
// reaches the program, not the screen, because agent CLIs draw their input box freely. Pure, so it
// can be tested without xterm. Keeps no text, only how much of it there is.

const PASTE_START = "200~";
const PASTE_END = "\x1b[201~";
/** An escape sequence longer than this is no key: the parser gives up and reads text again. */
const SEQUENCE_LIMIT = 32;

type Mode = "text" | "escape" | "csi" | "ss3" | "osc" | "paste";

/**
 * Follows the person's input line from the keys and pastes that reach the program. Errs on the
 * side of a draft: whenever the line's content is no longer known for sure, it counts as a draft
 * until Enter or Ctrl+C, since a delivery that waits too long costs a moment and one that sends a
 * half-typed prompt cannot be taken back.
 *
 * - Printable characters and pasted text (bracketed or not) add to the line; a paste's line breaks
 *   stay inside it.
 * - Enter sends the line, except after a backslash, where agent CLIs and shells continue it. Alt+
 *   Enter (Hangar's Shift+Enter) and Ctrl+J add a line break inside the draft.
 * - Ctrl+C clears the line. Ctrl+U clears it while the cursor stayed at its end on one line.
 * - Backspace removes one character; the line is empty once every typed character went, while the
 *   cursor stayed at its end.
 * - Moving the cursor inside the line (Left, Right, Home, End, Delete), deleting a word (Ctrl+W,
 *   Alt+Backspace, Ctrl+K) or Up and Down (a line from the history) leave the content unknown.
 * - Escape sequences that are no key press (xterm's replies, focus and mouse reports, the wheel
 *   scrolling a full-screen program with Up and Down) change nothing. A lone Escape never counts
 *   as clearing: some agents clear on it, others do not.
 */
export class InputDraft {
  /** Characters in the line since it was last empty, as far as they are known. */
  private chars = 0;
  /** `chars` is the line's whole content and the cursor stands at its end. */
  private exact = true;
  /** The line holds more than one line (a line break inside the draft). */
  private lines = false;
  /** The line may hold text although `chars` says nothing (Up or Down brought one back). */
  private recalled = false;
  /** The last character added, for the backslash that makes Enter continue the line. */
  private last = "";
  private mode: Mode = "text";
  private sequence = "";
  private pasted = 0;
  private pasteTail = "";
  /** Bytes of an X10 mouse report (`ESC [ M` and three raw bytes) still to skip. */
  private mouseBytes = 0;
  /** The chunk being fed comes from a key press, not from a reply or a paste. */
  private key = false;

  /** Whether the line may hold a draft the person has not sent. */
  get pending(): boolean {
    return this.chars > 0 || this.recalled;
  }

  /** The line is empty again: a new process started. */
  reset(): void {
    this.clear();
    this.mode = "text";
    this.sequence = "";
    this.mouseBytes = 0;
  }

  /**
   * Feeds one chunk of input as it reaches the program. `key`: the chunk is one key press (xterm's
   * `onKey`), so its escape sequences move the cursor or recall a line; otherwise they are replies
   * or the wheel and change nothing.
   */
  feed(data: string, key = false): void {
    this.key = key;
    for (const char of data) {
      switch (this.mode) {
        case "text":
          this.text(char);
          break;
        case "escape":
          this.escape(char);
          break;
        case "csi":
          this.csi(char);
          break;
        case "ss3":
          this.mode = "text";
          if (char === "A" || char === "B") this.recall();
          else if (char === "C" || char === "D" || char === "H" || char === "F") this.moved();
          break;
        case "osc":
          // OSC, DCS, APC, PM and SOS strings end with BEL or ESC \; the ESC also leaves as an
          // escape that the next char ends.
          if (char === "\x07") this.mode = "text";
          else if (char === "\x1b") this.mode = "escape";
          break;
        case "paste":
          this.paste(char);
          break;
      }
    }
    this.endChunk();
  }

  /**
   * xterm hands over each key, reply and paste as one whole chunk, so nothing that is still open
   * at a chunk's end continues in the next one. An ESC there is the Escape key itself; `ESC ]`,
   * `ESC [` or `ESC O` there are Alt+], Alt+[ and Alt+O. Left open, they would swallow the
   * characters typed next, and a draft nobody counted is one the delivery's Enter would send.
   */
  private endChunk(): void {
    this.mouseBytes = 0;
    if (this.mode === "paste") {
      // A paste without its end marker: what came of it is still the person's text.
      this.flushPasteTail();
      this.chars += this.pasted;
      if (this.pasted > 0) this.last = "";
      this.pasted = 0;
    }
    this.mode = "text";
  }

  private text(char: string): void {
    if (this.mouseBytes > 0) {
      this.mouseBytes -= 1;
      return;
    }
    switch (char) {
      case "\x1b":
        this.mode = "escape";
        return;
      case "\r":
        if (this.last === "\\" && this.chars > 0) this.newline();
        else this.clear();
        return;
      case "\n":
        this.newline();
        return;
      case "\x7f":
      case "\b":
        if (this.chars > 0) this.chars -= 1;
        // Only a line known to the last character is empty when the count is.
        if (this.chars === 0 && !this.exact) this.chars = 1;
        this.last = "";
        return;
      case "\x03":
        this.clear();
        return;
      case "\x15":
        if (this.exact && !this.lines) this.clear();
        return;
      case "\x17":
      case "\x0b":
        this.unknown();
        return;
    }
    if (isControl(char)) return;
    this.add(char);
  }

  private escape(char: string): void {
    this.sequence = "";
    if (char === "[") this.mode = "csi";
    else if (char === "O") this.mode = "ss3";
    else if (char === "]" || char === "P" || char === "_" || char === "^" || char === "X") {
      this.mode = "osc";
    } else {
      this.mode = "text";
      // Alt+Enter: a new line in the same prompt. Alt+Backspace and Alt+D delete a word.
      if (char === "\r") this.newline();
      else if (char === "\x7f" || char === "d") this.unknown();
    }
  }

  private csi(char: string): void {
    this.sequence += char;
    const code = char.codePointAt(0) ?? 0;
    // Parameter and intermediate bytes continue, a final byte (0x40 to 0x7e) ends.
    if (code < 0x40 || code > 0x7e) {
      if (this.sequence.length > SEQUENCE_LIMIT) this.mode = "text";
      return;
    }
    this.mode = "text";
    // A mouse report in X10 encoding: its three bytes look like printable text.
    if (this.sequence === "M" && !this.key) {
      this.mouseBytes = 3;
      return;
    }
    if (this.sequence === PASTE_START) {
      this.mode = "paste";
      this.pasted = 0;
      this.pasteTail = "";
      return;
    }
    // Replies and reports carry markers (`?`, `<`) or end in letters no key sends.
    if (/[?<>]/u.test(this.sequence)) return;
    if (char === "A" || char === "B") this.recall();
    else if (char === "C" || char === "D" || char === "H" || char === "F") this.moved();
    else if (char === "~" && /^[1-8](;\d+)?~$/u.test(this.sequence)) this.moved();
  }

  /** Counts the paste's characters up to its end marker, without keeping them. */
  private paste(char: string): void {
    this.pasteTail += char;
    if (PASTE_END.startsWith(this.pasteTail)) {
      if (this.pasteTail.length < PASTE_END.length) return;
      this.mode = "text";
      if (this.pasted > 0) {
        this.chars += this.pasted;
        this.last = "";
      }
      this.pasteTail = "";
      return;
    }
    // Not the end marker after all: what was held back is pasted text.
    this.flushPasteTail();
  }

  private flushPasteTail(): void {
    for (const part of this.pasteTail) {
      if (part === "\r" || part === "\n") this.lines = true;
      if (part !== "\x1b") this.pasted += 1;
    }
    this.pasteTail = "";
  }

  private add(char: string): void {
    this.chars += 1;
    this.last = char;
  }

  private newline(): void {
    this.chars += 1;
    this.lines = true;
    this.last = "";
  }

  /** A key that moves the cursor inside the line: Backspace and Ctrl+U are no longer counted. */
  private moved(): void {
    if (this.key && this.pending) this.exact = false;
  }

  /** Up or Down: a line from the history may stand there now, of unknown length. */
  private recall(): void {
    if (!this.key) return;
    this.recalled = true;
    this.exact = false;
  }

  /** Part of the line went, how much is unknown. */
  private unknown(): void {
    if (this.pending) this.exact = false;
  }

  private clear(): void {
    this.chars = 0;
    this.exact = true;
    this.lines = false;
    this.recalled = false;
    this.last = "";
  }
}

function isControl(char: string): boolean {
  const code = char.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}
