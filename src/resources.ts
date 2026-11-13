// CPU, memory and GPU of each session's whole process tree (Rust: `SessionResources` in
// src-tauri/src/resources.rs). The window asks every RESOURCE_POLL_MS while it is visible and the
// setting is on; the session's row and peek show the numbers, the row an outlier as a small mark.

/** How often the window asks while it is visible. */
export const RESOURCE_POLL_MS = 2000;

export interface SessionResources {
  /** Share of the whole machine in percent; `null` until the second reading. */
  cpu: number | null;
  /** Resident memory in bytes. */
  memory: number;
  /** Busiest GPU engine of the tree in percent; `null` without GPU numbers (Windows only). */
  gpu: number | null;
  /** Dedicated GPU memory of the tree in bytes; `null` like `gpu`. */
  gpuMemory: number | null;
  /** Above 90 % CPU for 30 s, or more than 4 GB of memory. */
  outlier: boolean;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

/** An optional count: missing (older backends) and `null` are `null`, anything else malformed. */
function optionalCount(value: unknown): number | null | undefined {
  if (value === undefined || value === null) return null;
  return isCount(value) ? value : undefined;
}

/** The backend's answer per session id; `null` for anything malformed. */
export function parseResources(value: unknown): Map<number, SessionResources> | null {
  if (!Array.isArray(value)) return null;
  const map = new Map<number, SessionResources>();
  const items: readonly unknown[] = value;
  for (const item of items) {
    if (typeof item !== "object" || item === null) return null;
    const id: unknown = Reflect.get(item, "id");
    const cpu: unknown = Reflect.get(item, "cpu");
    const memory: unknown = Reflect.get(item, "memory");
    const outlier: unknown = Reflect.get(item, "outlier");
    const gpu = optionalCount(Reflect.get(item, "gpu"));
    const gpuMemory = optionalCount(Reflect.get(item, "gpuMemory"));
    if (!isCount(id) || !Number.isInteger(id) || !isCount(memory)) return null;
    if (cpu !== null && !isCount(cpu)) return null;
    if (typeof outlier !== "boolean") return null;
    if (gpu === undefined || gpuMemory === undefined) return null;
    map.set(id, {
      cpu: cpu === null ? null : Math.min(cpu, 100),
      memory,
      gpu: gpu === null ? null : Math.min(gpu, 100),
      gpuMemory,
      outlier,
    });
  }
  return map;
}

/** Equal numbers as shown, so an unchanged session keeps its object and does not re-render. */
export function sameResources(a: SessionResources, b: SessionResources): boolean {
  return (
    a.cpu === b.cpu &&
    a.memory === b.memory &&
    a.gpu === b.gpu &&
    a.gpuMemory === b.gpuMemory &&
    a.outlier === b.outlier
  );
}

const MB = 1024 * 1024;
const GB = 1024 * MB;

/** "340 MB", "4.2 GB" in the UI language's units ("340 Mo" in French). */
export function formatMemory(bytes: number, language: string): string {
  const giga = bytes >= GB;
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: giga ? "gigabyte" : "megabyte",
    unitDisplay: "short",
    maximumFractionDigits: giga ? 1 : 0,
  }).format(bytes / (giga ? GB : MB));
}

/** "12 %"; below 1 % with one decimal, so a quiet session does not read as idle. */
export function formatCpu(percent: number, language: string): string {
  return new Intl.NumberFormat(language, {
    style: "percent",
    maximumFractionDigits: percent > 0 && percent < 1 ? 1 : 0,
  }).format(percent / 100);
}

/**
 * The GPU part, "30 % 1.2 GB", "30 %" or "1.2 GB"; `null` unless the tree really uses the GPU
 * (some load or some dedicated memory), so a session that never touches it shows nothing.
 */
export function formatGpu(resources: SessionResources, language: string): string | null {
  const load = resources.gpu !== null && resources.gpu > 0 ? resources.gpu : null;
  const memory =
    resources.gpuMemory !== null && resources.gpuMemory > 0 ? resources.gpuMemory : null;
  if (load === null && memory === null) return null;
  return [
    load === null ? null : formatCpu(load, language),
    memory === null ? null : formatMemory(memory, language),
  ]
    .filter((part) => part !== null)
    .join(" ");
}

/** The values of the row's resource line; `cpu` is `null` before the second reading. */
export interface ResourceParts {
  cpu: string | null;
  memory: string;
  gpu: string | null;
}

export function resourceParts(resources: SessionResources, language: string): ResourceParts {
  return {
    cpu: resources.cpu === null ? null : formatCpu(resources.cpu, language),
    memory: formatMemory(resources.memory, language),
    gpu: formatGpu(resources, language),
  };
}
