// Hangar Drive, the separate sync and backup app (ADR 0024): whether it is installed, which
// project folders it holds, and which folder a session's "Back up" hands to it. Pure; the backend
// detects the app, reads its public folder list and checks every folder again.

import type { MessageKey } from "./i18n";

/** A folder's state in Hangar Drive; `other` is a state this version does not know yet. */
export type DriveState = "active" | "paused" | "stopped" | "other";

export interface DriveView {
  /** Hangar Drive is installed; nothing of it shows otherwise. */
  installed: boolean;
  /** Per asked folder that lies in a folder Hangar Drive syncs, that folder's state. */
  marks: ReadonlyMap<string, DriveState>;
}

export const NO_DRIVE: DriveView = { installed: false, marks: new Map() };

/** Folders one status call asks about at most (Rust: `hangar_drive::MAX_PATHS`). */
export const MAX_DRIVE_PATHS = 256;

const STATES: readonly DriveState[] = ["active", "paused", "stopped", "other"];

function isState(value: unknown): value is DriveState {
  return STATES.some((state) => state === value);
}

/** The backend's answer for `paths`, in the same order; anything unexpected means no Hangar Drive. */
export function parseDriveStatus(paths: readonly string[], value: unknown): DriveView {
  if (typeof value !== "object" || value === null) return NO_DRIVE;
  const installed: unknown = Reflect.get(value, "installed");
  const marks: unknown = Reflect.get(value, "marks");
  if (installed !== true || !Array.isArray(marks)) return NO_DRIVE;
  const found = new Map<string, DriveState>();
  paths.forEach((path, index) => {
    const mark: unknown = marks[index];
    if (isState(mark)) found.set(path, mark);
  });
  return { installed: true, marks: found };
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
 * (`\\server`, `\\?\`), no `..`, no control characters. The backend checks the same and more.
 */
export function isLocalFolder(path: string): boolean {
  if (path.trim() === "" || hasControl(path)) return false;
  if (path.startsWith("\\\\") || path.startsWith("//")) return false;
  if (!/^(?:[A-Za-z]:(?:[\\/]|$)|\/)/.test(path)) return false;
  const parts = path.split(/[\\/]/);
  if (parts.includes("..")) return false;
  return path.startsWith("/") || !parts.slice(1).some(awkwardWindowsPart);
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
export function driveFolder(session: DriveSession): string | null {
  if (session.kind === "update") return null;
  const folder = session.worktree?.repoRoot ?? session.cwd;
  return isLocalFolder(folder) ? folder : null;
}

/** Hangar Drive can take this folder: installed, local, and not already in a synced folder. */
export function canBackUp(view: DriveView, folder: string | null): folder is string {
  return view.installed && folder !== null && !view.marks.has(folder);
}

/**
 * The folder "Back up" hands over: the repository's main folder when git knows one (a session in
 * a subfolder backs up the whole project), else the session's own folder.
 */
export async function backUpFolder(
  session: DriveSession,
  repoRoot: (folder: string) => Promise<string | null>,
): Promise<string | null> {
  const folder = driveFolder(session);
  if (folder === null || session.worktree !== undefined) return folder;
  const root = await repoRoot(folder).catch(() => null);
  return root !== null && isLocalFolder(root) ? root : folder;
}
