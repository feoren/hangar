// Local dictation in the window: the backend's state (src-tauri/src/dictation/mod.rs, serde
// camelCase), the user's choices and the checks on what the backend sends.

import { DEFAULT_DICTATION_KEY, isDictationKey, type DictationKey } from "./shortcuts";

export type DictationModel = "base" | "small";
export type DictationLanguage = "auto" | "de" | "en";

export const DICTATION_MODELS: readonly DictationModel[] = ["base", "small"];
export const DICTATION_LANGUAGES: readonly DictationLanguage[] = ["auto", "de", "en"];

/** Rust `DictationState`. */
export type DictationState =
  | { kind: "notInstalled" }
  | { kind: "downloading"; model: DictationModel; progress: number }
  | { kind: "ready" }
  | { kind: "recording"; session: number }
  | { kind: "transcribing"; session: number }
  | { kind: "noMicrophone" }
  | { kind: "permissionDenied" }
  | { kind: "error"; message: string };

export interface DictationModelInfo {
  id: DictationModel;
  bytes: number;
  installed: boolean;
}

/** Rust `DictationStatus`. */
export interface DictationStatus {
  state: DictationState;
  models: DictationModelInfo[];
  /** Host the models come from, named in the consent dialog. */
  source: string;
  /** The system has an input device; without one the settings say so instead of a download. */
  microphone: boolean;
}

/** Rust `DictationText`: what to paste into which session. */
export interface DictationText {
  session: number;
  text: string;
}

/**
 * Model, language and key, stored with the settings in localStorage. Whether the key is on lives with
 * the backend's preferences (`dictationEnabled`, mainStore.ts): another window that rewrote that
 * could make the next hold of the key open the microphone.
 */
export interface DictationPrefs {
  model: DictationModel;
  language: DictationLanguage;
  /** The key held to dictate. */
  key: DictationKey;
}

/**
 * `base`: measured 0.5 s for 10 s of speech against 1.7 to 3 s for `small`
 * (docs/wiki/raw/2026-11-12-dictation-measurement.md).
 */
export const DEFAULT_DICTATION: DictationPrefs = {
  model: "base",
  language: "auto",
  key: DEFAULT_DICTATION_KEY,
};

/** Until the backend answered: no download offered, and no "no microphone" either. */
export const EMPTY_DICTATION_STATUS: DictationStatus = {
  state: { kind: "notInstalled" },
  models: [],
  source: "huggingface.co",
  microphone: true,
};

function isModel(value: unknown): value is DictationModel {
  return value === "base" || value === "small";
}

function isLanguage(value: unknown): value is DictationLanguage {
  return value === "auto" || value === "de" || value === "en";
}

/** A plain object's fields, read without a cast; `null` for anything else. */
function record(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const fields: Record<string, unknown> = {};
  for (const key of Object.keys(value)) fields[key] = Reflect.get(value, key);
  return fields;
}

export function parseDictationPrefs(value: unknown): DictationPrefs {
  const raw = record(value);
  if (raw === null) return DEFAULT_DICTATION;
  return {
    model: isModel(raw.model) ? raw.model : DEFAULT_DICTATION.model,
    language: isLanguage(raw.language) ? raw.language : DEFAULT_DICTATION.language,
    // Stored before the choice existed, or by a newer Hangar with a key this one lacks.
    key: isDictationKey(raw.key) ? raw.key : DEFAULT_DICTATION.key,
  };
}

function isSessionId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function parseDictationState(value: unknown): DictationState | null {
  const raw = record(value);
  if (raw === null) return null;
  switch (raw.kind) {
    case "notInstalled":
    case "ready":
    case "noMicrophone":
    case "permissionDenied":
      return { kind: raw.kind };
    case "downloading":
      if (!isModel(raw.model) || typeof raw.progress !== "number") return null;
      return {
        kind: "downloading",
        model: raw.model,
        progress: Math.min(1, Math.max(0, raw.progress)),
      };
    case "recording":
    case "transcribing":
      return isSessionId(raw.session) ? { kind: raw.kind, session: raw.session } : null;
    case "error":
      return { kind: "error", message: typeof raw.message === "string" ? raw.message : "" };
    default:
      return null;
  }
}

export function parseDictationStatus(value: unknown): DictationStatus | null {
  const raw = record(value);
  if (raw === null) return null;
  const state = parseDictationState(raw.state);
  if (state === null || !Array.isArray(raw.models)) return null;
  const models: DictationModelInfo[] = [];
  for (const item of raw.models) {
    const model = record(item);
    if (
      model !== null &&
      isModel(model.id) &&
      typeof model.bytes === "number" &&
      typeof model.installed === "boolean"
    ) {
      models.push({ id: model.id, bytes: model.bytes, installed: model.installed });
    }
  }
  const source = typeof raw.source === "string" ? raw.source : EMPTY_DICTATION_STATUS.source;
  if (typeof raw.microphone !== "boolean") return null;
  return { state, models, source, microphone: raw.microphone };
}

export function parseDictationText(value: unknown): DictationText | null {
  const raw = record(value);
  if (raw === null || !isSessionId(raw.session) || typeof raw.text !== "string") return null;
  return { session: raw.session, text: raw.text };
}

/** What `dictation_take_text` answered: the valid transcripts, oldest first. */
export function parseDictationTexts(value: unknown): DictationText[] {
  if (!Array.isArray(value)) return [];
  const texts: DictationText[] = [];
  for (const item of value) {
    const text = parseDictationText(item);
    if (text !== null) texts.push(text);
  }
  return texts;
}

/**
 * The dictation key as the backend keeps it (`dictationEnabled` of the main preferences); an older
 * Hangar kept it in the stored settings text (`dictation.enabled`), read once for the migration.
 */
export function legacyDictationEnabled(settingsText: string | null): boolean {
  let raw: unknown;
  try {
    raw = settingsText === null ? null : JSON.parse(settingsText);
  } catch {
    return false;
  }
  const dictation = record(record(raw)?.dictation);
  return dictation?.enabled === true;
}

/**
 * What goes into the prompt for a transcript: the text and one space, so the next hold continues
 * the sentence instead of gluing onto its last word. Blank text pastes nothing.
 */
export function pasteForm(text: string): string {
  const line = text.trim();
  return line === "" ? "" : `${line} `;
}

/** The input level, 0 to 1; anything else counts as silence. */
export function parseLevel(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
}

/** The session whose pane shows the recording dot, or `null`. */
export function busySession(state: DictationState): number | null {
  return state.kind === "recording" || state.kind === "transcribing" ? state.session : null;
}

/** Whether a key press may start a recording now. */
export function canStart(state: DictationState): boolean {
  return (
    state.kind === "ready" ||
    state.kind === "noMicrophone" ||
    state.kind === "permissionDenied" ||
    state.kind === "error"
  );
}

/** What went wrong after a recording had started, for the notice on its pane. */
export type DictationFailure = "noMicrophone" | "permissionDenied" | "other";

/**
 * The failure to show when the state moves from a recording or transcription of a session to a
 * failure state: the microphone turned out blocked, the recognition failed. `null` otherwise.
 */
export function failureAfter(
  previous: DictationState,
  next: DictationState,
): { session: number; failure: DictationFailure } | null {
  const session = busySession(previous);
  if (session === null) return null;
  switch (next.kind) {
    case "noMicrophone":
    case "permissionDenied":
      return { session, failure: next.kind };
    case "error":
      return { session, failure: "other" };
    default:
      return null;
  }
}

/**
 * Takes the waiting transcripts and hands each non-blank one to `onText`, oldest first. The
 * `dictation-text` signal carries nothing; this is the only way the text reaches the window.
 */
export async function takeDictationTexts(
  take: () => Promise<unknown>,
  onText: (text: DictationText) => void,
): Promise<void> {
  let answer: unknown;
  try {
    answer = await take();
  } catch {
    return;
  }
  for (const text of parseDictationTexts(answer)) {
    if (text.text !== "") onText(text);
  }
}

/**
 * Status reads that answer in another order than they were asked: only the newest answer counts.
 * `next` numbers a read, `current` tells whether its answer is still wanted.
 */
export class LatestOnly {
  private last = 0;

  next(): number {
    this.last += 1;
    return this.last;
  }

  current(ticket: number): boolean {
    return ticket === this.last;
  }
}

/**
 * A window that loads (first start or a reload while the key was held) has no hold running, so a
 * recording the backend still runs belongs to no key: it is stopped. The backend's deadline
 * catches the case where this call never comes. Texts that finished while no window listened are
 * dropped first: otherwise the next dictation would paste them along, long after they were
 * spoken. The stop comes after that, so the text of the recording it ends still arrives.
 */
export async function endStaleRecording(
  stop: () => Promise<unknown>,
  take: () => Promise<unknown>,
): Promise<void> {
  await take().catch(() => undefined);
  await stop().catch(() => undefined);
}

/**
 * Drops the text of a recording whose hold was discarded (right Ctrl turned into Ctrl+C or the
 * like after the recording started). The backend has no command to cancel a recording; the window
 * stops it like any other and does not paste what comes out. Holds are numbered by the caller.
 *
 * The backend signals a text before the state change that ends its transcription, so a text
 * signal that arrives while a hold is marked belongs to that hold. The mark goes away when the
 * state leaves recording and transcribing, when the hold's start failed (nothing will come), or
 * when the backend accepted a later hold (it records one thing at a time, so the discarded one is
 * done).
 */
export class DiscardGate {
  private hold: number | null = null;

  discard(hold: number): void {
    this.hold = hold;
  }

  /**
   * The hold's start failed: nothing will come of it. Returns whether that hold was discarded, so
   * a notice is left out only for it, never for a later hold while an older mark is still set.
   */
  failed(hold: number): boolean {
    if (this.hold !== hold) return false;
    this.hold = null;
    return true;
  }

  started(hold: number): void {
    if (this.hold !== null && this.hold < hold) this.hold = null;
  }

  settled(previous: DictationState, next: DictationState): void {
    if (busySession(previous) !== null && busySession(next) === null) this.hold = null;
  }

  /** Read when a text signal arrives: whether the texts it announces are dropped. */
  get dropping(): boolean {
    return this.hold !== null;
  }
}
