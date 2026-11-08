// Guard at the boundary between the clipboard or program output and the terminal. Pure logic,
// so it can be tested without xterm.

/**
 * Control characters that have no business in a paste: C0 except tab, line feed and carriage
 * return, plus DEL and the C1 characters (0x80 to 0x9f, e.g. 0x9b as 8-bit CSI).
 * ESC is what matters: a pasted `ESC[201~` would otherwise end bracketed paste early, and the
 * rest of the text would run into the shell or the agent as typed commands.
 */
// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const PASTE_CONTROL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/g;

/** Clipboard text, sanitized for `term.paste`. Line breaks are preserved. */
export function sanitizePaste(text: string): string {
  return text.replace(PASTE_CONTROL, "");
}

/**
 * Address of a terminal link (OSC 8) if it is safe to show: only http and https, otherwise
 * `null`. Hangar never opens `javascript:`, `file:` or custom schemes.
 */
export function safeLinkUrl(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  return url.href;
}
