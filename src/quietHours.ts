// Quiet hours: a daily time window in which Hangar does not interrupt. Taskbar flash, chime and
// desktop notifications stay off; the state colors in the sidebar and the waiting count stay.
// Times are minutes after local midnight on the wall clock, so a daylight saving change moves
// nothing: 22:00 stays 22:00 whatever the offset to UTC is that night.

const MINUTES_PER_DAY = 24 * 60;

export interface QuietHours {
  enabled: boolean;
  /** Start, minutes after local midnight (0 to 1439). */
  from: number;
  /** End, minutes after local midnight; before `from` when the window crosses midnight. */
  to: number;
}

export const DEFAULT_QUIET_HOURS: QuietHours = { enabled: false, from: 22 * 60, to: 7 * 60 };

/** Minutes after local midnight on the wall clock. */
export function minuteOfDay(date: Date): number {
  return date.getHours() * 60 + date.getMinutes();
}

/**
 * Whether `minute` lies in the window from `from` (inclusive) to `to` (exclusive). A window whose
 * end lies before its start runs over midnight; equal start and end mean an empty window.
 */
export function inWindow(from: number, to: number, minute: number): boolean {
  if (from === to) return false;
  if (from < to) return minute >= from && minute < to;
  return minute >= from || minute < to;
}

export function isQuietTime(quiet: QuietHours, date: Date): boolean {
  return quiet.enabled && inWindow(quiet.from, quiet.to, minuteOfDay(date));
}

function isMinute(value: unknown): value is number {
  return (
    Number.isInteger(value) && typeof value === "number" && value >= 0 && value < MINUTES_PER_DAY
  );
}

/** Stored quiet hours; anything invalid falls back to the defaults field by field. */
export function parseQuietHours(raw: unknown): QuietHours {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return DEFAULT_QUIET_HOURS;
  const enabled: unknown = Reflect.get(raw, "enabled");
  const from: unknown = Reflect.get(raw, "from");
  const to: unknown = Reflect.get(raw, "to");
  return {
    enabled: typeof enabled === "boolean" ? enabled : DEFAULT_QUIET_HOURS.enabled,
    from: isMinute(from) ? from : DEFAULT_QUIET_HOURS.from,
    to: isMinute(to) ? to : DEFAULT_QUIET_HOURS.to,
  };
}

/** "07:05" for 425; the value format of `<input type="time">`. */
export function formatClockTime(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  return `${String(hours).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
}

/** Minutes after midnight from "HH:MM" (seconds allowed and ignored); `null` when invalid. */
export function parseClockTime(text: string): number | null {
  const match = /^(\d{2}):(\d{2})(?::\d{2}(?:\.\d+)?)?$/.exec(text);
  if (match === null) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) return null;
  return hours * 60 + minutes;
}

export interface NoticeSettings {
  taskbarFlash: boolean;
  notifyDesktop: boolean;
  chime: boolean;
  quietHours: QuietHours;
}

/**
 * The settings as they apply at `date`: during quiet hours the interrupting notices are off.
 * Outside them, the very same object comes back.
 */
export function noticesAt<T extends NoticeSettings>(settings: T, date: Date): T {
  if (!isQuietTime(settings.quietHours, date)) return settings;
  return { ...settings, taskbarFlash: false, notifyDesktop: false, chime: false };
}
