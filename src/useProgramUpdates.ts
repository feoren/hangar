// New versions of the installed programs. Runs after the version query, at startup and every
// 6 hours; "Check now" bypasses the backend's disk cache. When switched off, nothing is queried.

import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import {
  NO_UPDATES,
  parseProgramUpdates,
  UPDATE_CHECK_INTERVAL_MS,
  type ProgramUpdates,
} from "./programUpdates";

const FIRST_CHECK_DELAY_MS = 1500;

export interface ProgramUpdateState {
  updates: ProgramUpdates;
  checking: boolean;
  /** Error from the last "Check now"; automatic checks stay silent. */
  error: string | null;
  /** Time of the last successful check (ms), `null` before that. */
  checkedAt: number | null;
  checkNow: () => void;
  /**
   * Check again without forcing the network, e.g. after an update session: the backend detects
   * the installed version anew, the latest one comes from its disk cache.
   */
  recheck: () => void;
}

/**
 * `installedKey`: a new check as soon as the set of installed programs changes.
 * `ready`: the version query has finished; the backend takes its result from the cache.
 */
export function useProgramUpdates(
  installedKey: string,
  enabled: boolean,
  ready: boolean,
): ProgramUpdateState {
  const [updates, setUpdates] = useState<ProgramUpdates>(NO_UPDATES);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkedAt, setCheckedAt] = useState<number | null>(null);
  /** Only the most recent request may set the result. */
  const generation = useRef(0);

  const run = useCallback((force: boolean) => {
    generation.current += 1;
    const mine = generation.current;
    setChecking(true);
    if (force) setError(null);
    api
      .checkProgramUpdates(force)
      .then((value) => {
        if (mine !== generation.current) return;
        setUpdates(parseProgramUpdates(value));
        setCheckedAt(Date.now());
      })
      .catch((e: unknown) => {
        if (mine === generation.current && force) setError(errorText(e));
      })
      .finally(() => {
        if (mine === generation.current) setChecking(false);
      });
  }, []);

  useEffect(() => {
    if (!enabled || !ready || installedKey === "") return;
    // Shortly after startup, so the query does not compete with building the window.
    const first = window.setTimeout(() => {
      run(false);
    }, FIRST_CHECK_DELAY_MS);
    const interval = window.setInterval(() => {
      run(false);
    }, UPDATE_CHECK_INTERVAL_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(interval);
    };
  }, [enabled, ready, installedKey, run]);

  const checkNow = useCallback(() => {
    run(true);
  }, [run]);

  const recheck = useCallback(() => {
    if (enabled) run(false);
  }, [enabled, run]);

  return {
    updates: enabled ? updates : NO_UPDATES,
    checking: enabled && checking,
    error: enabled ? error : null,
    checkedAt,
    checkNow,
    recheck,
  };
}
