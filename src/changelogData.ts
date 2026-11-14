// The changelog's entries, loaded on first use: src/changelog.json is large and only needed when
// What's New opens or a start after an update has to decide whether it does.

import { parseChangelog, type ChangelogEntry } from "./changelog";

let loading: Promise<readonly ChangelogEntry[]> | null = null;

/** Parsed once; invalid entries are dropped (changelog.ts). A failed load is tried again. */
export function loadChangelog(): Promise<readonly ChangelogEntry[]> {
  loading ??= import("./changelog.json?raw").then(
    (module) => parseChangelog(module.default),
    (error: unknown) => {
      loading = null;
      throw error;
    },
  );
  return loading;
}
