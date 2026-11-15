// Local dictation in the window: the backend's state (src-tauri/src/dictation/mod.rs, serde
// camelCase), the user's choices and the checks on what the backend sends.

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
}

/** Rust `DictationText`: what to paste into which session. */
export interface DictationText {
  session: number;
  text: string;
}

/** The user's choices, stored with the settings. */
export interface DictationPrefs {
  /** Off: the key goes to the program in the terminal as before. */
  enabled: boolean;
  model: DictationModel;
  language: DictationLanguage;
}

/**
 * Off until the user turns it on and agrees to the download. `base`: measured 0.5 s for 10 s of
 * speech against 1.7 to 3 s for `small` (docs/wiki/raw/2026-11-12-dictation-measurement.md).
 */
export const DEFAULT_DICTATION: DictationPrefs = {
  enabled: false,
  model: "base",
  language: "auto",
};

export const EMPTY_DICTATION_STATUS: DictationStatus = {
  state: { kind: "notInstalled" },
  models: [],
  source: "huggingface.co",
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
    enabled: typeof raw.enabled === "boolean" ? raw.enabled : DEFAULT_DICTATION.enabled,
    model: isModel(raw.model) ? raw.model : DEFAULT_DICTATION.model,
    language: isLanguage(raw.language) ? raw.language : DEFAULT_DICTATION.language,
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
  return { state, models, source };
}

export function parseDictationText(value: unknown): DictationText | null {
  const raw = record(value);
  if (raw === null || !isSessionId(raw.session) || typeof raw.text !== "string") return null;
  return { session: raw.session, text: raw.text };
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
