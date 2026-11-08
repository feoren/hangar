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
