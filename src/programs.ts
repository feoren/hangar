// Extra details about installed programs (version, icon) that the backend delivers separately from
// the profile list, because determining them takes time. Responses arrive as `unknown` and are
// checked here: only strings or PNG data URLs get through, everything else is dropped.

/** Version per profile ID; if an entry is missing, nothing is shown. */
export type ProgramVersions = ReadonlyMap<string, string>;
/** Icon (PNG as a data URL) per profile ID; if an entry is missing, the letter badge stays. */
export type ProgramIcons = ReadonlyMap<string, string>;

export const NO_PROGRAM_INFO: ReadonlyMap<string, string> = new Map();

const MAX_VERSION_LENGTH = 40;
const PNG_DATA_URL = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;

function entries(value: unknown): [string, unknown][] {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return [];
  return Object.entries(value);
}

/** `{ id: "7.6.9" | null }`: accepts only short, non-empty strings. */
export function parseVersions(value: unknown): ProgramVersions {
  const result = new Map<string, string>();
  for (const [id, version] of entries(value)) {
    if (typeof version !== "string") continue;
    const text = version.trim();
    if (text.length > 0 && text.length <= MAX_VERSION_LENGTH) result.set(id, text);
  }
  return result;
}

/**
 * `{ id: "data:image/png;base64,..." | null }`: PNG data URLs only, so no entry can make the
 * WebView load a foreign address.
 */
export function parseIcons(value: unknown): ProgramIcons {
  const result = new Map<string, string>();
  for (const [id, icon] of entries(value)) {
    if (typeof icon === "string" && PNG_DATA_URL.test(icon)) result.set(id, icon);
  }
  return result;
}

/** "PowerShell 7 · 7.6.9"; without a version just the name. */
export function labelWithVersion(name: string, version: string | undefined): string {
  return version ? `${name} · ${version}` : name;
}

/**
 * Delay before asking again for missing versions: the backend keeps a failed version query for
 * 60 s (`NEGATIVE_TTL` in version.rs) and answers from that until it has passed.
 */
export const VERSION_RETRY_MS = 65_000;

/** Tries per program list, so a program that never reports a version is not started every minute. */
export const MAX_VERSION_RETRIES = 3;

/**
 * Whether to ask again: an installed entry (one id per line of `installedKey`, see profiles.ts)
 * has no version yet, for example because the query ran while the program was being installed
 * or updated, and fewer than {@link MAX_VERSION_RETRIES} tries have been made for this list.
 */
export function retryVersions(
  installedKey: string,
  versions: ProgramVersions,
  retries: number,
): boolean {
  if (retries >= MAX_VERSION_RETRIES || installedKey === "") return false;
  return installedKey.split("\n").some((id) => !versions.has(id));
}
