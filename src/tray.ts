// Notification area icon ("Keep running in the notification area"). The backend owns the icon
// and hides the window on a close request while it exists; the window decides whether closing
// quits. It may only skip quitting while the icon really exists, or the user would be left with
// a hidden window and no way back.

import type { TrayLabels } from "./api";
import type { Translate } from "./i18n";

/** Menu texts and tooltip; the tooltip names how many sessions are waiting. */
export function trayLabels(t: Translate, waiting: number): TrayLabels {
  return {
    open: t("tray.open"),
    quit: t("menu.quit"),
    tooltip: waiting > 0 ? t("tray.tooltipWaiting", { count: waiting }) : "Hangar",
  };
}

/**
 * Tracks whether the icon exists. Active means: the last `configure(true)` resolved without
 * error and no `configure(false)` was requested after it. Calls may overlap (setting toggled
 * quickly, language changed); an answer older than one already settled is ignored.
 */
export class TrayState {
  private requested = 0;
  private settled = 0;
  private disabledAt = 0;
  private iconActive = false;

  constructor(private readonly send: (enabled: boolean, labels: TrayLabels) => Promise<unknown>) {}

  get active(): boolean {
    return this.iconActive;
  }

  async configure(enabled: boolean, labels: TrayLabels): Promise<void> {
    this.requested += 1;
    const call = this.requested;
    // Turning off counts at once: a close request from now on quits.
    if (!enabled) {
      this.disabledAt = call;
      this.iconActive = false;
    }
    let ok: boolean;
    try {
      await this.send(enabled, labels);
      ok = true;
    } catch {
      ok = false;
    }
    if (call < this.settled) return;
    this.settled = call;
    // Without an icon (failed call), closing quits as before.
    this.iconActive = ok && enabled && call > this.disabledAt;
  }
}
