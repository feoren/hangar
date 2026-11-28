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
 * The folder "Back up" hands over: the repository's main folder when git knows one (a session in
 * a subfolder backs up the whole project), else the session's own folder.
 */
export async function backUpFolder(
  session: DriveSession,
  repoRoot: (folder: string) => Promise<string | null>,
  home: string | null = null,
): Promise<string | null> {
  const folder = driveFolder(session, home);
  if (folder === null || session.worktree !== undefined) return folder;
  const root = await repoRoot(folder).catch(() => null);
  if (root === null) return folder;
  // A project Hangar Drive refuses (directly in the home folder, say) is not swapped for one of
  // its subfolders: that would back up less than the person asked for.
  return isLocalFolder(root, home) ? root : null;
}
