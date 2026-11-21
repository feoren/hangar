// Versions and icons of the installed programs. Both queries are slow and optional: errors stay
// silent (the display is simply missing then), and when switched off nothing is queried at all.

import { useCallback, useEffect, useRef, useState } from "react";

import { api } from "./api";
import {
  NO_PROGRAM_INFO,
  parseIcons,
  parseVersions,
  retryVersions,
  VERSION_RETRY_MS,
  type ProgramIcons,
  type ProgramVersions,
} from "./programs";

export interface ProgramInfo {
  versions: ProgramVersions;
  icons: ProgramIcons;
  /** The version query for the current program list has finished (or is switched off). */
  versionsSettled: boolean;
  /** Query versions again, e.g. after an update session. */
  refreshVersions: () => void;
}

/**
 * `installedKey` (see profiles.ts) triggers a new query as soon as the set of installed programs
 * changes. Until the new response arrives, the old details stay in place. An installed program
 * without a version is asked again a few times, a minute apart (see `retryVersions`).
 */
export function usePrograms(
  installedKey: string,
  showVersions: boolean,
  showIcons: boolean,
  /** Only query after restoring: both queries launch programs or read files. */
  ready = true,
): ProgramInfo {
  /** Counter for repeated version queries with the same program list. */
  const [revision, setRevision] = useState(0);
  const versions = useBackendMap(
    installedKey,
    showVersions,
    ready,
    api.detectVersions,
    parseVersions,
    revision,
  );
  const icons = useBackendMap(installedKey, showIcons, ready, api.programIcons, parseIcons, 0);
  const refreshVersions = useCallback(() => {
    setRevision((current) => current + 1);
  }, []);
  /** Tries made for the missing versions of one program list. */
  const retries = useRef({ key: "", count: 0 });
  useEffect(() => {
    if (!showVersions || versions.settledKey !== installedKey) return;
    if (retries.current.key !== installedKey) retries.current = { key: installedKey, count: 0 };
    if (!retryVersions(installedKey, versions.map, retries.current.count)) return;
    const timer = window.setTimeout(() => {
      retries.current.count += 1;
      setRevision((current) => current + 1);
    }, VERSION_RETRY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [showVersions, installedKey, versions.map, versions.settledKey]);
  return {
    versions: versions.map,
    icons: icons.map,
    versionsSettled: !showVersions || versions.settledKey === installedKey,
    refreshVersions,
  };
}

function useBackendMap(
  installedKey: string,
  enabled: boolean,
  ready: boolean,
  load: () => Promise<unknown>,
  parse: (value: unknown) => ReadonlyMap<string, string>,
  revision: number,
): { map: ReadonlyMap<string, string>; settledKey: string | null } {
  const [map, setMap] = useState(NO_PROGRAM_INFO);
  /** Program list that the last response (or error) was for. */
  const [settledKey, setSettledKey] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled || !ready || installedKey === "") return;
    // An older response must not overwrite a newer one.
    let current = true;
    load()
      .then((value) => {
        if (current) setMap(parse(value));
      })
      .catch(() => undefined)
      .finally(() => {
        if (current) setSettledKey(installedKey);
      });
    return () => {
      current = false;
    };
    // `revision` only triggers a new query; the value itself is not needed.
  }, [installedKey, enabled, ready, load, parse, revision]);

  return { map: enabled ? map : NO_PROGRAM_INFO, settledKey };
}
