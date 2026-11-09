// Local claims (team service step A): which files each running session's agent changed, and where
// two of the user's own sessions in one repository collide. The backend observes and compares
// (src-tauri/src/claims.rs, claims_service.rs); this module checks its snapshot and turns it into
// texts for the sidebar marker, the Changes panel and the one-time notice.

import type { Translate } from "./i18n";
import { revealInvisible } from "./invisible";

export type ChangeKind = "added" | "modified" | "deleted" | "untracked" | "conflicted";
/** Ordered from harmless to worst, as in the backend. */
export type OverlapLevel = "none" | "sameArea" | "sameFile" | "overlappingLines";

/** Lines `start..end` (1-based, inclusive) on the side of the session's base commit. */
export interface LineRange {
  start: number;
  end: number;
}

/** One changed file of a session's worktree (Rust: `Claim`). */
export interface Claim {
  path: string;
  kind: ChangeKind;
  wholeFile: boolean;
  ranges: LineRange[];
  area: string | null;
}

export type ClaimState =
  | { kind: "pending" }
  | { kind: "ready" }
  | { kind: "notRepo" }
  | { kind: "failed"; message: string };

export interface SessionClaims {
  sessionId: number;
  state: ClaimState;
  root: string | null;
  base: string | null;
  /** Other sessions in the same folder: they see the same changes. */
  sharedWith: number[];
  claims: Claim[];
  truncated: boolean;
  level: OverlapLevel;
  /** Overlaps of this session in all; the snapshot lists at most 200 of them, the worst first. */
  overlapCount: number;
  overlapsTruncated: boolean;
}

/** One collision seen from `sessionId`; the backend lists each from both sides. */
export interface Overlap {
  sessionId: number;
  otherId: number;
  level: OverlapLevel;
  path: string | null;
  area: string | null;
  lines: LineRange[];
}

/** Payload of `claims-changed` and answer of `claims_snapshot`. */
export interface ClaimsSnapshot {
  enabled: boolean;
  sessions: SessionClaims[];
  overlaps: Overlap[];
}

export const EMPTY_CLAIMS: ClaimsSnapshot = { enabled: false, sessions: [], overlaps: [] };

const LEVELS: readonly OverlapLevel[] = ["none", "sameArea", "sameFile", "overlappingLines"];
const KINDS: readonly ChangeKind[] = ["added", "modified", "deleted", "untracked", "conflicted"];

function field(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null ? Reflect.get(value, key) : undefined;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

function optionalText(value: unknown): string | null | undefined {
  if (value === null) return null;
  return typeof value === "string" ? value : undefined;
}

function list<T>(value: unknown, parse: (item: unknown) => T | null): T[] | null {
  if (!Array.isArray(value)) return null;
  const items: T[] = [];
  for (const item of value) {
    const parsed = parse(item);
    if (parsed === null) return null;
    items.push(parsed);
  }
  return items;
}

function parseLevel(value: unknown): OverlapLevel | null {
  return LEVELS.find((level) => level === value) ?? null;
}

function parseRange(value: unknown): LineRange | null {
  const start = field(value, "start");
  const end = field(value, "end");
  return isCount(start) && isCount(end) && start <= end ? { start, end } : null;
}

function parseClaim(value: unknown): Claim | null {
  const path = field(value, "path");
  const kind = KINDS.find((k) => k === field(value, "kind"));
  const wholeFile = field(value, "wholeFile");
  const ranges = list(field(value, "ranges"), parseRange);
  const area = optionalText(field(value, "area"));
  if (typeof path !== "string" || kind === undefined || typeof wholeFile !== "boolean") return null;
  if (ranges === null || area === undefined) return null;
  return { path, kind, wholeFile, ranges, area };
}

function parseState(value: unknown): ClaimState | null {
  const kind = field(value, "kind");
  if (kind === "pending" || kind === "ready" || kind === "notRepo") return { kind };
  const message = field(value, "message");
  return kind === "failed" && typeof message === "string" ? { kind, message } : null;
}

function parseSession(value: unknown): SessionClaims | null {
  const sessionId = field(value, "sessionId");
  const state = parseState(field(value, "state"));
  const root = optionalText(field(value, "root"));
  const base = optionalText(field(value, "base"));
  const sharedWith = list(field(value, "sharedWith"), (id) => (isCount(id) ? id : null));
  const claims = list(field(value, "claims"), parseClaim);
  const truncated = field(value, "truncated");
  const level = parseLevel(field(value, "level"));
  const overlapCount = field(value, "overlapCount");
  const overlapsTruncated = field(value, "overlapsTruncated");
  if (!isCount(sessionId) || state === null || root === undefined || base === undefined) {
    return null;
  }
  if (sharedWith === null || claims === null || typeof truncated !== "boolean" || level === null) {
    return null;
  }
  if (!isCount(overlapCount) || typeof overlapsTruncated !== "boolean") return null;
  return {
    sessionId,
    state,
    root,
    base,
    sharedWith,
    claims,
    truncated,
    level,
    overlapCount,
    overlapsTruncated,
  };
}

function parseOverlap(value: unknown): Overlap | null {
  const sessionId = field(value, "sessionId");
  const otherId = field(value, "otherId");
  const level = parseLevel(field(value, "level"));
  const path = optionalText(field(value, "path"));
  const area = optionalText(field(value, "area"));
  const lines = list(field(value, "lines"), parseRange);
  if (!isCount(sessionId) || !isCount(otherId) || level === null || lines === null) return null;
  if (path === undefined || area === undefined) return null;
  return { sessionId, otherId, level, path, area, lines };
}

/** Checks the backend's snapshot; `null` if anything is malformed. */
export function parseClaimsSnapshot(value: unknown): ClaimsSnapshot | null {
  const enabled = field(value, "enabled");
  const sessions = list(field(value, "sessions"), parseSession);
  const overlaps = list(field(value, "overlaps"), parseOverlap);
  if (typeof enabled !== "boolean" || sessions === null || overlaps === null) return null;
  return { enabled, sessions, overlaps };
}

/** Same file or overlapping lines: worth a marker and a notice. A shared area only shows in the panel. */
export function isFileLevel(level: OverlapLevel): boolean {
  return level === "sameFile" || level === "overlappingLines";
}

export function claimsOf(snapshot: ClaimsSnapshot, sessionId: number): SessionClaims | null {
  return snapshot.sessions.find((entry) => entry.sessionId === sessionId) ?? null;
}

/** Collisions of one session, worst first (the backend's order). */
export function overlapsOf(snapshot: ClaimsSnapshot, sessionId: number): Overlap[] {
  return snapshot.overlaps.filter((overlap) => overlap.sessionId === sessionId);
}

/** The same for every session at once, in one pass over the snapshot. */
export function overlapsBySession(snapshot: ClaimsSnapshot): Map<number, Overlap[]> {
  const grouped = new Map<number, Overlap[]>();
  for (const overlap of snapshot.overlaps) {
    const list = grouped.get(overlap.sessionId);
    if (list === undefined) grouped.set(overlap.sessionId, [overlap]);
    else list.push(overlap);
  }
  return grouped;
}

/** "line 4" or "lines 4 to 9", joined with commas. */
export function formatLines(ranges: readonly LineRange[], t: Translate): string {
  return ranges
    .map((range) =>
      range.start === range.end
        ? t("claims.line", { line: range.start })
        : t("claims.lines", { start: range.start, end: range.end }),
    )
    .join(", ");
}

/**
 * "Same file as Claude 2: src/x.ts", "Same lines as …", "Same area as …". Paths and areas come
 * from git and show invisible characters as placeholders.
 */
export function overlapText(overlap: Overlap, otherName: string, t: Translate): string {
  const path = revealInvisible(overlap.path ?? "");
  switch (overlap.level) {
    case "overlappingLines":
      return t("claims.sameLines", {
        session: otherName,
        path,
        lines: formatLines(overlap.lines, t),
      });
    case "sameFile":
      return t("claims.sameFile", { session: otherName, path });
    case "sameArea":
    case "none":
      return t("claims.sameArea", {
        session: otherName,
        area: revealInvisible(overlap.area ?? ""),
      });
  }
}

/**
 * Text of a session's sidebar marker: its worst file-level collision, plus how many more there
 * are. `null` when the session collides on no file.
 */
export function markerText(
  overlaps: readonly Overlap[],
  nameOf: (id: number) => string,
  t: Translate,
): string | null {
  const files = overlaps.filter((overlap) => isFileLevel(overlap.level));
  const first = files[0];
  if (first === undefined) return null;
  const text = overlapText(first, nameOf(first.otherId), t);
  return files.length > 1 ? `${text} ${t("claims.moreCount", { count: files.length - 1 })}` : text;
}

/** Same for both sides of a collision: the session pair and the file. */
export function overlapKey(overlap: Overlap): string {
  const low = Math.min(overlap.sessionId, overlap.otherId);
  const high = Math.max(overlap.sessionId, overlap.otherId);
  return `${String(low)}:${String(high)}:${overlap.path ?? ""}`;
}

/**
 * File-level collisions not announced yet, one per session pair and file, seen from the session
 * with the lower id. The caller adds the keys to `seen`, so the same collision is announced once.
 */
export function freshOverlaps(seen: ReadonlySet<string>, snapshot: ClaimsSnapshot): Overlap[] {
  const fresh = new Map<string, Overlap>();
  for (const overlap of snapshot.overlaps) {
    if (!isFileLevel(overlap.level) || overlap.sessionId > overlap.otherId) continue;
    const key = overlapKey(overlap);
    if (!seen.has(key) && !fresh.has(key)) fresh.set(key, overlap);
  }
  return [...fresh.values()];
}

export interface OverlapNotice {
  title: string;
  body: string;
}

/** "Claude 1 and Codex change the same file" with the file and lines as body. */
export function overlapNotice(
  overlap: Overlap,
  nameOf: (id: number) => string,
  t: Translate,
): OverlapNotice {
  const lines = overlap.level === "overlappingLines" ? formatLines(overlap.lines, t) : "";
  const path = revealInvisible(overlap.path ?? "");
  return {
    title: t("claims.noticeTitle", {
      session: nameOf(overlap.sessionId),
      other: nameOf(overlap.otherId),
    }),
    body: lines === "" ? path : `${path}, ${lines}`,
  };
}

/** What a claim's lines say in the panel: the ranges or "whole file". */
export function claimLines(claim: Claim, t: Translate): string {
  return claim.wholeFile || claim.ranges.length === 0
    ? t("claims.wholeFile")
    : formatLines(claim.ranges, t);
}
