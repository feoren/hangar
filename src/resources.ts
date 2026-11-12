// CPU and memory of each session's whole process tree (Rust: `SessionResources` in
// src-tauri/src/resources.rs). The window asks every RESOURCE_POLL_MS while it is visible and the
// setting is on; the session's peek shows the numbers and its row an outlier as a small mark.

import type { Translate } from "./i18n";

/** How often the window asks while it is visible. */
export const RESOURCE_POLL_MS = 2000;

export interface SessionResources {
  /** Share of the whole machine in percent; `null` until the second reading. */
  cpu: number | null;
  /** Resident memory in bytes. */
  memory: number;
  /** Above 90 % CPU for 30 s, or more than 4 GB of memory. */
  outlier: boolean;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
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
    if (!isCount(id) || !Number.isInteger(id) || !isCount(memory)) return null;
    if (cpu !== null && !isCount(cpu)) return null;
    if (typeof outlier !== "boolean") return null;
    map.set(id, { cpu: cpu === null ? null : Math.min(cpu, 100), memory, outlier });
  }
  return map;
}

/** Equal numbers as shown, so an unchanged session keeps its object and does not re-render. */
export function sameResources(a: SessionResources, b: SessionResources): boolean {
  return a.cpu === b.cpu && a.memory === b.memory && a.outlier === b.outlier;
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

/** "CPU 12 % · 340 MB", or only the memory before the second reading. */
export function resourceText(resources: SessionResources, language: string, t: Translate): string {
  const memory = formatMemory(resources.memory, language);
  if (resources.cpu === null) return t("session.resourcesMemory", { memory });
  return t("session.resources", { cpu: formatCpu(resources.cpu, language), memory });
}
