// The user's last prompt in an agent session, shown above its terminal; a click scrolls back to
// where it was sent. Which submitted lines count is decided here, without xterm.

/** Characters of a prompt kept for the bar; the full text stays in the tooltip up to this. */
export const LAST_PROMPT_MAX = 2000;

/**
 * A submitted line worth showing as "your last prompt": not empty, not a slash command and not a
 * one-character answer to a menu or a yes/no question ("1", "y").
 */
export function promptWorthShowing(line: string): string | null {
  const text = line.replace(/\s+/g, " ").trim();
  if (text.length < 2 || text.startsWith("/")) return null;
  return text.length > LAST_PROMPT_MAX ? `${text.slice(0, LAST_PROMPT_MAX - 1)}…` : text;
}
