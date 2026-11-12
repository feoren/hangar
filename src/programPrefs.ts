// The user's own preferences for terminals and agents: order, visibility and icon per profile ID.
// Applies to the sidebar ("New session", session list, collapsed bar) and File > New session.
// Stored in the settings (settings.ts); profiles.toml stays untouched. IDs that are not
// installed right now are kept, so a program removed for a while keeps its place.

import type { Profile } from "./api";
import { BADGE_COLORS, badgeColor, badgeText, DEFAULT_BADGE_COLOR } from "./format";

/** Custom letters on a color from BADGE_COLORS, or a custom image (PNG, 32 × 32). */
export type IconChoice =
  { kind: "letters"; text: string; color: string } | { kind: "image"; src: string };

/** Without an entry, "Automatic" applies: the program icon, otherwise letters. */
export type IconMode = "auto" | IconChoice["kind"];

export interface ProgramPrefs {
  /** Preferred order; entries not listed follow after. */
  order: string[];
  /** Hidden in the sidebar and menu. */
  hidden: string[];
  icons: Readonly<Record<string, IconChoice>>;
}

export const DEFAULT_PROGRAM_PREFS: ProgramPrefs = { order: [], hidden: [], icons: {} };

/** What a badge looks like, fully resolved. */
export type BadgeLook =
  { kind: "image"; src: string } | { kind: "letters"; text: string; color: string };

const MAX_IDS = 200;
const MAX_ID_LENGTH = 100;
/** A 32 × 32 PNG is rarely larger than 6 KB; the limit protects storage from outliers. */
export const MAX_ICON_DATA_URL = 24_000;
const PNG_DATA_URL = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;
export const MAX_LETTERS = 2;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_ID_LENGTH;
}

function idList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const result: string[] = [];
  for (const id of value) {
    if (validId(id) && !result.includes(id)) result.push(id);
    if (result.length >= MAX_IDS) break;
  }
  return result;
}

/** At most two characters, no control characters, no surrounding whitespace; empty gives `""`. */
export function normalizeLetters(text: string): string {
  const clean = Array.from(text.replace(/\p{C}/gu, "").trim());
  return clean.slice(0, MAX_LETTERS).join("").trim();
}

export function isIconImage(value: unknown): value is string {
  return typeof value === "string" && value.length <= MAX_ICON_DATA_URL && PNG_DATA_URL.test(value);
}

function parseChoice(value: unknown): IconChoice | null {
  if (!isRecord(value)) return null;
  if (value.kind === "letters") {
    const text = typeof value.text === "string" ? normalizeLetters(value.text) : "";
    if (!text) return null;
    const color = BADGE_COLORS.find((c) => c === value.color) ?? DEFAULT_BADGE_COLOR;
    return { kind: "letters", text, color };
  }
  if (value.kind === "image" && isIconImage(value.src)) return { kind: "image", src: value.src };
  return null;
}

/** Reads saved preferences leniently; invalid parts are dropped one by one. */
export function parseProgramPrefs(value: unknown): ProgramPrefs {
  if (!isRecord(value)) return DEFAULT_PROGRAM_PREFS;
  const icons: [string, IconChoice][] = [];
  if (isRecord(value.icons)) {
    for (const [id, raw] of Object.entries(value.icons)) {
      const choice = validId(id) ? parseChoice(raw) : null;
      if (choice) icons.push([id, choice]);
      if (icons.length >= MAX_IDS) break;
    }
  }
  // fromEntries creates own properties, even for IDs like "__proto__".
  return {
    order: idList(value.order),
    hidden: idList(value.hidden),
    icons: Object.fromEntries(icons),
  };
}

/** Icon preference for an ID; inherited properties like "constructor" do not count. */
export function iconChoice(prefs: ProgramPrefs, id: string): IconChoice | undefined {
  return Object.hasOwn(prefs.icons, id) ? prefs.icons[id] : undefined;
}

export function iconMode(prefs: ProgramPrefs, id: string): IconMode {
  return iconChoice(prefs, id)?.kind ?? "auto";
}

/** Sets or removes (`null` = Automatic) the icon preference of an ID. */
export function withIcon(prefs: ProgramPrefs, id: string, choice: IconChoice | null): ProgramPrefs {
  const rest = Object.entries(prefs.icons).filter(([key]) => key !== id);
  return { ...prefs, icons: Object.fromEntries(choice ? [...rest, [id, choice]] : rest) };
}

/**
 * Sorts by the saved order. Unknown entries (newly installed) follow at the end in the
 * backend's order.
 */
export function sortByOrder<T extends { id: string }>(
  items: readonly T[],
  order: readonly string[],
): T[] {
  const rank = new Map(order.map((id, index) => [id, index]));
  return items
    .map((item, index) => ({ item, index, rank: rank.get(item.id) }))
    .sort((a, b) => {
      if (a.rank !== undefined && b.rank !== undefined) return a.rank - b.rank;
      if (a.rank !== undefined) return -1;
      if (b.rank !== undefined) return 1;
      return a.index - b.index;
    })
    .map(({ item }) => item);
}

/**
 * New saved order after `id` was dragged to `toIndex` in the displayed list `shown`.
 * Remembered IDs that are not shown right now are kept at the end.
 */
export function moveTo(
  shown: readonly string[],
  order: readonly string[],
  id: string,
  toIndex: number,
): string[] {
  if (!shown.includes(id)) return [...order];
  const without = shown.filter((other) => other !== id);
  const target = Math.min(Math.max(Math.round(toIndex), 0), without.length);
  const arranged = [...without.slice(0, target), id, ...without.slice(target)];
  return [...arranged, ...order.filter((other) => !shown.includes(other))];
}

export function isHidden(prefs: ProgramPrefs, id: string): boolean {
  return prefs.hidden.includes(id);
}

export function withHidden(prefs: ProgramPrefs, id: string, hidden: boolean): ProgramPrefs {
  const rest = prefs.hidden.filter((other) => other !== id);
  return { ...prefs, hidden: hidden ? [...rest, id] : rest };
}

/** Installed entries in the preferred order (hidden ones included, for the Settings). */
export function arrangedProfiles(profiles: readonly Profile[], prefs: ProgramPrefs): Profile[] {
  return sortByOrder(
    profiles.filter((profile) => profile.installed),
    prefs.order,
  );
}

/** What the sidebar and menu show under "New session". */
export function startableProfiles(profiles: readonly Profile[], prefs: ProgramPrefs): Profile[] {
  return arrangedProfiles(profiles, prefs).filter((profile) => !isHidden(prefs, profile.id));
}

/** Catalog agents that are not installed, for "Available", alphabetical. */
export function availableProfiles(profiles: readonly Profile[]): Profile[] {
  return profiles
    .filter((profile) => !profile.installed && profile.source === "catalog")
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Icons that "Automatic" can choose from, both optional. */
export interface AutoIcons {
  /** Bundled icon of a catalog agent (agentIcons.ts). */
  agent?: string | undefined;
  /** Icon extracted from the program file (Windows .exe). */
  program?: string | undefined;
}

/**
 * Badge for an ID: the user's own preference first. "Automatic" takes the bundled agent icon,
 * then the program icon, otherwise letters from the profile or ID on a stable color. The bundled
 * icon wins over the program icon because the program icon depends on how the CLI was installed
 * (an .exe has one, an npm or pip script has none), and the same agent should always look alike.
 */
export function resolveBadge(
  id: string,
  profile: Profile | undefined,
  icons: AutoIcons,
  prefs: ProgramPrefs,
): BadgeLook {
  const choice = iconChoice(prefs, id);
  if (choice?.kind === "image") return { kind: "image", src: choice.src };
  if (choice?.kind === "letters")
    return { kind: "letters", text: choice.text, color: choice.color };
  if (icons.agent) return { kind: "image", src: icons.agent };
  if (icons.program) return { kind: "image", src: icons.program };
  return { kind: "letters", text: badgeText(profile, id), color: badgeColor(id) };
}

/** Fit an image into `size` × `size`, keeping the aspect ratio, centered. */
export function fitContain(
  width: number,
  height: number,
  size: number,
): { x: number; y: number; width: number; height: number } {
  if (!(width > 0) || !(height > 0)) return { x: 0, y: 0, width: size, height: size };
  const scale = size / Math.max(width, height);
  const w = width * scale;
  const h = height * scale;
  return { x: (size - w) / 2, y: (size - h) / 2, width: w, height: h };
}

/** Largest file Hangar accepts for a custom icon. */
export const MAX_ICON_FILE_BYTES = 1024 * 1024;
const ICON_TYPES = ["image/png", "image/svg+xml"];

export type IconFileProblem = "type" | "size" | null;

export function iconFileProblem(type: string, size: number): IconFileProblem {
  if (!ICON_TYPES.includes(type)) return "type";
  if (size <= 0 || size > MAX_ICON_FILE_BYTES) return "size";
  return null;
}
