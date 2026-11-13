// Load of the whole machine for the sidebar's system bar (Rust: `SystemUsage` in
// src-tauri/src/system_usage.rs). Pure: parsing, the last minute of each value and the
// sparkline's shape are testable without a DOM; useSystemUsage.ts asks the backend.

import type { Translate } from "./i18n";
import { formatMemory } from "./resources";

/** How often the window asks while it is visible. */
export const SYSTEM_POLL_MS = 1500;
/** Readings a sparkline keeps: the last minute, plus the one that scrolls out at its left edge. */
export const HISTORY_LENGTH = Math.ceil(60_000 / SYSTEM_POLL_MS) + 1;
/** From this share on, a gauge turns to the warning color. */
export const WARN_PERCENT = 85;

export const GAUGES = ["cpu", "memory", "gpu"] as const;
export type Gauge = (typeof GAUGES)[number];

/**
 * The shares in percent (0 to 100), `null` where the platform cannot tell, which hides that
 * gauge; and the amounts behind RAM and GPU in bytes, `null` where unknown.
 */
export interface SystemUsage extends Readonly<Record<Gauge, number | null>> {
  readonly memoryUsed: number | null;
  readonly memoryTotal: number | null;
  /** Dedicated GPU memory in use on all adapters. */
  readonly gpuMemory: number | null;
}

const AMOUNTS = ["memoryUsed", "memoryTotal", "gpuMemory"] as const;

/** One gauge's readings, oldest first, at most HISTORY_LENGTH. */
export type History = readonly number[];

export interface SystemState {
  usage: SystemUsage;
  history: Readonly<Record<Gauge, History>>;
  /** Readings taken so far; a new one restarts the sparklines' scroll. */
  tick: number;
}

export const EMPTY_STATE: SystemState = {
  usage: {
    cpu: null,
    memory: null,
    gpu: null,
    memoryUsed: null,
    memoryTotal: null,
    gpuMemory: null,
  },
  history: { cpu: [], memory: [], gpu: [] },
  tick: 0,
};

function share(value: unknown): number | null | undefined {
  if (value === null) return null;
  if (typeof value !== "number" || !Number.isFinite(value)) return undefined;
  return Math.min(100, Math.max(0, value));
}

/** A byte count; missing (older backends) and `null` are `null`, anything else malformed. */
function amount(value: unknown): number | null | undefined {
  if (value === undefined || value === null) return null;
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return undefined;
  return value;
}

/** The backend's answer; `null` for anything malformed. */
export function parseSystemUsage(value: unknown): SystemUsage | null {
  if (typeof value !== "object" || value === null) return null;
  const cpu = share(Reflect.get(value, "cpu"));
  const memory = share(Reflect.get(value, "memory"));
  const gpu = share(Reflect.get(value, "gpu"));
  const memoryUsed = amount(Reflect.get(value, "memoryUsed"));
  const memoryTotal = amount(Reflect.get(value, "memoryTotal"));
  const gpuMemory = amount(Reflect.get(value, "gpuMemory"));
  if (cpu === undefined || memory === undefined || gpu === undefined) return null;
  if (memoryUsed === undefined || memoryTotal === undefined || gpuMemory === undefined) {
    return null;
  }
  return { cpu, memory, gpu, memoryUsed, memoryTotal, gpuMemory };
}

export function sameUsage(a: SystemUsage, b: SystemUsage): boolean {
  return (
    GAUGES.every((gauge) => a[gauge] === b[gauge]) && AMOUNTS.every((key) => a[key] === b[key])
  );
}

/**
 * The amount behind a gauge's share: the memory in use ("19.8 GB") or the GPU's dedicated
 * memory; `null` for the CPU and where unknown or zero.
 */
export function gaugeAmount(gauge: Gauge, usage: SystemUsage, language: string): string | null {
  const bytes = gauge === "memory" ? usage.memoryUsed : gauge === "gpu" ? usage.gpuMemory : null;
  return bytes === null || bytes <= 0 ? null : formatMemory(bytes, language);
}

/** The amount spelled out for the tooltip: "19.8 GB of 31.9 GB" for RAM, else the amount. */
export function gaugeAmountLabel(
  gauge: Gauge,
  usage: SystemUsage,
  language: string,
  t: Translate,
): string | null {
  const used = gaugeAmount(gauge, usage, language);
  if (used === null) return null;
  if (gauge !== "memory" || usage.memoryTotal === null || usage.memoryTotal <= 0) return used;
  return t("system.amountOf", { used, total: formatMemory(usage.memoryTotal, language) });
}

/** The history with `value` appended; a gauge that went unknown starts over. */
export function pushHistory(history: History, value: number | null): History {
  if (value === null) return history.length === 0 ? history : [];
  const next = [...history, value];
  return next.length > HISTORY_LENGTH ? next.slice(next.length - HISTORY_LENGTH) : next;
}

/**
 * The state after a reading, and whether anything the bar shows changed. The history always
 * moves on; an unchanged reading keeps the same values and is not published, so React does not
 * render for it (the next published reading carries it along).
 */
export function advance(
  state: SystemState,
  usage: SystemUsage,
): { state: SystemState; changed: boolean } {
  const history = {
    cpu: pushHistory(state.history.cpu, usage.cpu),
    memory: pushHistory(state.history.memory, usage.memory),
    gpu: pushHistory(state.history.gpu, usage.gpu),
  };
  const changed = !sameUsage(state.usage, usage);
  return {
    state: { usage: changed ? usage : state.usage, history, tick: state.tick + 1 },
    changed,
  };
}

/**
 * Whether the bar still waits for its second reading: the first one after start has no CPU yet
 * (a share needs two readings), so showing it would put RAM up alone for one poll. A first
 * reading that already has a CPU (the backend kept its baseline, say after the setting was
 * turned back on) shows at once.
 */
export function barPending(usage: SystemUsage, tick: number): boolean {
  return usage.cpu === null && tick < 2;
}

export function gaugeWarns(value: number): boolean {
  return value >= WARN_PERCENT;
}

/** Width of a sparkline in its own units: one unit per reading, so a reading scrolls by one. */
export const SPARK_WIDTH = HISTORY_LENGTH - 1;
/** Height of a sparkline in its own units; 0 % sits at the bottom, 100 % at the top. */
export const SPARK_HEIGHT = 16;

/**
 * The sparkline's line and the area below it as SVG paths: the newest reading at the right
 * edge, one unit per reading to the left. `null` with fewer than two readings.
 */
export function sparkPaths(history: History): { line: string; area: string } | null {
  if (history.length < 2) return null;
  const first = SPARK_WIDTH - (history.length - 1);
  const points = history.map((value, index) => {
    const y = SPARK_HEIGHT - (Math.min(100, Math.max(0, value)) / 100) * SPARK_HEIGHT;
    return `${String(first + index)} ${String(Math.round(y * 100) / 100)}`;
  });
  const line = `M${points.join(" L")}`;
  const area = `${line} L${String(SPARK_WIDTH)} ${String(SPARK_HEIGHT)} L${String(first)} ${String(SPARK_HEIGHT)} Z`;
  return { line, area };
}

/** "23 %" in the UI language's style, always whole. */
export function formatShare(percent: number, language: string): string {
  return new Intl.NumberFormat(language, { style: "percent", maximumFractionDigits: 0 }).format(
    percent / 100,
  );
}
