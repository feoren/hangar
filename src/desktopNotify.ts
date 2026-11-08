// Desktop notification when a session starts waiting while Hangar is in the background. Like the
// taskbar flash (taskbarFlash.ts) it complements the pulse in the sidebar, which only helps while
// you look at the window; a click on the notification brings that session to the front.

import type { AttentionState } from "./attention";
import { shortPath } from "./format";
import type { Translate } from "./i18n";

export interface NotifyContext {
  /** "Desktop notification" setting, only effective while attention is on. */
  enabled: boolean;
  /** The Hangar window has focus (`document.hasFocus()`). */
  windowFocused: boolean;
}

/** Notify only on a switch to "attention", only with the setting on, only in the background. */
export function shouldNotify(next: AttentionState, context: NotifyContext): boolean {
  return context.enabled && !context.windowFocused && next === "attention";
}

export interface NotificationText {
  title: string;
  body: string;
}

/**
 * "{title} is waiting for you", or "{title} has finished" when the process has exited; the body
 * names the working folder so two sessions with the same program can be told apart.
 */
export function notificationText(
  session: { title: string; cwd: string },
  exited: boolean,
  t: Translate,
): NotificationText {
  return {
    title: t(exited ? "notify.finished" : "notify.waiting", { title: session.title }),
    body: shortPath(session.cwd),
  };
}
