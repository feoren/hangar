// Names an agent session after its task: the first line the user submits there. Reads the raw
// keyboard input (what xterm sends to the PTY), not the screen, because agent CLIs redraw their
// input box freely. Pure, so it can be tested without xterm.

import type { ShellKind } from "./pathQuote";
import { isProgramTitle, projectFolder } from "./sessionLabel";

/** Longest title taken from a prompt, ellipsis included, in characters (code points). */
export const PROMPT_TITLE_MAX = 48;
/** The line buffer stops growing here; a title needs only its start. */
const LINE_LIMIT = 2000;
const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

type Mode = "text" | "escape" | "csi" | "ss3" | "osc" | "paste";

/**
 * Rebuilds the lines the user submits with Enter from raw terminal input: printable characters
 * append, Backspace removes, Ctrl+U and Ctrl+C clear, Ctrl+W removes a word, bracketed paste
 * counts as typed text (its line breaks as spaces), every other escape sequence (arrows, focus
 * reports, xterm's automatic replies) is skipped. Cursor movement inside the line is not
 * followed; the result is meant for a short title, not for replaying the input. Arrow keys that
 * click-to-move (terminalClick.ts) sends straight through `send()` never reach the tracker; that
 * only shifts where in the line typed text lands, which a title can live with.
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

/**
 * Short answers to the questions agents ask before the first task (trust this folder, add to
 * .gitignore); they never name a session.
 */
const ANSWERS = new Set([
  "yes",
  "no",
  "ja",
  "nein",
  "okay",
  "sure",
  "continue",
  "weiter",
  "cancel",
  "abbrechen",
  "exit",
  "quit",
]);

/**
 * Title for a submitted line: trimmed, inner whitespace collapsed, at most `PROMPT_TITLE_MAX`
 * characters with an ellipsis (cut at a word where one is near). `null` for lines that do not
 * describe a task: empty ones, slash commands ("/model"), and short answers ("1", "y", "yes").
 */
export function promptTitle(line: string): string | null {
  const text = line.replace(/\s+/gu, " ").trim();
  if (text === "" || text.startsWith("/")) return null;
  const letters = text.match(/\p{L}/gu)?.length ?? 0;
  if (letters < 4 || ANSWERS.has(text.toLocaleLowerCase())) return null;
  const chars = Array.from(text);
  if (chars.length <= PROMPT_TITLE_MAX) return text;
  const cut = chars.slice(0, PROMPT_TITLE_MAX - 1).join("");
  const space = cut.lastIndexOf(" ");
  const head = space >= cut.length - 16 && space > 0 ? cut.slice(0, space) : cut;
  return `${head.trimEnd()}…`;
}

/**
 * Whether `title` is one Hangar gave on its own and a prompt may replace: the program name,
 * numbered ("Claude Code (2)") or with a worktree suffix ("Claude Code · 1a2b3c"), or the
 * project folder's name. Anything else was typed by the user and stays.
 */
export function isAutoTitle(title: string, cwd: string, programNames: readonly string[]): boolean {
  const wanted = title.trim().toLocaleLowerCase();
  if (isProgramTitle(title, programNames)) return true;
  const folder = projectFolder(cwd).toLocaleLowerCase();
  if (folder !== "" && wanted === folder) return true;
  return programNames.some((name) => {
    const base = name.trim().toLocaleLowerCase();
    return base !== "" && wanted.startsWith(`${base} · `);
  });
}

export interface PromptSession {
  title: string;
  cwd: string;
  kind?: string | undefined;
  /** Program name and profile id: the names Hangar may have given the session. */
  programNames: readonly string[];
  /** How the program reads input (pathQuote.ts); only agents are named after a prompt. */
  shell: ShellKind;
}

/**
 * What a submitted line means for the session's name: `stop` listening (not an agent, an update
 * session, or the user named it), `wait` for a line that describes a task, or `rename`.
 */
export type PromptVerdict = { kind: "stop" } | { kind: "wait" } | { kind: "rename"; title: string };

export function promptVerdict(session: PromptSession, line: string): PromptVerdict {
  if (session.shell !== "agent" || session.kind === "update") return { kind: "stop" };
  if (!isAutoTitle(session.title, session.cwd, session.programNames)) return { kind: "stop" };
  const title = promptTitle(line);
  return title === null ? { kind: "wait" } : { kind: "rename", title };
}
