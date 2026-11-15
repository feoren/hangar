// Dictation in the window: the backend's state, the input level for the recording dot, the hold
// key (Ctrl+Shift+Space) and pasting the finished text into the session named on key down.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { api, DICTATION_LEVEL, DICTATION_STATUS, DICTATION_TEXT, errorText } from "./api";
import {
  canStart,
  EMPTY_DICTATION_STATUS,
  parseDictationStatus,
  parseDictationText,
  parseLevel,
  type DictationModel,
  type DictationPrefs,
  type DictationStatus,
  type DictationText,
} from "./dictation";
import { KeyHold } from "./shortcuts";

/** Listens to a backend event for as long as the component lives; no-op outside Tauri. */
function useBackendEvent(name: string, onPayload: (payload: unknown) => void): void {
  const handler = useRef(onPayload);
  useEffect(() => {
    handler.current = onPayload;
  });
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    try {
      listen(name, (event) => {
        handler.current(event.payload);
      })
        .then((unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there are no backend events.
    }
    return () => {
      disposed = true;
      stop?.();
    };
  }, [name]);
}

/** The dictation state for `model`, kept fresh by the backend's events. */
export function useDictationStatus(model: DictationModel): DictationStatus {
  const [status, setStatus] = useState<DictationStatus>(EMPTY_DICTATION_STATUS);
  useBackendEvent(DICTATION_STATUS, (payload) => {
    const parsed = parseDictationStatus(payload);
    if (parsed !== null) setStatus(parsed);
  });
  useEffect(() => {
    let current = true;
    api
      .dictationStatus(model)
      .then((value) => {
        const parsed = parseDictationStatus(value);
        if (current && parsed !== null) setStatus(parsed);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [model]);
  return status;
}

// The level arrives ten times a second while recording; only the dot re-renders for it.
let level = 0;
const levelListeners = new Set<() => void>();
let levelStop: (() => void) | null = null;

function subscribeLevel(onChange: () => void): () => void {
  levelListeners.add(onChange);
  if (levelListeners.size === 1) {
    let disposed = false;
    try {
      listen(DICTATION_LEVEL, (event) => {
        level = parseLevel(event.payload);
        for (const listener of levelListeners) listener();
      })
        .then((unlisten) => {
          if (disposed) unlisten();
          else levelStop = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri: the meter stays at zero.
    }
    levelStop = () => {
      disposed = true;
    };
  }
  return () => {
    levelListeners.delete(onChange);
    if (levelListeners.size === 0) {
      levelStop?.();
      levelStop = null;
      level = 0;
    }
  };
}

export function useDictationLevel(): number {
  return useSyncExternalStore(subscribeLevel, () => level);
}

/** A start that did not work, for a short notice on the pane. */
export type DictationProblem = "noMicrophone" | "permissionDenied" | "microphone" | "other";

function problemOf(error: unknown): DictationProblem {
  const kind = errorText(error);
  return kind === "noMicrophone" || kind === "permissionDenied" || kind === "microphone"
    ? kind
    : "other";
}

interface KeyOptions {
  prefs: DictationPrefs;
  status: DictationStatus;
  /** The session the text goes to, read on key down; `null`: nothing to dictate into. */
  target: () => number | null;
  /** A dialog is open: the key does nothing. */
  blocked: () => boolean;
  /** The key was held but no model is installed yet: show the setup. */
  onSetup: () => void;
  onText: (text: DictationText) => void;
  onProblem: (problem: DictationProblem, session: number) => void;
}

/**
 * Holding Ctrl+Shift+Space records, letting go transcribes. Listens on the window while
 * dictation is on; the terminal leaves the chord to the app meanwhile (`terminalKeyAction`).
 */
export function useDictationKey(options: KeyOptions): void {
  const latest = useRef(options);
  useEffect(() => {
    latest.current = options;
  });
  useBackendEvent(DICTATION_TEXT, (payload) => {
    const text = parseDictationText(payload);
    if (text !== null && text.text !== "") latest.current.onText(text);
  });

  const enabled = options.prefs.enabled;
  useEffect(() => {
    if (!enabled) return;
    // Stop waits for the start: a quick tap must not leave the microphone open because the stop
    // reached the backend before the start did.
    let started: Promise<unknown> = Promise.resolve();
    const hold = new KeyHold(
      () => {
        const { prefs, status, target, blocked, onSetup, onProblem } = latest.current;
        const session = target();
        if (session === null || blocked()) return;
        if (status.state.kind === "notInstalled") {
          onSetup();
          return;
        }
        if (!canStart(status.state)) return;
        started = api.dictationStart(session, prefs.model, prefs.language).catch((error) => {
          onProblem(problemOf(error), session);
        });
      },
      () => {
        void started.then(() => api.dictationStop()).catch(() => undefined);
      },
    );
    const onKeyDown = (event: KeyboardEvent) => {
      if (hold.keydown(event)) event.preventDefault();
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (hold.keyup(event)) event.preventDefault();
    };
    const onBlur = () => {
      hold.release();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
      // Turned off while held: the microphone closes.
      hold.release();
    };
  }, [enabled]);
}
