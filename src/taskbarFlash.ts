// Flash the taskbar (macOS: bounce the Dock icon) when a session switches to the "attention"
// state and the Hangar window is not in the foreground. Complements the pulse in the sidebar
// (attention.ts), which is only visible while you are looking at the window.
//
// Kind: informational. On Windows this only flashes the taskbar button a few times, and it then
// stays highlighted until the window gets focus; on macOS the Dock icon bounces once.
// "critical" would additionally make the title bar flash permanently on Windows and the icon bounce
// on macOS until you switch over. A waiting agent is a hint, not an alarm, and several agents
// would otherwise be a constant disturbance (tao 0.37: FLASHW_TRAY with 4 cycles versus
// FLASHW_ALL | FLASHW_TIMERNOFG).

import type { AttentionState } from "./attention";

export type FlashKind = "informational" | "critical";

export const FLASH_KIND: FlashKind = "informational";

export interface FlashContext {
  /** "Flash taskbar" setting. */
  enabled: boolean;
  /** The Hangar window has focus (`document.hasFocus()`). */
  windowFocused: boolean;
}

/** Flash only on a switch to "attention", only with the setting on, only in the background. */
export function shouldFlash(next: AttentionState, context: FlashContext): boolean {
  return context.enabled && !context.windowFocused && next === "attention";
}

/**
 * Tracks whether a flash is active, so turning it off reaches the window only once and only when
 * needed. `request(null)` ends the flash. Errors (say, in a browser without Tauri) are swallowed:
 * the flash is an extra; the pulse in the sidebar keeps working without it.
 */
export class TaskbarFlasher {
  private flashing = false;

  constructor(private readonly request: (kind: FlashKind | null) => Promise<void>) {}

  get active(): boolean {
    return this.flashing;
  }

  /** A session changed its state. Every session that starts waiting flashes again. */
  attentionChanged(next: AttentionState, context: FlashContext): void {
    if (!shouldFlash(next, context)) return;
    this.flashing = true;
    this.request(FLASH_KIND).catch(() => {
      this.flashing = false;
    });
  }

  /** The window got focus or the setting was turned off. */
  stop(): void {
    if (!this.flashing) return;
    this.flashing = false;
    this.request(null).catch(() => undefined);
  }
}
