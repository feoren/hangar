import type { Profile } from "./api";

/**
 * Launchable entries (program found in PATH) in the order the backend provides. Agents that are
 * not installed are deliberately hidden, so the list stays short.
 */
export function installedProfiles(profiles: readonly Profile[]): Profile[] {
  return profiles.filter((profile) => profile.installed);
}

/**
 * Identifier of the set of installed entries. If it changes (an agent was installed or removed),
 * the app fetches versions and icons again; merely reloading the list triggers nothing.
 */
export function installedKey(profiles: readonly Profile[]): string {
  return installedProfiles(profiles)
    .map((profile) => profile.id)
    .sort()
    .join("\n");
}

/** Same list as before? Then the app keeps the old array and does not re-render. */
export function sameProfiles(a: readonly Profile[], b: readonly Profile[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
