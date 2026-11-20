// Queued messages: the next instruction for an agent session, written while it still works and
// sent by Hangar once the session is done. One message per session, keyed by the session's stable
// key (workspace.ts), so it survives a restart of the process and of Hangar. Pure, so the state
// machine can be tested; the app owns the state and does the sending (broadcast.ts).
//
// The rules:
// - A message queued in this run is armed: the next end of work with the program gone quiet
//   ("done") sends it, exactly once.
// - A question (bell) or a pending gate decision ("asks") keeps it: the question needs an answer.
// - An exit or a stop by the user keeps it but disarms it, as does a restore after a restart: a
//   freshly started program prints its start-up screen, and that is no task coming to an end.
//   A disarmed message arms again once work starts that follows the user's own input.

import type { AttentionReason, AttentionState } from "./attention";

/** Longer text is cut; a prompt this long is pasted from a file anyway. */
export const MAX_QUEUED_CHARS = 20_000;

/** localStorage key older versions kept the queued texts under; migrated once (mainStore.ts). */
export const QUEUE_STORAGE_KEY = "hangar.queuedMessages";

export interface QueuedMessage {
  text: string;
  /** The next "done" sends it (see the rules above). */
  armed: boolean;
  /** The work ended on a question: the message waits for the answer and the work after it. */
  waiting?: boolean;
}

/** Queued messages per session key. */
export type MessageQueue = ReadonlyMap<string, QueuedMessage>;

export const EMPTY_QUEUE: MessageQueue = new Map();

/** How a working phase ended, for the queue. */
export type PhaseEnd = "done" | "asks" | "other";

/**
 * How a change of the attention state ends a working phase; `null` when it ends none.
 * `endedBy` is the tracker's `endedBy` (known also while the session is watched, where the state
 * only turns "idle"); `gateWaiting`: a gate decision of the session is pending or its agent's
 * hooks report a dialog (ADR 0016).
 */
export function phaseEnd(
  previous: AttentionState,
  next: AttentionState,
  reason: AttentionReason | null,
  endedBy: AttentionReason | null,
  gateWaiting: boolean,
): PhaseEnd | null {
  if (previous !== "working" || next === "working") return null;
  if (gateWaiting || reason === "gate" || reason === "permission") return "asks";
  if (reason === "bell" || endedBy === "bell") return "asks";
  return endedBy === "quiet" ? "done" : "other";
}

/**
 * A quiet end whose screen shows a question or choice (promptGuard.ts `looksLikeQuestion`) asks
 * instead: Enter after the pasted message would confirm the preselected answer.
 */
export function guardedEnd(end: PhaseEnd, showsQuestion: boolean): PhaseEnd {
  return end === "done" && showsQuestion ? "asks" : end;
}

/** Text as it is queued: trailing blank lines dropped, cut to MAX_QUEUED_CHARS. */
export function cleanQueuedText(text: string): string {
  return text.replace(/\s+$/u, "").slice(0, MAX_QUEUED_CHARS);
}

function withEntry(queue: MessageQueue, key: string, entry: QueuedMessage | null): MessageQueue {
  const next = new Map(queue);
  if (entry === null) next.delete(key);
  else next.set(key, entry);
  return next;
}

/** Queues or replaces the session's message; blank text discards it. Armed right away. */
export function queueMessage(queue: MessageQueue, key: string, text: string): MessageQueue {
  const clean = cleanQueuedText(text);
  if (clean.trim() === "") return discardMessage(queue, key);
  return withEntry(queue, key, { text: clean, armed: true });
}

export function discardMessage(queue: MessageQueue, key: string): MessageQueue {
  return queue.has(key) ? withEntry(queue, key, null) : queue;
}

/** Messages read back after a restart: all of them disarmed. */
export function restoredQueue(stored: ReadonlyMap<string, string>): MessageQueue {
  const queue = new Map<string, QueuedMessage>();
  for (const [key, text] of stored) {
    const clean = cleanQueuedText(text);
    if (clean.trim() !== "") queue.set(key, { text: clean, armed: false });
  }
  return queue;
}

/** A working phase started; `byUser`: it follows the user's own input, which arms the message. */
export function workStarted(queue: MessageQueue, key: string, byUser: boolean): MessageQueue {
  const entry = queue.get(key);
  if (entry === undefined) return queue;
  // The answer was given: the message no longer waits for it.
  const armed = entry.armed || byUser;
  if (armed === entry.armed && entry.waiting !== true) return queue;
  return withEntry(queue, key, { text: entry.text, armed });
}

/**
 * A working phase ended. "done" with an armed message hands out its text to send and removes it,
 * so it goes out exactly once; "asks" keeps it, marked as waiting for the answer; "other" keeps it
 * disarmed.
 */
export function phaseEnded(
  queue: MessageQueue,
  key: string,
  end: PhaseEnd,
): { queue: MessageQueue; send: string | null } {
  const entry = queue.get(key);
  if (entry === undefined) return { queue, send: null };
  if (end === "done" && entry.armed)
    return { queue: withEntry(queue, key, null), send: entry.text };
  if (end === "other" && entry.armed) {
    return { queue: withEntry(queue, key, { text: entry.text, armed: false }), send: null };
  }
  if (end === "asks" && entry.waiting !== true) {
    return { queue: withEntry(queue, key, { ...entry, waiting: true }), send: null };
  }
  return { queue, send: null };
}

/** The session at the moment the editor saves. */
export interface SessionNow {
  running: boolean;
  working: boolean;
  /** The tracker's `endedBy`. */
  endedBy: AttentionReason | null;
  gateWaiting: boolean;
  /** The screen shows a question (promptGuard.ts). */
  showsQuestion: boolean;
}

/**
 * Saves the editor's text. A session that is done already (its last work ended quietly, no
 * question on screen, no pending decision) gets it right away instead of waiting for work that
 * will not come; otherwise it is queued.
 */
export function saveMessage(
  queue: MessageQueue,
  key: string,
  text: string,
  now: SessionNow,
): { queue: MessageQueue; send: string | null } {
  const next = queueMessage(queue, key, text);
  if (!now.running || now.working || now.endedBy !== "quiet" || now.gateWaiting) {
    return { queue: next, send: null };
  }
  return phaseEnded(next, key, guardedEnd("done", now.showsQuestion));
}

/** Reads the stored texts; anything malformed is dropped, the rest kept. */
export function parseStoredQueue(value: unknown): Map<string, string> {
  const stored = new Map<string, string>();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return stored;
  for (const [key, text] of Object.entries(value)) {
    if (typeof text === "string" && text.trim() !== "") stored.set(key, text);
  }
  return stored;
}

/** What gets stored: the texts of the sessions `keep` accepts. */
export function storedQueue(
  queue: MessageQueue,
  keep: (key: string) => boolean,
): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [key, entry] of queue) if (keep(key)) result[key] = entry.text;
  return result;
}

/** A session key as the workspace makes them (`crypto.randomUUID`); the backend takes no other. */
const SESSION_KEY = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;

/** What the backend accepts of `entries`: session keys only, texts as they are queued. */
export function persistableQueue(entries: Record<string, string>): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [key, text] of Object.entries(entries)) {
    const clean = cleanQueuedText(text);
    if (SESSION_KEY.test(key) && clean.trim() !== "") result[key] = clean;
  }
  return result;
}

/**
 * The texts an older Hangar kept in localStorage, read once for the migration to the backend
 * (mainStore.ts). The panel and file viewer windows share that storage, so nothing is read from
 * there afterwards.
 */
export function loadLegacyQueue(storage: Pick<Storage, "getItem">): Map<string, string> {
  try {
    const text = storage.getItem(QUEUE_STORAGE_KEY);
    return text === null ? new Map<string, string>() : parseStoredQueue(JSON.parse(text));
  } catch {
    return new Map<string, string>();
  }
}

/** A short one-line form of the text for notes and the screen reader. */
export function queuedPreview(text: string, max = 80): string {
  const line = text.replace(/\s+/gu, " ").trim();
  return line.length <= max ? line : `${line.slice(0, max - 1)}…`;
}
