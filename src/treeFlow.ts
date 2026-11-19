// Tree flow: an optional animation on the sidebar's tree guides. On the way from a group's header
// to every session that is working, and on a sub-agent branch to every running sub-agent, the
// dotted guide carries a stream of light; its speed follows the session's output rate. Optionally
// the row shows its output and input rate. This module is pure: the settings' shape and parser,
// the rate meter with injected time, the speed levels and the number format. The store that feeds
// the rows lives in treeFlowStore.ts, the markup in TreeFlowView.tsx, the look in styles.css ("Tree
// flow").

/**
 * - `stream`: a thin stream of particles with a soft glow, blue and violet (default)
 * - `pulse`: single points of light travel along the guide
 * - `dots`: the guide's own dots light up and move
 */
export const TREE_FLOW_STYLES = ["stream", "pulse", "dots"] as const;
export type TreeFlowStyle = (typeof TREE_FLOW_STYLES)[number];

export interface TreeFlowPrefs {
  /** Off by default: no measuring and no animation at all. */
  enabled: boolean;
  style: TreeFlowStyle;
  /** Output and input per second on a working session's row. */
  showRate: boolean;
}

export const DEFAULT_TREE_FLOW: TreeFlowPrefs = { enabled: false, style: "stream", showRate: true };

export function isTreeFlowStyle(value: unknown): value is TreeFlowStyle {
  return typeof value === "string" && TREE_FLOW_STYLES.some((style) => style === value);
}

/** Stored value; each missing or invalid part takes its default. */
export function parseTreeFlow(value: unknown): TreeFlowPrefs {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return DEFAULT_TREE_FLOW;
  }
  const enabled: unknown = Reflect.get(value, "enabled");
  const style: unknown = Reflect.get(value, "style");
  const showRate: unknown = Reflect.get(value, "showRate");
  return {
    enabled: typeof enabled === "boolean" ? enabled : DEFAULT_TREE_FLOW.enabled,
    style: isTreeFlowStyle(style) ? style : DEFAULT_TREE_FLOW.style,
    showRate: typeof showRate === "boolean" ? showRate : DEFAULT_TREE_FLOW.showRate,
  };
}

/**
 * Speed levels of the flow: 1 calm, 2 normal, 3 busy (styles.css maps each to a duration). A few
 * coarse steps, so the animation keeps its pace instead of changing it with every chunk.
 */
export type FlowLevel = 1 | 2 | 3;

/** Bytes of output per second from which the flow runs at level 2 and 3. */
export const LEVEL_UP = { 2: 2_000, 3: 40_000 } as const;
/** A level is left downwards only below this share of its threshold (hysteresis). */
export const LEVEL_DOWN_SHARE = 0.5;

/** The next level for a smoothed output rate, from the current one. */
export function nextLevel(current: FlowLevel, bytesPerSecond: number): FlowLevel {
  const raw: FlowLevel = bytesPerSecond >= LEVEL_UP[3] ? 3 : bytesPerSecond >= LEVEL_UP[2] ? 2 : 1;
  if (raw >= current) return raw;
  // Down only once the rate is clearly below the current level's threshold.
  const threshold = LEVEL_UP[current === 3 ? 3 : 2];
  if (bytesPerSecond >= threshold * LEVEL_DOWN_SHARE) return current;
  return raw;
}

/** One session's smoothed rates and level. */
export interface FlowRate {
  /** Output bytes per second, smoothed. */
  down: number;
  /** Input bytes per second, smoothed. */
  up: number;
  level: FlowLevel;
}

/** Weight of the newest second in the smoothed rate. */
export const SMOOTHING = 0.5;
/** Below this many bytes per second a smoothed rate counts as zero. */
const QUIET_RATE = 1;

interface Counter {
  /** Bytes since the last sample. */
  down: number;
  up: number;
  /** The smoothed rates, exact. */
  smoothDown: number;
  smoothUp: number;
  /** What readers get; replaced only when the shown numbers or the level change. */
  rate: FlowRate;
}

/**
 * Counts output and input bytes per session and turns them into smoothed rates once per sample.
 * Time is passed in, so tests drive it without a clock.
 */
export class RateMeter {
  private readonly counters = new Map<number, Counter>();
  private lastSample: number | null = null;

  /** Output bytes of a session (the PTY stream the terminal receives). */
  output(id: number, bytes: number): void {
    this.counter(id).down += bytes;
  }

  /** Input bytes to a session (what Hangar writes to its PTY). */
  input(id: number, bytes: number): void {
    this.counter(id).up += bytes;
  }

  clear(): void {
    this.counters.clear();
    this.lastSample = null;
  }

  /**
   * Turns the bytes since the last sample into rates. A rate object stays the same object while
   * its shown numbers and its level do not change, so readers can compare by reference. Returns
   * whether any rate object changed.
   */
  sample(now: number): boolean {
    const seconds = this.lastSample === null ? 1 : (now - this.lastSample) / 1000;
    this.lastSample = now;
    if (seconds <= 0) return false;
    let changed = false;
    for (const [id, counter] of this.counters) {
      counter.smoothDown = smooth(counter.smoothDown, counter.down / seconds);
      counter.smoothUp = smooth(counter.smoothUp, counter.up / seconds);
      counter.down = 0;
      counter.up = 0;
      const before = counter.rate;
      const level = nextLevel(before.level, counter.smoothDown);
      if (counter.smoothDown === 0 && counter.smoothUp === 0 && level === 1) {
        // Quiet for good: nothing left to tell, and no reason to keep the counter.
        this.counters.delete(id);
        changed = true;
        continue;
      }
      const down = shown(counter.smoothDown);
      const up = shown(counter.smoothUp);
      if (down !== before.down || up !== before.up || level !== before.level) {
        counter.rate = { down, up, level };
        changed = true;
      }
    }
    return changed;
  }

  rate(id: number): FlowRate | undefined {
    return this.counters.get(id)?.rate;
  }

  private counter(id: number): Counter {
    let counter = this.counters.get(id);
    if (counter === undefined) {
      counter = { down: 0, up: 0, smoothDown: 0, smoothUp: 0, rate: QUIET };
      this.counters.set(id, counter);
    }
    return counter;
  }
}

const QUIET: FlowRate = { down: 0, up: 0, level: 1 };

/** A rate at the precision the row shows (0.1 KB/s), in bytes per second. */
function shown(bytesPerSecond: number): number {
  return Math.round(bytesPerSecond / 100) * 100;
}

function smooth(previous: number, current: number): number {
  const value = previous + SMOOTHING * (current - previous);
  return value < QUIET_RATE ? 0 : value;
}

/** The highest level among `levels`; 1 without any. */
export function highestLevel(levels: Iterable<FlowLevel>): FlowLevel {
  let highest: FlowLevel = 1;
  for (const level of levels) if (level > highest) highest = level;
  return highest;
}

/**
 * Which row ends the lit trunk of a group or branch: the last live row. With none live, the one
 * that ended it before, while it is still there, so the trunk fades out at its full length
 * instead of jumping first; `null` when there is none.
 */
export function flowEnd<T>(previous: T | null, rows: readonly T[], live: readonly T[]): T | null {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const row = rows[index];
    if (row !== undefined && live.includes(row)) return row;
  }
  return previous !== null && rows.includes(previous) ? previous : null;
}

/** A rate as kilo- or megabytes per second: the unit and the localized number. */
export function rateParts(
  bytesPerSecond: number,
  language: string,
): { unit: "kb" | "mb"; value: string } {
  const kb = bytesPerSecond / 1000;
  const mb = kb / 1000;
  const unit = kb >= 999.5 ? "mb" : "kb";
  const amount = unit === "mb" ? mb : kb;
  // One decimal below ten, whole numbers above: "0,4", "9,8", "12", "640".
  const digits = amount < 9.95 ? 1 : 0;
  const value = amount.toLocaleString(language, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  return { unit, value };
}

/** Anchor positioning draws the trunk; a WebView without it gets no tree flow at all. */
export function treeFlowSupported(): boolean {
  return typeof CSS !== "undefined" && typeof CSS.supports === "function"
    ? CSS.supports("anchor-name", "--tree-flow")
    : false;
}
