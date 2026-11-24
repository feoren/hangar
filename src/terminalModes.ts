// Terminal input modes a program switches on for itself (DECSET): bracketed paste, mouse
// tracking, focus reports, application cursor keys and keypad. xterm keeps them until something
// switches them off, so a program that crashes, or output that merely contains the sequence
// (`type` on a file, a log), leaves them on for whatever reads the input next. Paste and send
// decisions rely on bracketed paste (appPaste.ts, broadcast.ts), so the mode must belong to the
// program that actually reads the input.

import type { ShellKind } from "./pathQuote";

/**
 * Switches off every input mode the previous program may have left on: bracketed paste (2004),
 * the mouse tracking modes (9, 1000, 1002, 1003) and encodings (1005, 1006, 1015, 1016), focus
 * reports (1004), application cursor keys (1) and the application keypad (`ESC >`). Written when
 * the process exits and before a restart, so the next program starts from the defaults.
 */
export const RESET_INPUT_MODES =
  "\x1b[?2004l" +
  "\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l" +
  "\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l" +
  "\x1b[?1004l\x1b[?1l\x1b>";

/**
 * Whether the bracketed paste mode can be believed for a program of this kind. cmd and PowerShell
 * (PSReadLine) never switch it on themselves, so when it is on there, program output switched it
 * on and the shell still runs each pasted line as it arrives. POSIX shells (readline, zle) and
 * agent CLIs switch it on and off around their own prompt, so their mode is real.
 */
export function trustsBracketedPaste(kind: ShellKind): boolean {
  return kind !== "cmd" && kind !== "powershell";
}
