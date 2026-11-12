// The only place that talks to the backend. The types mirror the Rust structs
// (serde, camelCase) in src-tauri/src/session.rs, agent_catalog.rs and manager.rs.

import { Channel, invoke } from "@tauri-apps/api/core";

import { t } from "./i18n";
import type { AccountPage } from "./account";
import { parseImagePaste, type ImagePaste } from "./clipboardPaste";
import type { GateAnswer } from "./gate";
import type { PanelBounds, PanelMessage } from "./panelBridge";
import type { SessionState } from "./team";

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
  /** A profile from profiles.toml, which Settings can change and delete. */
  editable?: boolean;
  /** Names of the plain environment variables, never their values (`profileEnv` has those). */
  env?: string[];
  /** Names of the variables whose values live in the OS keychain. */
  secretEnv?: string[];
  /**
   * Can run as a team session (startMode.ts): the catalog says the agent gate can be installed
   * for it. Older backends send none.
   */
  teamCapable?: boolean;
}

/** What Settings saves with `saveProfile` (Rust: `ProfileInput` in profile_edit.rs). */
export interface ProfileInput {
  id: string;
  name: string;
  command: string;
  args: string[];
  cwd: string | null;
  env: Record<string, string>;
  secretEnv: string[];
}

/** A session's own git worktree (Rust: `Worktree` in worktree.rs). */
export interface Worktree {
  path: string;
  branch: string;
  repoRoot: string;
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
  /**
   * Conversation id Hangar gave the agent CLI (lowercase UUID), `null` for programs without
   * one. Stored in the workspace so a restore resumes exactly this conversation.
   */
  cliSessionId?: string | null;
  /** `watchSubAgents` works for this session (the program has sub-agents, the session an id). */
  subAgents?: boolean;
  /** Whether the gate (gate.ts) watches the current process; older backends send none. */
  gate?: GateStatus;
  /** Private or team session (ADR 0008); older backends send none. */
  mode?: SessionMode;
}

/**
 * Rust `SessionMode`: a private session has no agent gate and stays off the team; a team session
 * runs in its own worktree with the gate and is visible to the team (startMode.ts).
 */
export type SessionMode = "private" | "team";

/**
 * Rust `GateStatus`: "none" (not for this program, gate off, update session), "active", or why a
 * program that could take the gate does not get it ("noEndpoint", "launcher", "profileSettings").
 */
export type GateStatus = "none" | "active" | "noEndpoint" | "launcher" | "profileSettings";

export type SessionKind = "agent" | "update";

/**
 * `resumeFallback`: resuming failed within seconds, so the backend starts the same session
 * once more without resuming. Its `output` and `exit` events follow. `cliSessionId` and
 * `subAgents` replace the session's values (older backends send neither).
 */
export type SessionEvent =
  | { type: "output"; data: string; bytes: number }
  | { type: "exit"; code: number | null }
  | {
      type: "resumeFallback";
      code: number | null;
      cliSessionId?: string | null;
      subAgents?: boolean;
    };

/** What a `resumeFallback` changes about the session. */
export interface SessionFallback {
  cliSessionId: string | null;
  subAgents: boolean;
}

/**
 * Scope of the notes: a working folder (absolute) or `null` for the General area. `shared`:
 * the project's shared notes in the repository (ADR 0006) instead of the private ones.
 */
export interface NoteFolder {
  path: string | null;
  shared?: boolean;
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
    cliSessionId: string | null = null,
    mode: SessionMode = "private",
  ) =>
    invoke<SessionInfo>("create_session", {
      profileId,
      cwd,
      size,
      onEvent,
      resume,
      cliSessionId,
      mode,
    }),
  /**
   * Starts the program update as a session of its own. Only the backend picks the command, from
   * its update plan; useful only when `ProgramUpdate.plan` is "run". `expectedPlan` is the
   * `planId` the user saw: the backend refuses if the plan changed since (`isPlanChanged`).
   * There is deliberately no working folder: the backend uses the profile's fixed folder or the
   * home folder, so no foreign project can slip in its own programs for the update.
   */
  createUpdateSession: (
    profileId: string,
    size: TermSize,
    onEvent: Channel<SessionEvent>,
    expectedPlan: string | null,
  ) =>
    invoke<SessionInfo>("create_update_session", {
      profileId,
      size,
      onEvent,
      ...(expectedPlan === null ? {} : { expectedPlan }),
    }),
  write: (id: number, data: string) => invoke<null>("write_session", { id, data }),
  resize: (id: number, size: TermSize) => invoke<null>("resize_session", { id, size }),
  ack: (id: number, bytes: number) => invoke<null>("ack_session", { id, bytes }),
  kill: (id: number) => invoke<null>("kill_session", { id }),
  restart: (id: number, size: TermSize, onEvent: Channel<SessionEvent>) =>
    invoke<SessionInfo>("restart_session", { id, size, onEvent }),
  rename: (id: number, title: string) => invoke<SessionInfo>("rename_session", { id, title }),
  close: (id: number) => invoke<null>("close_session", { id }),
  /**
   * Streams snapshots of the session's sub-agents on `onEvent` (contract:
   * docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). A second call for the same
   * session replaces the first watch; `excerpts: false` sends no assistant text at all. `focus`
   * names the sub-agent whose activity log is streamed as well (`null`: none). Read the
   * messages with `parseSubAgentEvent` and `parseSubAgentLog` (subAgents.ts); their texts are
   * never stored.
   */
  watchSubAgents: (sessionId: number, onEvent: Channel, excerpts: boolean, focus: string | null) =>
    invoke<null>("watch_sub_agents", { sessionId, onEvent, excerpts, focus }),
  /** Switches the running watch's log to `agentId` (`null`: none); `false` if no watch runs. */
  focusSubAgent: (sessionId: number, agentId: string | null) =>
    invoke<boolean>("focus_sub_agent", { sessionId, agentId }),
  /** `true` if a watch was running. */
  unwatchSubAgents: (sessionId: number) => invoke<boolean>("unwatch_sub_agents", { sessionId }),
  /** The sessions' newest context use; check with `parseContextSnapshot` (contextUsage.ts). */
  contextUsage: () => invoke<unknown>("context_usage"),
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
   * in the backend; `force` bypasses its disk cache, `redetect` asks the programs for their
   * installed version again. Check with `parseProgramUpdates`.
   */
  checkProgramUpdates: (force: boolean, redetect: boolean) =>
    invoke<unknown>("check_program_updates", { force, redetect }),

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
  /** Check with `parseSearchHits`. */
  searchNotes: (query: string) => invoke<unknown>("search_notes", { query }),
  /** Check with `parseTrash`. */
  listTrash: (folder: NoteFolder) => invoke<unknown>("list_trash", { folder }),
  /** ID of the restored note (a string; check it). */
  restoreNote: (folder: NoteFolder, id: string) => invoke<unknown>("restore_note", { folder, id }),
  /** The backend asks where to save; the path written, `null` when cancelled (check it). */
  exportNote: (folder: NoteFolder, id: string) => invoke<unknown>("export_note", { folder, id }),
  revealNote: (folder: NoteFolder, id: string) => invoke<null>("reveal_note", { folder, id }),
  /** Private to shared notes or back; the new ID (a string; check it). */
  moveNote: (from: NoteFolder, id: string, to: NoteFolder) =>
    invoke<unknown>("move_note", { from, id, to }),
  // Shared project notes (contract: docs/wiki/pages/syntheses/notes.md, "Shared notes").
  /** Check with `parseSharedStatus`. Resolves conflicted notes first; `label` names copies. */
  sharedNotesStatus: (folder: NoteFolder, label: string, untitled: string) =>
    invoke<unknown>("shared_notes_status", { folder, label, untitled }),
  /** The notes recovery code to show once (a string; check it); the same while a setup waits. */
  sharedNotesPrepare: (folder: NoteFolder) => invoke<unknown>("shared_notes_prepare", { folder }),
  /** The backend asks where to save the code. Check with `parseSaveCode`. */
  sharedNotesSaveCode: (folder: NoteFolder, heading: string) =>
    invoke<unknown>("shared_notes_save_code", { folder, heading }),
  /** Copies the code (marked secret) and clears the clipboard after a minute if it still holds it. */
  sharedNotesCopyCode: (folder: NoteFolder) => invoke<null>("shared_notes_copy_code", { folder }),
  /** The setup dialog closed without turning notes on: the prepared code is dropped. */
  sharedNotesCancel: (folder: NoteFolder) => invoke<null>("shared_notes_cancel", { folder }),
  /** Check with `parseSharedStatus`. */
  sharedNotesEnable: (folder: NoteFolder, label: string, untitled: string) =>
    invoke<unknown>("shared_notes_enable", { folder, label, untitled }),
  /** Check with `parseUnlock`. */
  sharedNotesUnlock: (folder: NoteFolder, code: string, label: string, untitled: string) =>
    invoke<unknown>("shared_notes_unlock", { folder, code, label, untitled }),
  /**
   * Main window only: opens the detached side panel or brings it to the front. `bounds` is where
   * it was last; the backend ignores a place that is no longer on a monitor.
   */
  openPanelWindow: (alwaysOnTop: boolean, bounds: PanelBounds | null) =>
    invoke<null>("open_panel_window", { alwaysOnTop, bounds }),
  /** Panel window only: keeps itself above other windows or not. */
  setPanelOnTop: (onTop: boolean) => invoke<null>("set_panel_on_top", { onTop }),
  /** Panel window only: the one way to ask something of the main window (panelBridge.ts). */
  panelToMain: (message: PanelMessage) => invoke<null>("panel_to_main", { message }),

  /**
   * Saves a session's scrollback as a file. The backend opens the save dialog itself (the
   * frontend never chooses a path) and writes at most 32 MiB. Check with `parseExportResult`:
   * the saved path, or `null` when the dialog was cancelled.
   */
  exportSession: (format: "text" | "html", content: string, suggestedName: string) =>
    invoke<unknown>("export_session", { format, content, suggestedName }),
  /**
   * Saves the clipboard's image as a PNG in the app's cache folder (removed after a week): its
   * path, `noImage`, or `busy` while another paste runs. Rejects with a message when reading or
   * saving fails.
   */
  pasteClipboardImage: async (): Promise<ImagePaste> =>
    parseImagePaste(await invoke<unknown>("paste_clipboard_image")),

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
  /**
   * Changes of a working folder against HEAD (at most 2 MiB of diff text). Check with
   * `parseGitDiff`; rejects with a text when git is missing or the folder is refused.
   */
  gitDiff: (cwd: string) => invoke<unknown>("git_diff", { cwd }),
  /**
   * The user's own commits of about the last year in the repositories of these folders (at most
   * 24; folders that are no repository are skipped). Check with `parseGitActivity`.
   */
  gitActivity: (folders: string[]) => invoke<unknown>("git_activity", { folders }),
  /**
   * One page of the commits reachable from HEAD, newest first; the backend clamps `limit` to
   * 1..=100. Check with `parseGitLog` (gitHistory.ts).
   */
  gitLog: (cwd: string, skip: number, limit: number) =>
    invoke<unknown>("git_log", { cwd, skip, limit }),
  /**
   * Header and changes of one commit against its first parent, shaped like `git_diff`. Pass only
   * full ids from `git_log`. Check with `parseCommitDiff` (gitHistory.ts).
   */
  gitCommitDiff: (cwd: string, sha: string) => invoke<unknown>("git_commit_diff", { cwd, sha }),
  /**
   * One folder of a session's file tree; `relative` is a `/` separated path below the session's
   * folder, empty for the folder itself. Links are listed but cannot be opened. Check with
   * `parseFolderListing`; rejects with a text for an unknown session or a refused path.
   */
  listSessionFolder: (id: number, relative: string) =>
    invoke<unknown>("list_session_folder", { id, relative }),
  /** Opens the session's folder in Explorer, Finder or the file manager; the backend finds it. */
  revealSessionFolder: (id: number) => invoke<null>("reveal_session_folder", { id }),
  /**
   * Main window and side panel only: opens a read-only viewer window for one file of a session's
   * folder, or brings the one already showing it to the front. `path` is relative to the
   * session's folder as `listSessionFolder` takes it. Rejects with a text for a refused path or an unknown session.
   */
  openFileViewer: (sessionId: number, path: string) =>
    invoke<null>("open_file_viewer", { sessionId, path }),
  /**
   * Viewer window only: its file, read again on every call. Check with `parseFileContent`;
   * rejects with a text when the file is gone or cannot be read.
   */
  fileViewerContent: () => invoke<unknown>("file_viewer_content"),
  /** New branch `hangar/<slug>` and worktree for a session; check with `parseWorktree`. */
  createWorktree: (cwd: string, label: string) =>
    invoke<unknown>("create_worktree", { cwd, label }),
  /** Never forces; what stays is named in `reason`. Check with `parseRemoval`. */
  removeWorktree: (worktree: Worktree) => invoke<unknown>("remove_worktree", { worktree }),
  /**
   * Turns the observation of the sessions' changed files (local claims, claims.ts) on or off.
   * While on, the backend emits `claims-changed` with every new snapshot.
   */
  configureClaims: (enabled: boolean) => invoke<null>("configure_claims", { enabled }),
  /** The newest claims snapshot; check with `parseClaimsSnapshot`. */
  claimsSnapshot: () => invoke<unknown>("claims_snapshot"),
  /**
   * Turns the gate (gate.ts) on or off. Only sessions started while it is on get the hook;
   * turning it off answers every waiting decision with "allow" at once.
   */
  configureGate: (enabled: boolean) => invoke<null>("configure_gate", { enabled }),
  /** The newest gate snapshot; check with `parseGateSnapshot`. */
  gateSnapshot: () => invoke<unknown>("gate_snapshot"),
  /** `false`: the decision no longer waits (answered elsewhere, timed out, session ended). */
  answerGate: (id: number, answer: GateAnswer) => invoke<boolean>("answer_gate", { id, answer }),

  // Profiles in Settings (profile_edit.rs) and secret variables (secrets.rs).
  /** Inserts or replaces a profile in profiles.toml; returns the new list. */
  saveProfile: (profile: ProfileInput) => invoke<Profile[]>("save_profile", { profile }),
  /** Only the user's own profiles; returns the new list. */
  deleteProfile: (id: string) => invoke<Profile[]>("delete_profile", { id }),
  /** Values of a profile's plain variables, only for the edit form. Check with `parseProfileEnv`. */
  profileEnv: (id: string) => invoke<unknown>("profile_env", { id }),
  /** Per secret variable whether a value is stored, never the value. `parseSecretStatuses`. */
  profileSecrets: (profileId: string) => invoke<unknown>("profile_secrets", { profileId }),
  /** The value goes to the keychain only; the name must be listed in the saved profile. */
  setProfileSecret: (profileId: string, name: string, value: string) =>
    invoke<null>("set_profile_secret", { profileId, name, value }),
  deleteProfileSecret: (profileId: string, name: string) =>
    invoke<null>("delete_profile_secret", { profileId, name }),

  // Hangar account (account.rs, account_service.rs). Answers and errors are checked in
  // account.ts (`parseAccountView`, `parseProjects`, `parseAccountError`).
  /** The sign-in state; the first call reads a stored sign-in from the keychain. */
  accountStatus: () => invoke<unknown>("account_status"),
  /** Starts the device flow: a code to confirm, the confirmation page opens in the browser. */
  accountSignIn: () => invoke<unknown>("account_sign_in"),
  accountCancelSignIn: () => invoke<unknown>("account_cancel_sign_in"),
  /** Drops the sign-in on this device only. */
  accountSignOut: () => invoke<unknown>("account_sign_out"),
  /** The account's projects, fresh from the server. */
  accountProjects: () => invoke<unknown>("account_projects"),
  /** `null`: the official service. Rejects when the address is not https (or loopback http). */
  accountSetBaseUrl: (baseUrl: string | null) =>
    invoke<unknown>("account_set_base_url", { baseUrl }),
  /** Opens one of the account's own pages; the window never passes a URL. */
  accountOpen: (page: AccountPage) => invoke<null>("account_open", { page }),

  // Team connection per project. Answers and errors are checked in team.ts
  // (`parseTeamStatus`, `parsePendingConfirmations`, `parseTeamError`).
  teamStatus: (projectId: string) => invoke<unknown>("team_status", { projectId }),
  /** Turns "Share with team" on; the backend keeps the choice. */
  teamEnable: (projectId: string) => invoke<unknown>("team_enable", { projectId }),
  teamDisable: (projectId: string) => invoke<unknown>("team_disable", { projectId }),
  /** New devices waiting for this device's confirmation. */
  teamPendingConfirmations: () => invoke<unknown>("team_pending_confirmations"),
  teamConfirm: (request: string) => invoke<unknown>("team_confirm", { request }),
  teamReject: (request: string) => invoke<unknown>("team_reject", { request }),
  /** What each session does (team.ts `sessionStates`), for the team's presence. */
  teamSessionStates: (states: { sessionId: number; state: SessionState }[]) =>
    invoke<undefined>("team_session_states", { states }),
  /**
   * Links a repository folder (root commit and clone folder) to a shared project, or unlinks it.
   * `folder: null` removes a link from before folders were bound (`on` must be false).
   */
  teamLinkRepo: (projectId: string, root: string, folder: string | null, on: boolean) =>
    invoke<unknown>("team_link_repo", { projectId, root, folder, on }),
  /** Reserves a task in a shared project; the relay decides who holds it (`team-changed`). */
  teamReserveTask: (
    projectId: string,
    title: string,
    sessionId: number | null,
    area: string | null,
  ) => invoke<unknown>("team_reserve_task", { projectId, title, sessionId, area }),
  /** Releases a task this device holds, or gives up a waiting attempt (`task`: its hash). */
  teamReleaseTask: (projectId: string, task: string) =>
    invoke<unknown>("team_release_task", { projectId, task }),
  /** Asks the holder of a task to hand it over. */
  teamAskRelease: (projectId: string, task: string) =>
    invoke<unknown>("team_ask_release", { projectId, task }),
  /** The holder's answer to the release request of `requester` (the member the card showed). */
  teamAnswerRelease: (projectId: string, task: string, requester: string, accept: boolean) =>
    invoke<unknown>("team_answer_release", { projectId, task, requester, accept }),

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
/** Event with the sessions' context use (`parseContextSnapshot` in contextUsage.ts). */
export const CONTEXT_USAGE_CHANGED = "context-usage-changed";
/** Event with the newest claims snapshot (`parseClaimsSnapshot` in claims.ts). */
export const CLAIMS_CHANGED = "claims-changed";
/** Event with the newest gate snapshot (`parseGateSnapshot` in gate.ts). */
export const GATE_CHANGED = "gate-changed";
/** Event with the newest account state (`parseAccountView` in account.ts). */
export const ACCOUNT_CHANGED = "account-changed";
/** Event with every project's team connection and the pending confirmations (`parseTeamSnapshot`). */
export const TEAM_CHANGED = "team-changed";

/** Session ID from the `notification-activated` payload `{ sessionId }`; `null` if malformed. */
export function parseActivatedSession(payload: unknown): number | null {
  if (typeof payload !== "object" || payload === null) return null;
  const id: unknown = Reflect.get(payload, "sessionId");
  return typeof id === "number" && Number.isInteger(id) ? id : null;
}

/** Kind of a file tree entry; a `symlink` (or junction) is never opened (Rust: `EntryKind`). */
export type FolderEntryKind = "dir" | "file" | "symlink";

/** Rust: `FolderEntry` in file_tree.rs. */
export interface FolderEntry {
  name: string;
  kind: FolderEntryKind;
  /** git ignores the entry. */
  ignored: boolean;
}

/** Answer of `list_session_folder` (Rust: `FolderListing` in file_tree.rs). */
export interface FolderListing {
  /** The session's folder, resolved. */
  root: string;
  /** Top level of the repository containing `root`, in the same form; `null` outside one. */
  repoRoot: string | null;
  /** Folders first, then files and links, each in natural order. */
  entries: FolderEntry[];
  /** The folder holds more entries than were listed. */
  truncated: boolean;
}

function folderEntry(value: unknown): FolderEntry | null {
  if (typeof value !== "object" || value === null) return null;
  const name: unknown = Reflect.get(value, "name");
  const kind: unknown = Reflect.get(value, "kind");
  if (typeof name !== "string" || name === "" || name.includes("/")) return null;
  if (kind !== "dir" && kind !== "file" && kind !== "symlink") return null;
  return { name, kind, ignored: Reflect.get(value, "ignored") === true };
}

/** Answer of `list_session_folder`; `null` if malformed. Malformed entries are left out. */
export function parseFolderListing(value: unknown): FolderListing | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const root: unknown = Reflect.get(value, "root");
  const repoRoot: unknown = Reflect.get(value, "repoRoot");
  const entries: unknown = Reflect.get(value, "entries");
  if (typeof root !== "string" || root === "" || !Array.isArray(entries)) return null;
  return {
    root,
    repoRoot: typeof repoRoot === "string" && repoRoot !== "" ? repoRoot : null,
    entries: entries.map(folderEntry).filter((entry): entry is FolderEntry => entry !== null),
    truncated: Reflect.get(value, "truncated") === true,
  };
}

/** Answer of `file_viewer_content` (Rust: `FileContent` in file_viewer.rs). */
export interface FileContent {
  /** The file name. */
  name: string;
  /** Absolute, for display only. */
  path: string;
  /** Relative to the session's folder, `/` separated, as passed to `openFileViewer`. */
  relativePath: string;
  /** Bytes on disk, also when only a part was read. */
  size: number;
  /** `null` for a binary file. */
  text: string | null;
  /** Larger than 2 MiB: `text` holds the first 2 MiB. */
  truncated: boolean;
  /** A NUL byte within the first 8 KiB. */
  binary: boolean;
}

/** Answer of `file_viewer_content`; `null` if malformed. */
export function parseFileContent(value: unknown): FileContent | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const name: unknown = Reflect.get(value, "name");
  const path: unknown = Reflect.get(value, "path");
  const relativePath: unknown = Reflect.get(value, "relativePath");
  const size: unknown = Reflect.get(value, "size");
  const text: unknown = Reflect.get(value, "text");
  const truncated: unknown = Reflect.get(value, "truncated");
  const binary: unknown = Reflect.get(value, "binary");
  if (typeof name !== "string" || name === "") return null;
  if (typeof path !== "string" || typeof relativePath !== "string") return null;
  if (typeof size !== "number" || !Number.isSafeInteger(size) || size < 0) return null;
  if (typeof truncated !== "boolean" || typeof binary !== "boolean") return null;
  if (text !== null && typeof text !== "string") return null;
  // A binary file never carries text; text never comes without being decoded.
  if (binary !== (text === null)) return null;
  return { name, path, relativePath, size, text, truncated, binary };
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
