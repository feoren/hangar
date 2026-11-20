// Sessions whose agent shows a permission or input dialog (ADR 0016), as Claude Code's
// report-only notice hooks tell the backend. Such a session waits with the reason "permission":
// a sure sign, unlike the screen check of promptGuard.ts, so neither an agent command, a queued
// message nor a link message presses Enter there. Programs without the hooks keep the screen
// check. Pure.

import type { AttentionView } from "./gate";

/** The backend's answer to `permission_waiting`; anything malformed reads as none. */
export function parsePermissionWaiting(value: unknown): ReadonlySet<number> {
  if (!Array.isArray(value)) return new Set();
  return new Set(
    value.filter(
      (id): id is number => typeof id === "number" && Number.isSafeInteger(id) && id > 0,
    ),
  );
}

/**
 * The attention with every session that shows a dialog on top: waiting with the reason
 * "permission", seen or not, until the dialog is gone. A pending gate decision keeps its own
 * reason, which says more. Without such sessions the maps come back unchanged.
 */
export function withPermission(view: AttentionView, waiting: ReadonlySet<number>): AttentionView {
  if (waiting.size === 0) return view;
  const attention = new Map(view.attention);
  const reasons = new Map(view.reasons);
  for (const id of waiting) {
    if (reasons.get(id) === "gate") continue;
    attention.set(id, "attention");
    reasons.set(id, "permission");
  }
  return { attention, reasons };
}

/**
 * Keys that answer an agent's permission or choice dialog: Enter takes the selected answer, Esc
 * declines. A digit is left out on purpose: it may be no choice of this dialog or text typed into
 * a form, and clearing the signal then would let an automatic Enter into the open dialog. A digit
 * that does answer leaves the signal until the next hook or its expiry, the safe direction.
 */
export function answersDialog(key: string): boolean {
  return key === "\r" || key === "\x1b";
}
