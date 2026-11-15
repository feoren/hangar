// Timeline of a session's work tree in the changes panel: checks the answers of the snapshot
// commands (git_snapshots.rs), decides what each entry shows and what is sent with a new
// snapshot. Pure, so it can be tested without the backend or React.
// Contract: docs/wiki/pages/syntheses/work-tree-timeline.md.

import { isFullSha } from "./gitHistory";

/** Emitted by the backend with the session ID whenever that session's snapshots change. */
export const SNAPSHOTS_CHANGED = "snapshots-changed";

/** Characters of the prompt sent with a snapshot; the backend cuts at the same length. */
export const SNAPSHOT_PROMPT_MAX = 200;
/** Characters of the prompt shown on one row; the tooltip has all that was kept. */
export const PROMPT_PREVIEW_MAX = 90;

export type SnapshotKind = "step" | "undo";

export interface Snapshot {
  /** Position in the session's timeline, counting up from 1. */
  number: number;
  sha: string;
  /** Unix seconds. */
  createdAt: number;
  /** Files that differ from the snapshot before (the first one: from the last commit). */
  files: number;
  kind: SnapshotKind;
  /** The agent's last prompt before the step; empty when none was known. */
  prompt: string;
}

/** Why the newest step was not saved. */
export type Skipped =
  | { reason: "tooManyFiles"; limit: number }
  | { reason: "tooLarge"; fileMib: number; totalMib: number }
  | { reason: "failed"; message: string };

export interface SnapshotList {
  isRepo: boolean;
  /** Newest first. */
  snapshots: Snapshot[];
  skipped: Skipped | null;
  /** Other sessions whose folder lies in the same work tree. */
  others: number;
  /** A session in that work tree is working: the backend refuses to go back. */
  busy: boolean;
}

export type RestoreOutcome =
  | { kind: "restored"; files: number; undo: number }
  | { kind: "alreadyThere" }
  /** Writing stopped part way; the state from before is saved as snapshot `undo`. */
  | { kind: "incomplete"; undo: number; message: string };

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : null;
}

function snapshot(value: unknown): Snapshot | null {
  const entry = record(value);
  if (entry === null) return null;
  const number = count(Reflect.get(entry, "number"));
  const sha: unknown = Reflect.get(entry, "sha");
  const createdAt = count(Reflect.get(entry, "createdAt"));
  const files = count(Reflect.get(entry, "files"));
  const kind: unknown = Reflect.get(entry, "kind");
  const prompt: unknown = Reflect.get(entry, "prompt");
  if (number === null || number === 0 || !isFullSha(sha)) return null;
  if (createdAt === null || files === null || typeof prompt !== "string") return null;
  if (kind !== "step" && kind !== "undo") return null;
  return { number, sha, createdAt, files, kind, prompt };
}

function skipped(value: unknown): Skipped | null {
  const entry = record(value);
  if (entry === null) return null;
  const reason: unknown = Reflect.get(entry, "reason");
  if (reason === "tooManyFiles") {
    const limit = count(Reflect.get(entry, "limit"));
    return limit === null ? null : { reason, limit };
  }
  if (reason === "tooLarge") {
    const fileMib = count(Reflect.get(entry, "fileMib"));
    const totalMib = count(Reflect.get(entry, "totalMib"));
    return fileMib === null || totalMib === null ? null : { reason, fileMib, totalMib };
  }
  if (reason === "failed") {
    const message: unknown = Reflect.get(entry, "message");
    return typeof message === "string" ? { reason, message } : null;
  }
  return null;
}

/** Answer of `snapshot_list`; `null` if malformed. Malformed entries are left out. */
export function parseSnapshotList(value: unknown): SnapshotList | null {
  const answer = record(value);
  if (answer === null) return null;
  const isRepo: unknown = Reflect.get(answer, "isRepo");
  const snapshots: unknown = Reflect.get(answer, "snapshots");
  const others = count(Reflect.get(answer, "others"));
  const busy: unknown = Reflect.get(answer, "busy");
  if (typeof isRepo !== "boolean" || !Array.isArray(snapshots)) return null;
  if (others === null || typeof busy !== "boolean") return null;
  return {
    isRepo,
    snapshots: snapshots.map(snapshot).filter((entry): entry is Snapshot => entry !== null),
    skipped: skipped(Reflect.get(answer, "skipped")),
    others,
    busy,
  };
}

/** Answer of `snapshot_restore`; `null` if malformed. */
export function parseRestoreOutcome(value: unknown): RestoreOutcome | null {
  const answer = record(value);
  if (answer === null) return null;
  const kind: unknown = Reflect.get(answer, "kind");
  if (kind === "alreadyThere") return { kind };
  const undo = count(Reflect.get(answer, "undo"));
  if (kind === "incomplete") {
    const message: unknown = Reflect.get(answer, "message");
    return undo === null || typeof message !== "string" ? null : { kind, undo, message };
  }
  if (kind !== "restored") return null;
  const files = count(Reflect.get(answer, "files"));
  return files === null || undo === null ? null : { kind, files, undo };
}

/** The characters of `text` as a reader sees them (an emoji with modifiers stays one). */
function graphemes(text: string): string[] {
  const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
  return Array.from(segmenter.segment(text), (part) => part.segment);
}

/** The prompt sent with a new snapshot: the last prompt's text, cut, or `null` without one. */
export function snapshotPrompt(last: { text: string } | null): string | null {
  const text = last?.text.replace(/\s+/g, " ").trim() ?? "";
  if (text === "") return null;
  return graphemes(text).slice(0, SNAPSHOT_PROMPT_MAX).join("");
}

/**
 * What a row names: the prompt (shortened to one row), "before going back" for an undo point, or
 * a plain "agent step" when no prompt was known.
 */
export type SnapshotTitle = { kind: "prompt"; text: string } | { kind: "undo" } | { kind: "step" };

export function snapshotTitle(entry: Snapshot): SnapshotTitle {
  if (entry.kind === "undo") return { kind: "undo" };
  const text = entry.prompt.trim();
  if (text === "") return { kind: "step" };
  const chars = graphemes(text);
  return {
    kind: "prompt",
    text:
      chars.length > PROMPT_PREVIEW_MAX
        ? `${chars.slice(0, PROMPT_PREVIEW_MAX - 1).join("")}…`
        : text,
  };
}

/** Whether a `snapshots-changed` payload concerns the session `id`. */
export function concernsSession(payload: unknown, id: number): boolean {
  return payload === id;
}

/**
 * Whether "Go back to this state" can be used: the backend refuses while an agent works in the
 * work tree; its own session's state is fresher in the window than in the list.
 */
export function canRestore(list: SnapshotList, working: boolean): boolean {
  return !working && !list.busy;
}
