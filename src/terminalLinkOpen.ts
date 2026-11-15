// What a click on a terminal link does (terminalLinkProvider.ts asks, the app carries it out):
//
// - a web address opens in the default browser; if that fails, the app shows it to copy, as for
//   OSC 8 links;
// - a folder opens in the system file manager (the files panel only knows the session's folder,
//   a printed path can be anywhere);
// - a file in the session's folder opens in Hangar's viewer, unless only its own program can show
//   it; anything else, and whatever the viewer refuses, opens in its default program. The backend
//   opens only harmless types (text, code, images, PDF) and shows anything else selected in
//   the file manager.

import type { LinkRequest } from "./terminalLinkProvider";
import { pathAction } from "./terminalLinks";

export interface LinkOpeners {
  openUrl: (url: string) => Promise<unknown>;
  openPath: (path: string) => Promise<unknown>;
  /** `relative`: `/` separated below the session's folder, as the file viewer takes it. */
  openViewer: (relative: string) => Promise<unknown>;
}

export type LinkOutcome =
  | { kind: "opened" }
  /** The browser did not open: show the address to copy. */
  | { kind: "showUrl"; url: string }
  | { kind: "failed"; error: unknown };

export async function openLink(request: LinkRequest, openers: LinkOpeners): Promise<LinkOutcome> {
  if (request.kind === "url") {
    try {
      await openers.openUrl(request.url);
      return { kind: "opened" };
    } catch {
      return { kind: "showUrl", url: request.url };
    }
  }
  const action = pathAction(request.pathKind, request.path, request.root);
  if (action.kind === "viewer") {
    try {
      await openers.openViewer(action.relative);
      return { kind: "opened" };
    } catch {
      // A file the viewer refuses (a link, inside .git): its own program can still show it.
    }
  }
  try {
    await openers.openPath(request.path);
    return { kind: "opened" };
  } catch (error) {
    return { kind: "failed", error };
  }
}
