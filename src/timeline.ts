// Timeline of a session's work tree in the changes panel: checks the answers of the snapshot
// commands (git_snapshots.rs), decides what each entry shows and what is sent with a new
// snapshot. Pure, so it can be tested without the backend or React.
// Contract: docs/wiki/pages/syntheses/work-tree-timeline.md.

import type { AttentionState } from "./attention";
import { backendErrorText, splitBackendError } from "./backendError";
import { CHANGES_VIEWS, isFullSha, type ChangesView } from "./gitHistory";
import type { MessageKey, Translate } from "./i18n";
import { revealInvisible } from "./invisible";

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
  /** The first line the user gave the agent for this step; empty when none was known. */
  prompt: string;
  /** Full sha of the commit checked out when it was taken; `null` when there was none. */
  parent: string | null;
}

/** Why the newest step was not saved. */
export type Skipped =
  | { reason: "tooManyFiles"; limit: number }
  | { reason: "tooLarge"; fileMib: number; totalMib: number }
  | { reason: "failed"; message: string };

export interface SnapshotList {
  isRepo: boolean;
  /** Root of the work tree as a native path to show; empty outside a repository. */
  root: string;
  /** Full sha of the commit checked out now; `null` while the branch has no commit. */
  head: string | null;
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
  const parent: unknown = Reflect.get(entry, "parent");
  if (parent !== null && !isFullSha(parent)) return null;
  if (number === null || number === 0 || !isFullSha(sha)) return null;
  if (createdAt === null || files === null || typeof prompt !== "string") return null;
  if (kind !== "step" && kind !== "undo") return null;
  return { number, sha, createdAt, files, kind, prompt, parent };
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
  const root: unknown = Reflect.get(answer, "root");
  const head: unknown = Reflect.get(answer, "head");
  if (typeof isRepo !== "boolean" || !Array.isArray(snapshots)) return null;
  if (others === null || typeof busy !== "boolean") return null;
  if (typeof root !== "string" || (head !== null && !isFullSha(head))) return null;
  return {
    isRepo,
    root,
    head,
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

/** The prompt sent with a new snapshot: the line on one row, cut, or `null` without one. */
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

/**
 * Since the snapshot was taken another commit was checked out (a branch switch, a reset): going
 * back then restores the files only, HEAD stays where it is now.
 */
export function headMoved(list: SnapshotList, entry: Snapshot): boolean {
  return entry.parent !== list.head;
}

/**
 * Whether the window takes snapshots of a session (the trigger in App.tsx): only when it runs an
 * agent (`runsAgent`, no update sessions) and has started (placeholders have negative IDs).
 * Decided by the profile, not by the running process: an exited agent session keeps its timeline.
 */
export function takesSnapshots(id: number, agent: boolean): boolean {
  return id >= 0 && agent;
}

/**
 * The "Timeline" view of the changes panel: hidden where no snapshot is ever taken (shells,
 * placeholders), a hint instead of the list while working-state detection is off (the trigger
 * never fires then), else shown.
 */
export type TimelineAvailability = "hidden" | "detectionOff" | "shown";

export function timelineAvailability(snapshots: boolean, detection: boolean): TimelineAvailability {
  if (!snapshots) return "hidden";
  return detection ? "shown" : "detectionOff";
}

/** The views the changes panel offers. */
export function changesViews(timeline: TimelineAvailability): readonly ChangesView[] {
  return timeline === "hidden"
    ? CHANGES_VIEWS.filter((view) => view !== "timeline")
    : CHANGES_VIEWS;
}

/** The view shown: a remembered "timeline" falls back to the open changes where it is hidden. */
export function shownChangesView(view: ChangesView, timeline: TimelineAvailability): ChangesView {
  return view === "timeline" && timeline === "hidden" ? "open" : view;
}

/**
 * A prompt that does not echo what is typed, such as `Password:` or `sudo`'s question: the row
 * under the cursor ends with the cue and a colon, with nothing typed after it.
 */
const SECRET_CUE =
  /(pass(word|phrase|wort)|kennwort|\bpin\b|token|secret|mot de passe|contraseña|senha|пароль|密码|パスワード)[^:：]*[:：]\s*$/iu;

/**
 * The prompt kept for a step's snapshot, which lands in plain text in the side repository. Until
 * the step's working phase begins, the last line the user submits while the agent waits for a
 * task (`idle`) is the candidate, so a command without work (`/clear`, `/model`) gives way to the
 * real prompt after it; a line answered while the agent asks something (`attention`) only fills
 * an empty slot. When the work starts (`workStarted`) the candidate is fixed for the step. A line
 * submitted while it works (an answer to a tool's question, a password) and a line typed at a
 * prompt that hides its input are never kept. Cut on arrival, so no long text stays in memory.
 */
export class StepPrompt {
  private text: string | null = null;
  /** The step's working phase has begun: the prompt is fixed until the snapshot takes it. */
  private started = false;

  /**
   * A line submitted with Enter, the attention state at that moment and the screen row under the
   * cursor before the Enter (the program's prompt and the echo of the line).
   */
  submitted(line: string, state: AttentionState, row: string): void {
    if (this.started || state === "working" || SECRET_CUE.test(row)) return;
    if (state === "attention" && this.text !== null) return;
    this.text = snapshotPrompt({ text: line }) ?? this.text;
  }

  /**
   * The agent began working: the step's prompt is the candidate held now. Without one nothing is
   * fixed, because work nobody asked for (a program's start-up output) ends without a snapshot,
   * so no `take` would release the slot for the real prompt.
   */
  workStarted(): void {
    if (this.text !== null) this.started = true;
  }

  /** The step's prompt for its snapshot; the next step starts without one. */
  take(): string | null {
    const text = this.text;
    this.text = null;
    this.started = false;
    return text;
  }
}

type TimelineFailure = Extract<
  MessageKey,
  "timeline.failed" | "timeline.diffFailed" | "timeline.restoreFailed"
>;

/** Refusals of `snapshot_*` with a stable code (SnapshotError), and what their detail names. */
const SNAPSHOT_ERRORS: ReadonlyMap<string, { key: MessageKey; detail: "path" | "reason" | null }> =
  new Map<string, { key: MessageKey; detail: "path" | "reason" | null }>([
    ["snapshot_working", { key: "timeline.errorWorking", detail: null }],
    ["snapshot_folder_in_the_way", { key: "timeline.errorFolderInTheWay", detail: "path" }],
    ["snapshot_ignored_in_the_way", { key: "timeline.errorIgnoredInTheWay", detail: "path" }],
    ["snapshot_cannot_save_current", { key: "timeline.errorCannotSaveCurrent", detail: "reason" }],
    ["snapshot_linked", { key: "timeline.errorLinked", detail: "path" }],
    ["snapshot_filtered", { key: "timeline.errorFiltered", detail: null }],
    ["snapshot_symlink", { key: "timeline.errorSymlink", detail: "path" }],
    ["snapshot_too_large_to_check", { key: "timeline.errorTooLargeToCheck", detail: "path" }],
    ["snapshot_outside", { key: "timeline.errorOutside", detail: "path" }],
  ]);

/**
 * A failed timeline call in the window's own words: a known snapshot refusal as its own sentence
 * (with the path or git's reason it names), anything else in the call's general sentence.
 */
export function timelineErrorText(error: unknown, failure: TimelineFailure, t: Translate): string {
  if (typeof error !== "string") {
    return t(failure, { error: error instanceof Error ? error.message : t("error.unknown") });
  }
  const { code, detail } = splitBackendError(error);
  const known = code === null ? undefined : SNAPSHOT_ERRORS.get(code);
  if (known !== undefined) {
    if (known.detail === null) return t(known.key);
    // Without the detail the sentence would end in nothing: the general one says enough.
    if (detail !== null && detail.trim() !== "") {
      // git's reason may span lines and reads fine on one; a path shows every character.
      const shown = known.detail === "reason" ? detail.replace(/\s+/g, " ") : detail;
      return t(known.key, { [known.detail]: revealInvisible(shown.trim()) });
    }
  }
  return t(failure, { error: backendErrorText(error) });
}

/** Why going back did not work, as the timeline shows it. */
export function restoreErrorText(error: unknown, t: Translate): string {
  return timelineErrorText(error, "timeline.restoreFailed", t);
}
