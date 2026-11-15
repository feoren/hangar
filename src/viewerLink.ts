// A click on a link in the file viewer's Markdown preview. A Markdown file is written by anyone
// (a cloned repository, a downloaded package), and `[github.com/org](https://example.net/)` shows
// one address while leading to another, just as an OSC 8 link in the terminal can. So the same
// check applies (termSafety.ts `osc8Decision`): misleading link text shows the real address first.

import { osc8Decision, safeLinkUrl } from "./termSafety";

export type ViewerLinkAction =
  | { kind: "open"; url: string }
  | { kind: "confirm"; url: string }
  /** No http(s) address, or one with a user name: nothing opens. */
  | { kind: "ignore" };

/** What a click on a link with the address `href` and the visible text `shown` does. */
export function viewerLinkAction(href: string, shown: string): ViewerLinkAction {
  const url = safeLinkUrl(href);
  if (url === null) return { kind: "ignore" };
  return osc8Decision(shown, url) === "confirm" ? { kind: "confirm", url } : { kind: "open", url };
}
