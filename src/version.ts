// Version scheme following https://de.wikipedia.org/wiki/Versionsnummer:
// Major.Minor.Revision-Build, e.g. 0.12.224-1548.
// Internally (Tauri, Cargo, npm, updater) the build number is SemVer build metadata after "+",
// because Tauri derives the four-part Windows file version 0.12.224.1548 from it. "-" is displayed.
// See docs/wiki/pages/decisions/0002-version-scheme.md.

export interface AppVersion {
  major: number;
  minor: number;
  revision: number;
  build: number | null;
}

const PATTERN = /^v?(\d+)\.(\d+)\.(\d+)(?:[+-](\d+))?$/;

/** Reads "0.12.224+1548", "0.12.224-1548", "v0.12.224" or "0.12.224"; otherwise `null`. */
export function parseVersion(text: string): AppVersion | null {
  const match = PATTERN.exec(text.trim());
  if (!match) return null;
  const [, major, minor, revision, build] = match;
  if (major === undefined || minor === undefined || revision === undefined) return null;
  return {
    major: Number(major),
    minor: Number(minor),
    revision: Number(revision),
    build: build === undefined ? null : Number(build),
  };
}

/** Display form "0.12.224-1548"; without a build number "0.12.224". Unreadable input stays as is. */
export function formatVersion(text: string): string {
  const version = parseVersion(text);
  if (!version) return text;
  const base = `${String(version.major)}.${String(version.minor)}.${String(version.revision)}`;
  return version.build === null ? base : `${base}-${String(version.build)}`;
}

/**
 * Adds the build number if the version carries none itself (dev build: tauri.conf.json only knows
 * X.Y.Z). A version with a build number stays unchanged, as does unreadable input.
 */
export function withBuild(text: string, build: number | null): string {
  const version = parseVersion(text);
  if (!version || version.build !== null || build === null) return text;
  return `${formatVersion(text)}+${String(build)}`;
}

/** Splits for display with a badge: the number "0.12.125-5655" and whether "beta" belongs to it. */
export function versionParts(text: string): { number: string; beta: boolean } {
  const version = parseVersion(text);
  return { number: formatVersion(text), beta: version !== null && version.major === 0 };
}

/**
 * Display for users: "0.12.125-5655 beta". While the major version is 0, Hangar counts as beta;
 * from 1.0.0 on the suffix is dropped. `betaLabel` comes from the translation. Unreadable input stays.
 */
export function displayVersion(text: string, betaLabel: string): string {
  const version = parseVersion(text);
  if (!version) return text;
  const number = formatVersion(text);
  return version.major === 0 ? `${number} ${betaLabel}` : number;
}
