// Last launched program for Ctrl+Shift+T and the empty state's hint. Only the profile ID is stored,
// per machine, by the backend for the main window alone (mainStore.ts); if it is missing or the
// program can no longer be launched, the first one applies.

/** Where an older Hangar kept the ID, read once for the migration (mainStore.ts). */
export const LAST_PROGRAM_STORAGE_KEY = "hangar.lastProgram";
/** Profile IDs as long as the backend takes them (`main_store.rs`, `MAX_PROFILE_ID_UNITS`). */
const MAX_ID_LENGTH = 200;

/** Program for "New session" via shortcut: the most recently used, otherwise the first launchable. */
export function pickProgram<T extends { id: string }>(
  startable: readonly T[],
  lastId: string | null,
): T | undefined {
  return startable.find((profile) => profile.id === lastId) ?? startable[0];
}

/** A stored profile ID: not blank, no control characters, not too long; `null` otherwise. */
export function parseLastProgram(value: unknown): string | null {
  return typeof value === "string" &&
    value.trim() !== "" &&
    value.length <= MAX_ID_LENGTH &&
    // eslint-disable-next-line no-control-regex
    !/[\u0000-\u001f\u007f]/.test(value)
    ? value
    : null;
}

/** The ID an older Hangar kept in `storage`; `null` when missing, broken or blocked. */
export function legacyLastProgram(storage: Pick<Storage, "getItem">): string | null {
  try {
    return parseLastProgram(storage.getItem(LAST_PROGRAM_STORAGE_KEY));
  } catch {
    return null;
  }
}
