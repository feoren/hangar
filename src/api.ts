// The only place that talks to the backend. The types mirror the Rust structs
// (serde, camelCase) in src-tauri/src/session.rs, agent_catalog.rs and manager.rs.

import { Channel, invoke } from "@tauri-apps/api/core";

import { t } from "./i18n";

export type SessionStatus = "starting" | "running" | "exited";

/** "user": from profiles.toml; "catalog": from the built-in agent catalog. */
export type ProfileSource = "user" | "catalog";

/**
 * Entry of the "New session" list (Rust: `ProfileView` in src-tauri/src/agent_catalog.rs).
 * The profile's environment (`env`) is deliberately left out because it may contain secrets.
 */
export interface Profile {
  id: string;
  name: string;
  /** Command used to start it: the one found in PATH, otherwise the first known name. */
  command: string;
  args: string[];
  cwd: string | null;
  icon: string | null;
  source: ProfileSource;
  /** Result of the PATH search at fetch time; call `listProfiles` again to refresh it. */
  installed: boolean;
  homepage: string | null;
  /** Install command for this platform, for display only. */
  installHint: string | null;
  /** Arguments to resume the last conversation; `null`: resume is not possible. */
  resumeArgs?: string[] | null;
}

export interface SessionInfo {
  id: number;
  profileId: string;
  title: string;
  cwd: string;
  status: SessionStatus;
  exitCode: number | null;
  createdAt: number;
  /** Started with `resumeArgs` (conversation resumed); `false` after a fallback. */
  resumed?: boolean;
  /**
   * "update": runs the program's update command (`createUpdateSession`); a restart
   * repeats the update. Such sessions never go into the saved workspace.
   */
  kind?: SessionKind;
}

export type SessionKind = "agent" | "update";

/**
 * `resumeFallback`: resuming failed within seconds, so the backend starts the same session
 * once more without resuming. Its `output` and `exit` events follow.
 */
export type SessionEvent =
  | { type: "output"; data: string; bytes: number }
  | { type: "exit"; code: number | null }
  | { type: "resumeFallback"; code: number | null };

/** Scope of the notes: a working folder (absolute) or `null` for the General area. */
export interface NoteFolder {
  path: string | null;
}

export interface TermSize {
  cols: number;
  rows: number;
}

export { Channel };

export const api = {
  listProfiles: () => invoke<Profile[]>("list_profiles"),
  defaultCwd: () => invoke<string>("default_cwd"),
  listSessions: () => invoke<SessionInfo[]>("list_sessions"),
  createSession: (
    profileId: string,
    cwd: string | null,
    size: TermSize,
    onEvent: Channel<SessionEvent>,
    resume = false,
  ) => invoke<SessionInfo>("create_session", { profileId, cwd, size, onEvent, resume }),
  /**
   * Starts the program update as a session of its own. Only the backend picks the command, from
   * the profile's `update_hint`; useful only when `ProgramUpdate.canRun` holds. There is
   * deliberately no working folder: the backend uses the profile's fixed folder or the home folder,
   * so no foreign project can slip in its own programs for the update.
   */
  createUpdateSession: (profileId: string, size: TermSize, onEvent: Channel<SessionEvent>) =>
    invoke<SessionInfo>("create_update_session", { profileId, size, onEvent }),
  write: (id: number, data: string) => invoke<null>("write_session", { id, data }),
  resize: (id: number, size: TermSize) => invoke<null>("resize_session", { id, size }),
  ack: (id: number, bytes: number) => invoke<null>("ack_session", { id, bytes }),
  kill: (id: number) => invoke<null>("kill_session", { id }),
  restart: (id: number, size: TermSize, onEvent: Channel<SessionEvent>) =>
    invoke<SessionInfo>("restart_session", { id, size, onEvent }),
  rename: (id: number, title: string) => invoke<SessionInfo>("rename_session", { id, title }),
  close: (id: number) => invoke<null>("close_session", { id }),
  /**
   * Version per profile ID (`Record<string, string | null>`), installed programs only.
   * Slow (up to 5 s) because the backend starts the programs to ask. Check with `parseVersions`.
   */
  detectVersions: () => invoke<unknown>("detect_versions"),
  /** Icon per profile ID as `data:image/png;base64,...` (32 × 32) or null. Check with `parseIcons`. */
  programIcons: () => invoke<unknown>("program_icons"),
  /** Actual path of this machine's profiles.toml. */
  profilePath: () => invoke<string>("profile_path"),
  /**
   * Rereads profiles.toml and returns the new list like `listProfiles`; running sessions stay
   * untouched. Rejects with `{ message, line, column }` (check with `parseProfileProblem`).
   */
  reloadProfiles: () => invoke<Profile[]>("reload_profiles"),
  /** Opens profiles.toml in the default editor, creating it from a template if needed. No path. */
  openProfileFile: () => invoke<null>("open_profile_file"),
  /**
   * State per profile ID (installed, latest version, update command). Asks npm, PyPI or GitHub
   * in the backend; `force` bypasses its disk cache. Check with `parseProgramUpdates`.
   */
  checkProgramUpdates: (force: boolean) => invoke<unknown>("check_program_updates", { force }),

  // Workspace and scrollback (contract: docs/wiki/pages/syntheses/session-restore.md).
  /** Expects an object built with `buildWorkspace`; the backend validates it again. */
  saveWorkspace: (state: unknown) => invoke<null>("save_workspace", { state }),
  /** Check with `parseWorkspace`. */
  loadWorkspace: () => invoke<unknown>("load_workspace"),
  saveScrollback: (key: string, data: string) => invoke<null>("save_scrollback", { key, data }),
  loadScrollback: (key: string) => invoke<unknown>("load_scrollback", { key }),
  deleteScrollback: (key: string) => invoke<null>("delete_scrollback", { key }),
  clearScrollbacks: () => invoke<null>("clear_scrollbacks"),

  // Notes per working folder (contract: docs/wiki/pages/syntheses/notes.md).
  /** Check with `parseNoteFolders`. */
  listNoteFolders: () => invoke<unknown>("list_note_folders"),
  /** Check with `parseNotes`. */
  listNotes: (folder: NoteFolder) => invoke<unknown>("list_notes", { folder }),
  readNote: (folder: NoteFolder, id: string) => invoke<unknown>("read_note", { folder, id }),
  writeNote: (folder: NoteFolder, id: string, content: string) =>
    invoke<null>("write_note", { folder, id, content }),
  createNote: (folder: NoteFolder, title: string) =>
    invoke<unknown>("create_note", { folder, title }),
  renameNote: (folder: NoteFolder, id: string, title: string) =>
    invoke<null>("rename_note", { folder, id, title }),
  deleteNote: (folder: NoteFolder, id: string) => invoke<null>("delete_note", { folder, id }),
  reorderNotes: (folder: NoteFolder, ids: string[]) =>
    invoke<null>("reorder_notes", { folder, ids }),
  revealNotesFolder: (folder: NoteFolder) => invoke<null>("reveal_notes_folder", { folder }),

  /**
   * Saves a session's scrollback as a file. The backend opens the save dialog itself (the
   * frontend never chooses a path) and writes at most 32 MiB. Check with `parseExportResult`:
   * the saved path, or `null` when the dialog was cancelled.
   */
  exportSession: (format: "text" | "html", content: string, suggestedName: string) =>
    invoke<unknown>("export_session", { format, content, suggestedName }),

  /**
   * Desktop notification for a session; `false` when the platform has no support. A click on it
   * brings the window to the front and emits `notification-activated` (`parseActivatedSession`).
   */
  notifySession: (id: number, title: string, body: string) =>
    invoke<unknown>("notify_session", { id, title, body }),
  /**
   * Branch per folder (at most 64, read from `.git`, no process): branch name, the short hash of
   * a detached HEAD, or `null` outside a repository. Check with `parseBranches`.
   */
  gitBranches: (paths: string[]) => invoke<unknown>("git_branches", { paths }),
  /** Adds, updates (texts) or removes the notification area icon. */
  configureTray: (enabled: boolean, labels: TrayLabels) =>
    invoke<null>("configure_tray", { enabled, labels }),
};

/** Menu texts and tooltip of the notification area icon (Rust: `TrayLabels` in tray.rs). */
export interface TrayLabels {
  open: string;
  quit: string;
  tooltip: string;
}

/** Event sent when the user clicks a session's desktop notification. */
export const NOTIFICATION_ACTIVATED = "notification-activated";
/** Event sent by the notification area menu item Quit. */
export const TRAY_QUIT = "tray-quit";

/** Session ID from the `notification-activated` payload `{ sessionId }`; `null` if malformed. */
export function parseActivatedSession(payload: unknown): number | null {
  if (typeof payload !== "object" || payload === null) return null;
  const id: unknown = Reflect.get(payload, "sessionId");
  return typeof id === "number" && Number.isInteger(id) ? id : null;
}

/** Result of `export_session`: the path written, `null` when cancelled. */
export function parseExportResult(value: unknown): string | null {
  if (typeof value === "string" && value !== "") return value;
  if (value === null || value === undefined) return null;
  throw new Error(t("error.unknown"));
}

/** Errors from `invoke` arrive from the backend as strings (ManagerError serialized as text). */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return t("error.unknown");
}
