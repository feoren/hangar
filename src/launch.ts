// Last launched program for Ctrl+Shift+T and the empty state's hint. Only the profile ID is stored,
// per machine; if it is missing or the program can no longer be launched, the first one applies.

const LAST_PROGRAM_KEY = "hangar.lastProgram";

/** Program for "New session" via shortcut: the most recently used, otherwise the first launchable. */
export function pickProgram<T extends { id: string }>(
  startable: readonly T[],
  lastId: string | null,
): T | undefined {
  return startable.find((profile) => profile.id === lastId) ?? startable[0];
}

export function loadLastProgram(): string | null {
  try {
    const value: unknown = localStorage.getItem(LAST_PROGRAM_KEY);
    return typeof value === "string" && value.length > 0 && value.length <= 200 ? value : null;
  } catch {
    return null;
  }
}

export function saveLastProgram(id: string): void {
  try {
    localStorage.setItem(LAST_PROGRAM_KEY, id);
  } catch {
    // Without a stored value, the shortcut launches the first program in the list.
  }
}
