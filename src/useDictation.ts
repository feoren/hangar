// Dictation in the window: the backend's state, the input level for the recording dot, the hold
// key (Ctrl+Shift+Space) and pasting the finished text into the session named on key down.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { api, DICTATION_LEVEL, DICTATION_STATUS, DICTATION_TEXT, errorText } from "./api";
import {
  canStart,
  EMPTY_DICTATION_STATUS,
  endStaleRecording,
  failureAfter,
  LatestOnly,
  parseDictationStatus,
  parseLevel,
  takeDictationTexts,
  type DictationModel,
  type DictationPrefs,
  type DictationState,
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

/**
 * The dictation state for `model`. The backend's signal carries nothing; each one reads the
 * state again, and only the newest answer counts.
 */
export function useDictationStatus(model: DictationModel): DictationStatus {
  const [status, setStatus] = useState<DictationStatus>(EMPTY_DICTATION_STATUS);
  const reads = useRef(new LatestOnly());
  const read = useRef<() => void>(() => undefined);
  useEffect(() => {
    let mounted = true;
    read.current = () => {
      const ticket = reads.current.next();
      api
        .dictationStatus(model)
        .then((value) => {
          const parsed = parseDictationStatus(value);
          if (mounted && reads.current.current(ticket) && parsed !== null) setStatus(parsed);
        })
        .catch(() => undefined);
    };
    read.current();
    return () => {
      mounted = false;
      read.current = () => undefined;
    };
  }, [model]);
  useBackendEvent(DICTATION_STATUS, () => {
    read.current();
  });
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
  /** Off: the key goes to the program in the terminal as before. */
  enabled: boolean;
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
  useBackendEvent(DICTATION_TEXT, () => {
    void takeDictationTexts(api.dictationTakeText, (text) => {
      latest.current.onText(text);
    });
  });
  // Loaded while a recording ran (reloaded with the key held): no key belongs to it any more,
  // and texts nobody took belong to the window before.
  useEffect(() => {
    void endStaleRecording(api.dictationStop, api.dictationTakeText);
  }, []);

  // A failure that shows only after the start (a blocked microphone, a failed recognition)
  // arrives as a state change; its notice goes to the pane the recording was for.
  const previous = useRef<DictationState>(options.status.state);
  const state = options.status.state;
  useEffect(() => {
    const failed = failureAfter(previous.current, state);
    previous.current = state;
    if (failed !== null) latest.current.onProblem(failed.failure, failed.session);
  }, [state]);

  const enabled = options.enabled;
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
        started = api
          .dictationStart(session, prefs.model, prefs.language)
          .catch((error: unknown) => {
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
