// New versions of the installed programs. Runs after the version query at startup, then every
// 30 minutes and when the window comes back after at least that long; "Check now" bypasses the
// backend's disk cache. The periodic checks also ask the programs for their installed version
// again, so an agent that updated itself loses its offer. When switched off, nothing is queried.

import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import {
  isCheckDue,
  NO_UPDATES,
  parseProgramUpdates,
  UPDATE_TICK_MS,
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
 * `onRedetected`: after a check that read the installed versions again, so the version shown
 * next to each program can follow.
 */
export function useProgramUpdates(
  installedKey: string,
  enabled: boolean,
  ready: boolean,
  onRedetected?: () => void,
): ProgramUpdateState {
  const [updates, setUpdates] = useState<ProgramUpdates>(NO_UPDATES);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkedAt, setCheckedAt] = useState<number | null>(null);
  /** Only the most recent request may set the result. */
  const generation = useRef(0);
  /** Start of the last check, successful or not: a failing network is not asked on every tick. */
  const lastAttempt = useRef<number | null>(null);
  const redetected = useRef(onRedetected);
  useEffect(() => {
    redetected.current = onRedetected;
  });

  const run = useCallback((force: boolean, redetect: boolean) => {
    generation.current += 1;
    const mine = generation.current;
    lastAttempt.current = Date.now();
    setChecking(true);
    if (force) setError(null);
    api
      .checkProgramUpdates(force, redetect)
      .then((value) => {
        if (mine !== generation.current) return;
        setUpdates(parseProgramUpdates(value));
        setCheckedAt(Date.now());
        if (redetect) redetected.current?.();
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
    // Shortly after startup, so the query does not compete with building the window. The
    // version query has just run, so the installed versions are current.
    const first = window.setTimeout(() => {
      run(false, false);
    }, FIRST_CHECK_DELAY_MS);
    const whenDue = () => {
      if (isCheckDue(lastAttempt.current, Date.now())) run(false, true);
    };
    const tick = window.setInterval(whenDue, UPDATE_TICK_MS);
    const onVisible = () => {
      if (document.visibilityState === "visible") whenDue();
    };
    window.addEventListener("focus", whenDue);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(tick);
      window.removeEventListener("focus", whenDue);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [enabled, ready, installedKey, run]);

  const checkNow = useCallback(() => {
    run(true, true);
  }, [run]);

  const recheck = useCallback(() => {
    if (enabled) run(false, false);
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
