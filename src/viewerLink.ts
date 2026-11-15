// A click on a link in the file viewer's Markdown preview. A Markdown file is written by anyone
// (a cloned repository, a downloaded package), and `[github.com/org](https://example.net/)` shows
// one address while leading to another, just as an OSC 8 link in the terminal can. So the same
// check applies (termSafety.ts `osc8Decision`): misleading link text shows the real address first.
// As in the terminal, the link is judged with the text that touches it: `github.com/[acme](...)`
// shows `github.com/acme`, also when the text before is code or another link.

import { osc8DecisionOf, safeLinkUrl } from "./termSafety";
import { WORD_LIMIT } from "./terminalLinkWord";

export type ViewerLinkAction =
  | { kind: "open"; url: string }
  | { kind: "confirm"; url: string }
  /** No http(s) address, or one with a user name: nothing opens. */
  | { kind: "ignore" };

/**
 * What a click on a link with the address `href` does. `shown` are the texts it reads by: the
 * word around it (`wordAround`) and its own text. It opens at once only when none misleads.
 */
export function viewerLinkAction(href: string, shown: string, ...more: string[]): ViewerLinkAction {
  const url = safeLinkUrl(href);
  if (url === null) return { kind: "ignore" };
  return osc8DecisionOf([shown, ...more], url) === "confirm"
    ? { kind: "confirm", url }
    : { kind: "open", url };
}

/**
 * The link at `start` to `end` (UTF-16 offsets, `end` exclusive) in the text of its block, widened
 * by the characters that touch it up to white space on either side, as the reader sees one word.
 */
export function wordAround(text: string, start: number, end: number): string {
  let left = start;
  while (left > 0 && start - left < WORD_LIMIT && !/\s/u.test(text.charAt(left - 1))) left -= 1;
  let right = end;
  while (right < text.length && right - end < WORD_LIMIT && !/\s/u.test(text.charAt(right))) {
    right += 1;
  }
  return text.slice(left, right);
}
