// Number of waiting sessions on the taskbar button (Windows: a small overlay icon drawn by the
// window) or the Dock and launcher icon (macOS, Linux: the system's badge count). Tied to the
// attention setting: without attention there is nothing waiting and the badge is cleared.

/** How the platform shows a count: Windows has no badge count, only an overlay icon. */
export type BadgeKind = "overlay" | "count";

export function badgeKind(userAgent: string): BadgeKind {
  return /Windows/i.test(userAgent) ? "overlay" : "count";
}

/** Text in the overlay icon: "1" to "9", then "9+"; `null` clears it. */
export function badgeLabel(count: number): string | null {
  if (!Number.isFinite(count) || count < 1) return null;
  return count > 9 ? "9+" : String(Math.floor(count));
}

/** What the badge shows: the overlay label, or the plain count for a system badge. */
export function badgeText(count: number, kind: BadgeKind): string | null {
  if (kind === "overlay") return badgeLabel(count);
  return Number.isFinite(count) && count >= 1 ? String(Math.floor(count)) : null;
}

/**
 * Sends the count only when what is shown changes (the overlay is drawn once per label and color)
 * and swallows errors, like the taskbar flash: the badge is an extra, the sidebar keeps working
 * without it. A failed call is retried on the next change.
 */
export class TaskbarBadge {
  private shown: string | null = null;
  private color = "";

  constructor(
    private readonly kind: BadgeKind,
    /** `text` is `null` to clear the badge; `count` is 0 then. `color` fills the overlay. */
    private readonly apply: (count: number, text: string | null, color: string) => Promise<void>,
  ) {}

  update(count: number, color: string): void {
    const text = badgeText(count, this.kind);
    const key = text === null ? null : `${text} ${color}`;
    if (key === this.shown) return;
    const previous = this.shown;
    this.shown = key;
    this.color = color;
    this.apply(text === null ? 0 : Math.floor(count), text, color).catch(() => {
      if (this.shown === key) this.shown = previous;
    });
  }

  /**
   * Removes a shown badge before quitting: a Dock or launcher badge can outlive the window.
   * Never rejects, so quitting goes on even when the platform call fails.
   */
  async clear(): Promise<void> {
    if (this.shown === null) return;
    this.shown = null;
    await this.apply(0, null, this.color).catch(() => undefined);
  }
}
