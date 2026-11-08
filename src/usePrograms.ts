// Versions and icons of the installed programs. Both queries are slow and optional: errors stay
// silent (the display is simply missing then), and when switched off nothing is queried at all.

import { useCallback, useEffect, useState } from "react";

import { api } from "./api";
import {
  NO_PROGRAM_INFO,
  parseIcons,
  parseVersions,
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
 * changes. Until the new response arrives, the old details stay in place.
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
