// Hangar Drive, the separate sync and backup app (ADR 0024): whether it can take a folder, which
// project folders it holds, and which folder a session's "Back up" hands to it. Pure; the backend
// detects the app, reads its public folder list and checks every folder again.

import type { MessageKey } from "./i18n";

/** A folder's state in Hangar Drive; `other` is a state this version does not know yet. */
export type DriveState = "active" | "paused" | "stopped" | "other";

export interface DriveView {
  /** Hangar Drive is installed and set up, so it can take a folder; nothing of it shows otherwise. */
  ready: boolean;
  /** The home folder as the backend compares it, for `isLocalFolder`; `null` when unknown. */
  home: string | null;
  /** Per asked folder that lies in a folder Hangar Drive syncs, that folder's state. */
  marks: ReadonlyMap<string, DriveState>;
  /** Asked folders the backend's full check refuses (hidden, system, home, missing, …). */
  refused: ReadonlySet<string>;
}

export const NO_DRIVE: DriveView = {
  ready: false,
  home: null,
  marks: new Map(),
  refused: new Set(),
};

/** Folders one status call asks about at most (Rust: `hangar_drive::MAX_PATHS`). */
export const MAX_DRIVE_PATHS = 256;

const STATES: readonly DriveState[] = ["active", "paused", "stopped", "other"];

function isState(value: unknown): value is DriveState {
  return STATES.some((state) => state === value);
}

/** The backend's answer for `paths`, in the same order; anything unexpected means no Hangar Drive. */
export function parseDriveStatus(paths: readonly string[], value: unknown): DriveView {
  if (typeof value !== "object" || value === null) return NO_DRIVE;
  const ready: unknown = Reflect.get(value, "ready");
  const home: unknown = Reflect.get(value, "home");
  const marks: unknown = Reflect.get(value, "marks");
  const refused: unknown = Reflect.get(value, "refused");
  if (ready !== true || !Array.isArray(marks) || !Array.isArray(refused)) return NO_DRIVE;
  const found = new Map<string, DriveState>();
  const refusedPaths = new Set<string>();
  paths.forEach((path, index) => {
    const mark: unknown = marks[index];
    if (isState(mark)) found.set(path, mark);
    // Anything but an explicit `false` counts as refused: fail closed.
    if (refused[index] !== false) refusedPaths.add(path);
  });
  return {
    ready: true,
    home: typeof home === "string" && home !== "" ? home : null,
    marks: found,
    refused: refusedPaths,
  };
}

/** The words of a mark: the state only where the folder does not simply sync. */
export function driveMarkKey(state: DriveState): MessageKey {
  switch (state) {
    case "paused":
      return "drive.paused";
    case "stopped":
      return "drive.stopped";
    default:
      return "drive.synced";
  }
}

function hasControl(text: string): boolean {
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/**
 * A local absolute folder Hangar Drive takes (`C:\…` or `/…`): no network share or device path
 * (`\\server`, `\\?\`), no `..`, no control characters, no part starting with a dot (`.git`), and
 * neither the home folder nor a direct child of it (`home`, when known). The backend checks the
 * same on the canonical path, and on Windows also hidden and system folders.
 */
export function isLocalFolder(path: string, home: string | null = null): boolean {
  if (path.trim() === "" || hasControl(path)) return false;
  if (path.startsWith("\\\\") || path.startsWith("//")) return false;
  if (!/^(?:[A-Za-z]:(?:[\\/]|$)|\/)/.test(path)) return false;
  const windows = !path.startsWith("/");
  const parts = path.split(windows ? /[\\/]/ : "/").slice(1);
  if (parts.some((part) => part === ".." || part.startsWith("."))) return false;
  if (windows && parts.some(awkwardWindowsPart)) return false;
  return home === null || !homeOrItsChild(path, home, windows);
}

/** The parts of an absolute path for comparing: no empty parts, without case on Windows. */
function comparedParts(path: string, windows: boolean): string[] {
  const parts = path.split(windows ? /[\\/]/ : "/").filter((part) => part !== "");
  return windows ? parts.map((part) => part.toLowerCase()) : parts;
}

/**
 * The home folder itself or a folder directly in it (Rust: `home_or_its_child`); deeper folders
 * such as `C:\Users\a\Documents\proj` are fine.
 */
function homeOrItsChild(path: string, home: string, windows: boolean): boolean {
  const own = comparedParts(path, windows);
  const base = comparedParts(home, windows);
  if (base.length === 0 || own.length < base.length || own.length > base.length + 1) return false;
  return base.every((part, index) => own[index] === part);
}

/** Names Windows keeps for devices, also with an extension (`nul.txt`). */
const RESERVED_NAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/**
 * A part of a Windows path Hangar Drive refuses (Rust: `awkward_windows_part`): a reserved device
 * name, also with an extension, or a name that ends in a dot or a space.
 */
function awkwardWindowsPart(part: string): boolean {
  if (part.endsWith(".") || part.endsWith(" ")) return true;
  return RESERVED_NAME.test((part.split(".")[0] ?? part).trimEnd());
}

/** What the session menu needs of a session. */
export interface DriveSession {
  cwd: string;
  kind?: "agent" | "update" | undefined;
  worktree?: { path: string; repoRoot: string } | undefined;
}

/**
 * The folder that stands for a session's project before git is asked: the main folder of a
 * worktree Hangar made, else the working folder. `null` for update sessions (home folder on
 * purpose) and folders Hangar Drive does not take.
 */
export function driveFolder(session: DriveSession, home: string | null = null): string | null {
  if (session.kind === "update") return null;
  const folder = session.worktree?.repoRoot ?? session.cwd;
  return isLocalFolder(folder, home) ? folder : null;
}

/**
 * Hangar Drive can take this folder: it is installed and set up, the folder is local, the
 * backend's full check does not refuse it, and it is not already in a synced folder.
 */
export function canBackUp(view: DriveView, folder: string | null): folder is string {
  return (
    view.ready &&
    folder !== null &&
    isLocalFolder(folder, view.home) &&
    !view.refused.has(folder) &&
    !view.marks.has(folder)
  );
}

/**
 * The folder "Back up" hands over for `session`, known before the menu offers the entry, so the
 * entry never shows for a folder that would hand over nothing (ADR 0024): the repository's main
 * folder from `roots` (asked folder to main folder, `null` outside a repository or when git
 * failed), so a session in a subfolder backs up the whole project; else the session's own folder;
 * the main folder of a worktree Hangar made. `null` for update sessions, folders Hangar Drive does
 * not take and while git has not answered for the folder yet. Whether Hangar Drive takes the main
 * folder (directly in the home folder, say) is `canBackUp`'s question: a refused project is never
 * swapped for one of its subfolders, which would back up less than the person asked for.
 */
export function handOverFolder(
  session: DriveSession,
  roots: ReadonlyMap<string, string | null>,
): string | null {
  const folder = driveFolder(session);
  if (folder === null || session.worktree !== undefined) return folder;
  const root = roots.get(folder);
  if (root === undefined) return null;
  return root ?? folder;
}

/** What a place on screen shows for a folder: the way to back it up, or that Drive holds it. */
export type DriveMark = { kind: "offer" } | { kind: "held"; state: DriveState };

/**
 * The logo a session or project shows for `folder`: held while the folder lies in a folder
 * Hangar Drive syncs, the way to back it up while `canBackUp` holds, and nothing otherwise
 * (Hangar Drive not set up, folder refused, git not answered yet): hidden, never greyed out.
 */
export function driveMark(view: DriveView, folder: string | null): DriveMark | null {
  if (!view.ready || folder === null) return null;
  const state = view.marks.get(folder);
  if (state !== undefined) return { kind: "held", state };
  return canBackUp(view, folder) ? { kind: "offer" } : null;
}

/**
 * Where the sidebar shows the logo: at the end of a project group's header, after the folder in
 * each session row, as a button beside the row, in a column left of the rows, or nowhere (the
 * session menu's entry stays in every case).
 */
export type DrivePlace = "sessionCard" | "besideCard" | "leftGutter" | "projectHeader" | "off";

export const DRIVE_PLACES: readonly DrivePlace[] = [
  "sessionCard",
  "besideCard",
  "leftGutter",
  "projectHeader",
  "off",
];

export const DEFAULT_DRIVE_PLACE: DrivePlace = "sessionCard";

export function parseDrivePlace(value: unknown): DrivePlace {
  return DRIVE_PLACES.find((place) => place === value) ?? DEFAULT_DRIVE_PLACE;
}

/** The logo the place `at` shows: the mark where the setting puts it, nothing anywhere else. */
/**
 * Where a row shows the logo: the place the setting names, except that a row layout without the
 * folder has no place after the folder name, so the logo moves beside the row there.
 */
export function effectiveDrivePlace(place: DrivePlace, rowHasFolder: boolean): DrivePlace {
  return place === "sessionCard" && !rowHasFolder ? "besideCard" : place;
}

export function driveMarkAt(
  place: DrivePlace,
  at: DrivePlace,
  mark: DriveMark | null,
): DriveMark | null {
  return place === at && place !== "off" ? mark : null;
}

/**
 * A project group header's logo while the setting puts it there: the first of the group's
 * sessions that shows one stands for the project, whose folder they share.
 */
export function headerDrive(
  place: DrivePlace,
  sessionIds: readonly number[],
  marks: ReadonlyMap<number, DriveMark>,
): { id: number; mark: DriveMark } | null {
  if (place !== "projectHeader") return null;
  for (const id of sessionIds) {
    const mark = marks.get(id);
    if (mark !== undefined) return { id, mark };
  }
  return null;
}
