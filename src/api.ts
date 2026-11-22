// The only place that talks to the backend. The types mirror the Rust structs
// (serde, camelCase) in src-tauri/src/session.rs, agent_catalog.rs and manager.rs.

import { Channel, invoke } from "@tauri-apps/api/core";

import { t } from "./i18n";
import type { AccountPage } from "./account";
import { parseImagePaste, type ImagePaste } from "./clipboardPaste";
import type { DictationLanguage, DictationModel } from "./dictation";
import type { GateAnswer } from "./gate";
import type { MainPrefs } from "./mainStore";
import type { PanelBounds, PanelMessage } from "./panelBridge";
import type { PushEvent, PushInput } from "./phonePush";
import type { ProtectedSettings, SecuritySwitches } from "./settings";
import { backendErrorText } from "./backendError";
import { InputStalled, inputQueue } from "./sessionInput";
import { noteInput } from "./treeFlowStore";
import type { FigureInput } from "./subAgentLibrary";
import type { DefProgram, SubAgentSettings } from "./subAgentModels";
import type { WorkspaceTemplate } from "./templates";
import type { SessionState } from "./team";
import type { UsageFeature, UsageSettings } from "./usageStats";

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
  /**
   * Why it cannot be a team session on this machine, `null` when it can (`fixedFolder`: only in
   * its own folder). Older backends send none.
   */
  teamBlock?: TeamBlock | null;
  /**
   * Rust `InstructionsReach`: whether HANGAR.md reaches the program as found on this machine
   * (`launcher`: an npm cmd.exe launcher that cannot carry the text). Older backends send none.
   */
  instructions?: InstructionsReach;
  /**
   * Rust `AgentCommand` list (agent_commands.rs): what the session menu can type into a running
   * session of the program, the catalog's first. Older backends send none.
   */
  agentCommands?: AgentCommand[];
  /**
   * The program can start as a Claude Code sub-agent definition (`agent_args` in the catalog,
   * subAgentLibrary.ts). Older backends send none.
   */
  takesAgent?: boolean;
}

/** Rust `LabelKey`: a command label Hangar translates (`agentCommand.<key>` in i18n.ts). */
export type AgentCommandLabel =
  "compact" | "clear" | "new" | "model" | "usage" | "context" | "status" | "stats";

/** Rust `AgentCommand`: exactly one of `labelKey` and `label` is set. */
export interface AgentCommand {
  labelKey?: AgentCommandLabel;
  label?: string;
  /** Typed into the session as a bracketed paste. */
  text: string;
  /** Enter follows the text. */
  submit: boolean;
  /** Ask before sending (the command throws the conversation away). */
  confirm: boolean;
}

export type InstructionsReach = "none" | "yes" | "launcher";

/**
 * Rust `TeamBlock` (agent_catalog.rs). `noHooks` and `notMeasured` come from the catalog; the
 * others can be fixed on this machine.
 */
export type TeamBlock =
  | { kind: "noHooks" }
  | { kind: "notMeasured" }
  | { kind: "unknownVersion" }
  | { kind: "belowMinVersion"; min: string; installed: string }
  | { kind: "adminSettings" }
  | { kind: "fixedFolder" };

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
  /** The sub-agent definition the session runs as (`--agent`); kept across restarts. */
  subAgent?: string | null;
}

/**
 * Rust `SessionMode`: a private session has no agent gate and stays off the team; a team session
 * runs in its own worktree with the gate and is visible to the team (startMode.ts).
 */
export type SessionMode = "private" | "team";

/**
 * Rust `GateStatus`: "none" (not for this program, gate off, update session), "active", or why a
 * program that could take the gate does not get it ("noEndpoint", "launcher", "profileSettings",
 * "installPath", "hookFiles").
 */
export type GateStatus =
  "none" | "active" | "noEndpoint" | "launcher" | "profileSettings" | "installPath" | "hookFiles";

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

/** Rust `StartPrompt`: a first prompt by name; its text lives in the backend's agent catalog. */
export type StartPrompt = "worktree";

/**
 * Rust `StartRequest`: what a new agent session gets besides its program's arguments, where the
 * program takes it (startOptions.ts).
 */
export interface StartOptions {
  /** Hand the agent HANGAR.md. */
  instructions: boolean;
  startPrompt: StartPrompt | null;
  /** Start as this sub-agent definition; only for profiles with `takesAgent`. */
  subAgent?: string | null;
}

export const NO_START_OPTIONS: StartOptions = { instructions: false, startPrompt: null };

export { Channel };

/** `write_session` answers `false` while the session's input queue is full. */
const writeSession = inputQueue((id, data) => invoke<boolean>("write_session", { id, data }));

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
    options: StartOptions = NO_START_OPTIONS,
  ) =>
    invoke<SessionInfo>("create_session", {
      profileId,
      cwd,
      size,
      onEvent,
      resume,
      cliSessionId,
      mode,
      instructions: options.instructions,
      startPrompt: options.startPrompt,
      subAgent: options.subAgent ?? null,
    }),
  /** Opens HANGAR.md in the default editor; the backend creates it from a template if missing. */
  openHangarMd: () => invoke<null>("open_hangar_md"),
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
  /**
   * Input to a session, in order per session; a large paste goes in pieces below the backend's
   * limit, and a full input queue is waited out (sessionInput.ts).
   */
  write: (id: number, data: string) => {
    // The tree flow's input rate; returns at once while the flow is off.
    noteInput(id, data);
    return writeSession(id, data);
  },
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
   * Tokens and estimated cost per running session and per local day; check with
   * `parseCostReport` (costs.ts). Only sessions whose program keeps readable usage appear.
   */
  costReport: () => invoke<unknown>("cost_report"),
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

  // Sub-agent definitions and figures (contract: docs/wiki/pages/syntheses/sub-agent-library.md).
  /** Global definitions and those of `folders`; check with `parseLibrary` (subAgentLibrary.ts). */
  subAgentLibrary: (folders: readonly string[]) =>
    invoke<unknown>("sub_agent_library", { folders }),
  /** Saves a definition's text as a global one; rejects with a `DefError` (`parseDefError`). */
  createSubAgent: (text: string, replace: boolean) =>
    invoke<unknown>("create_sub_agent", { text, replace }),
  deleteSubAgent: (name: string) => invoke<null>("delete_sub_agent", { name }),
  /** Brings Hangar's copies of the definitions in other programs up to date (main window). */
  syncSubAgents: (folders: readonly string[]) => invoke<null>("sync_sub_agents", { folders }),
  /**
   * Sets one definition's model (Codex CLI: and effort); `null` removes the field. A project's
   * file only with `confirmed`, else a `DefError` of kind `confirm`.
   */
  setSubAgentModel: (change: {
    program: DefProgram;
    name: string;
    folder: string | null;
    model: string | null;
    effort: string | null;
    confirmed: boolean;
  }) => invoke<null>("set_sub_agent_model", change),
  /** Default model per program and switched-off copies; check with `parseSubAgentSettings`. */
  subAgentSettings: () => invoke<unknown>("sub_agent_settings"),
  saveSubAgentSettings: (settings: SubAgentSettings) =>
    invoke<null>("save_sub_agent_settings", { settings }),
  /** Opens the definition's file in the editor; `folder` null: the global one. */
  openSubAgent: (name: string, folder: string | null) =>
    invoke<null>("open_sub_agent", { name, folder }),
  /** Chosen figure per agent name; check with `parseFigures`. */
  subAgentFigures: () => invoke<unknown>("sub_agent_figures"),
  /** Answers with every figure, as `subAgentFigures`. */
  setSubAgentFigure: (name: string, figure: FigureInput) =>
    invoke<unknown>("set_sub_agent_figure", { name, figure }),
  /** The clipboard's image as the figure; answers like `setSubAgentFigure`. */
  pasteSubAgentFigure: (name: string) => invoke<unknown>("paste_sub_agent_figure", { name }),
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
  /** `false` when the previous run did not quit cleanly (crash, kill, power loss). */
  lastExitWasClean: () => invoke<unknown>("last_exit_was_clean"),
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
  /** Panel window only: closes itself; its close handler saves input, then destroys it. */
  closePanelWindow: () => invoke<null>("close_panel_window"),
  /** Panel window only: ends itself once pending input is saved. */
  destroyPanelWindow: () => invoke<null>("destroy_panel_window"),
  /** Panel window only: the one way to ask something of the main window (panelBridge.ts). */
  panelToMain: (message: PanelMessage) => invoke<null>("panel_to_main", { message }),
  /**
   * Main window only: the one session the panel window may address from now on (its git,
   * file tree and viewer commands answer any other id as unknown); `null`: none. Called before
   * the panel gets its context.
   */
  setPanelSession: (id: number | null) => invoke<null>("set_panel_session", { id }),

  // Main window only, kept by the backend rather than in localStorage, which the panel and file
  // viewer windows share (mainStore.ts).
  /** The stored security switches, `null` before the first save; check with `parseSwitches`. */
  securitySwitches: () => invoke<unknown>("security_switches"),
  saveSecuritySwitches: (switches: SecuritySwitches) =>
    invoke<null>("save_security_switches", { switches }),
  /**
   * Paste warning, automatic updates, HANGAR.md and snippets, `null` before the first save;
   * check with `parseProtected`.
   */
  protectedSettings: () => invoke<unknown>("protected_settings"),
  saveProtectedSettings: (settings: ProtectedSettings) =>
    invoke<null>("save_protected_settings", { settings }),
  /**
   * Start mode, last program, folders, session restore and notices, `null` before the first
   * save; check with `parseMainPrefs`.
   */
  mainPrefs: () => invoke<unknown>("main_prefs"),
  saveMainPrefs: (prefs: MainPrefs) => invoke<null>("save_main_prefs", { prefs }),
  /** The workspace templates, `null` before the first save; check with `parseTemplates`. */
  loadTemplates: () => invoke<unknown>("load_templates"),
  saveTemplates: (templates: readonly WorkspaceTemplate[]) =>
    invoke<null>("save_templates", { templates }),
  /** The partner projects (ADR 0012); check with `parsePartnerBook`. */
  partnerProjects: () => invoke<unknown>("partner_projects"),
  /**
   * Makes two existing project folders partners under their short names; answers the new list.
   * Rejects with the reason (name taken, folder missing, already partners).
   */
  linkPartner: (link: {
    thisFolder: string;
    thisName: string;
    partnerFolder: string;
    partnerName: string;
  }) => invoke<unknown>("link_partner", link),
  /** Ends the partnership of the projects named `a` and `b`; answers the new list. */
  unlinkPartner: (a: string, b: string) => invoke<unknown>("unlink_partner", { a, b }),
  /** The partner of a session's project and its sessions; check with `parsePartnerTargets`. */
  partnerTargets: (from: number, partner: string) =>
    invoke<unknown>("partner_targets", { from, partner }),
  /**
   * The checked text to paste into session `to` (header line, control characters removed);
   * the backend logs the delivery. Paste the answer right away.
   */
  deliverPartnerMessage: (from: number, to: number, partner: string, text: string) =>
    invoke<string>("deliver_partner_message", { from, to, partner, text }),
  /** Messages agents sent that wait for the person; check with `parseWaitingMessages`. */
  waitingMessages: () => invoke<unknown>("waiting_messages"),
  /**
   * The checked text of waiting message `id` for session `to` (header line first). The message
   * keeps waiting until `closeWaitingMessage`.
   */
  waitingMessageText: (id: number, to: number) =>
    invoke<string>("waiting_message_text", { id, to }),
  /** Removes waiting message `id`: pasted into `pastedInto`, or discarded (`null`). */
  closeWaitingMessage: (id: number, pastedInto: number | null) =>
    invoke<null>("close_waiting_message", { id, pastedInto }),
  /** Link groups, linkable sessions and their messages (ADR 0014); check with `parseLinkState`. */
  linkState: () => invoke<unknown>("link_state"),
  /**
   * Links the running agent sessions `ids` into a new group; rejects with the reason. `names`
   * (chosen beforehand, a workspace template), `lead` and `askFirst` are optional.
   */
  linkSessions: (
    ids: readonly number[],
    options: {
      names?: readonly { session: number; name: string }[];
      lead?: number | null;
      askFirst?: boolean;
    } = {},
  ) =>
    invoke<unknown>("link_sessions", {
      ids,
      names: options.names ?? null,
      lead: options.lead ?? null,
      askFirst: options.askFirst ?? null,
    }),
  /** Ends link group `group`; what still waits in it goes. */
  unlinkGroup: (group: number) => invoke<null>("unlink_group", { group }),
  /** New member names, the "Ask before pasting" toggle and the lead (`null`: none). */
  editLinkGroup: (
    group: number,
    names: readonly { session: number; name: string }[],
    askFirst: boolean,
    lead: number | null,
  ) => invoke<null>("edit_link_group", { group, names, askFirst, lead }),
  /** The last messages of link group `group`, newest first; check with `parseLinkHistory`. */
  linkHistory: (group: number) => invoke<unknown>("link_history", { group }),
  /**
   * The checked text of link message `id` for session `to` (header line first), claimed for
   * this one paste until `closeLinkMessage` or `holdLinkMessage`. `automatic`: pasted without a
   * click, which the backend refuses once the message waits for one (toggle, brake).
   */
  linkMessageText: (id: number, to: number, automatic: boolean) =>
    invoke<string>("link_message_text", { id, to, automatic }),
  /** The person typed into session `id`; a link group's budget starts again. */
  linkPersonTyped: (id: number) => invoke<null>("link_person_typed", { id }),
  /** Link message `id` waits for a click from now on. */
  holdLinkMessage: (id: number) => invoke<null>("hold_link_message", { id }),
  /** Removes link message `id`: pasted into `pastedInto`, or discarded (`null`). */
  closeLinkMessage: (id: number, pastedInto: number | null) =>
    invoke<null>("close_link_message", { id, pastedInto }),
  /** Queued message texts per session key, `null` before the first save. */
  loadMessageQueue: () => invoke<unknown>("load_message_queue"),
  /** Replaces the stored queued messages; keys are session keys (UUIDs). */
  saveMessageQueue: (entries: Record<string, string>) =>
    invoke<null>("save_message_queue", { entries }),
  /**
   * The results of finished update sessions not taken yet, each once; the payload-free
   * `program-update-result` says one waits. Check each with `parseUpdateOutcome`.
   */
  takeUpdateResults: () => invoke<unknown>("take_update_results"),

  /**
   * Saves a session's scrollback as a file. The backend opens the save dialog itself (the
   * frontend never chooses a path) and writes at most 32 MiB. Check with `parseExportResult`:
   * the saved path, or `null` when the dialog was cancelled.
   */
  exportSession: (format: "text" | "html", content: string, suggestedName: string) =>
    invoke<unknown>("export_session", { format, content, suggestedName }),
  /**
   * Saves the session's conversation as a Markdown report, read by the backend from the agent's
   * own transcript (only for sessions with `subAgents`, see `canExportReport`). The backend
   * renders it, opens the save dialog itself and writes at most 32 MiB. Check with
   * `parseExportResult`.
   */
  exportReport: (
    sessionId: number,
    title: string,
    suggestedName: string,
    utcOffsetMinutes: number,
    labels: ReportLabels,
  ) =>
    invoke<unknown>("export_report", {
      sessionId,
      title,
      suggestedName,
      utcOffsetMinutes,
      labels,
    }),
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
  /** CPU and memory of every running session's process tree (resources.ts parses it). */
  sessionResources: () => invoke<unknown>("session_resources"),
  /** CPU, memory and GPU of the whole machine in percent (systemUsage.ts parses it). */
  systemUsage: () => invoke<unknown>("system_usage"),
  /** Battery or AC, read afresh (power.ts parses it). */
  powerState: () => invoke<unknown>("power_state"),
  /** Linux: the waiting sessions on the launcher icon; 0 hides the count (launcher_badge.rs). */
  setLauncherCount: (count: number) => invoke<unknown>("set_launcher_count", { count }),
  /** Push settings and which secrets are stored, never their values (phonePush.ts parses it). */
  pushSettings: () => invoke<unknown>("push_settings"),
  /** Saves the push settings; secrets go to the keychain only. Rejects with an error kind. */
  pushSave: (input: PushInput) => invoke<unknown>("push_save", { input }),
  /** Sends `text` once with the saved settings. Rejects with an error kind. */
  pushTest: (text: string) => invoke<null>("push_test", { text }),
  /**
   * A push for a session's new state; `false` when none was due (no provider, event not chosen,
   * a push for this session less than 30 s ago, ten pushes in the last minute). `template` is the
   * translated sentence with one `{name}`; the backend fills in program and number.
   */
  pushSession: (id: number, event: PushEvent, template: string) =>
    invoke<boolean>("push_session", { id, event, template }),
  /**
   * Opens a local dev server address (devServer.ts) in the default browser. The backend accepts
   * only http(s) on a loopback host with a port and rejects anything else; until a backend has
   * the command, the call rejects and the caller shows the address to copy instead.
   */
  openLocalUrl: (url: string) => invoke<null>("open_local_url", { url }),
  /**
   * Terminal links (terminalLinks.ts): "file", "dir" or `null` for each of at most 64 local
   * absolute paths; UNC and device paths are always `null`. Check with `parsePathKinds`.
   */
  statPaths: (paths: string[]) => invoke<unknown>("stat_paths", { paths }),
  /**
   * Opens a local absolute path clicked in the terminal: a folder in the file manager, a file in
   * its default program only for harmless types (text, code, images, PDF); anything else is
   * only shown selected. Main window, focused.
   */
  openPath: (path: string) => invoke<unknown>("open_path", { path }),
  /** Opens a plain http(s) address clicked in the terminal in the default browser. */
  openWebUrl: (url: string) => invoke<null>("open_web_url", { url }),
  /**
   * Branch per folder (at most 64, read from `.git`, no process): branch name, the short hash of
   * a detached HEAD, or `null` outside a repository. Check with `parseBranches`.
   */
  gitBranches: (paths: string[]) => invoke<unknown>("git_branches", { paths }),
  /**
   * Repository place per folder (at most 64, read from `.git`, no process): the checkout and the
   * repository's main folder (a linked worktree names its repository), or `null` outside one.
   * Check with `parseRepoPlaces` (projectCards.ts).
   */
  gitRepoPlaces: (paths: string[]) => invoke<unknown>("git_repo_places", { paths }),
  /**
   * Changes of a session's folder against HEAD (at most 2 MiB of diff text); the backend finds
   * the folder. Check with `parseGitDiff`; rejects with a text when git is missing, the folder is
   * refused or the session is unknown.
   */
  gitDiff: (id: number) => invoke<unknown>("git_diff", { id }),
  /**
   * Number of changed files of a session's folder, the same as the file list of `gitDiff`,
   * without reading the diff itself; `null` outside a repository. Check with
   * `parseChangedCount`; rejects with a text like `gitDiff`.
   */
  gitChangedCount: (id: number) => invoke<unknown>("git_changed_count", { id }),
  /**
   * The user's own commits of about the last year in the repositories of these folders (at most
   * 24; folders that are no repository are skipped). Only folders the backend knows itself
   * count (sessions' folders, favorites, recent folders). Check with `parseGitActivity`.
   */
  gitActivity: (folders: string[]) => invoke<unknown>("git_activity", { folders }),
  // Work log and daily review (ADR 0011).
  /** A working phase of the session ended; times in Unix ms. The backend adds the details. */
  logWorkPhase: (id: number, from: number, to: number) =>
    invoke<null>("log_work_phase", { id, from, to }),
  /** One local day `[start, end)` in Unix ms. Check with `parseDailyReview` (dailyReview.ts). */
  dailyReview: (start: number, end: number) => invoke<unknown>("daily_review", { start, end }),
  /** The same for a past day, with what is uncommitted right now (morningReview.ts). */
  morningReview: (start: number, end: number) =>
    invoke<unknown>("daily_review", { start, end, openChanges: true }),
  /**
   * One page of the commits reachable from HEAD of a session's folder, newest first; with
   * `root`, of its repository's main folder. The backend clamps `limit` to 1..=100. Check with
   * `parseGitLog` (gitHistory.ts).
   */
  gitLog: (id: number, skip: number, limit: number, root = false) =>
    invoke<unknown>("git_log", { id, skip, limit, root }),
  /**
   * Header and changes of one commit against its first parent, shaped like `git_diff`. Pass only
   * full ids from `git_log`. Check with `parseCommitDiff` (gitHistory.ts).
   */
  gitCommitDiff: (id: number, sha: string) => invoke<unknown>("git_commit_diff", { id, sha }),
  /**
   * Git actions on the repository of a session's folder; the backend finds the folder. `paths`
   * are relative to the repository root, exactly as `gitDiff` lists them. Each rejects with a
   * text (git's own message where git failed).
   */
  gitStage: (id: number, paths: string[]) => invoke<null>("git_stage", { id, paths }),
  gitUnstage: (id: number, paths: string[]) => invoke<null>("git_unstage", { id, paths }),
  /** Throws the changes away; untracked files are deleted. Ask the user first. */
  gitDiscard: (id: number, paths: string[]) => invoke<null>("git_discard", { id, paths }),
  /**
   * Commits what is staged; hooks run as configured. Nothing is committed when the staged changes
   * seem to hold a secret, unless `allowTree` is the tree that answer named ("Commit anyway" for
   * exactly that index); check the answer with `parseCommitOutcome`.
   */
  gitCommit: (id: number, message: string, allowTree: string | null) =>
    invoke<unknown>("git_commit", { id, message, allowTree }),
  /**
   * Timeline of a session's work tree (git_snapshots.rs). The main window binds the session's
   * workspace key once (`snapshotBind`), which names its snapshots across restarts; every other
   * call names the session by ID only. `snapshotTake` saves the work tree after an agent step
   * unless nothing changed; `prompt` is the agent's last prompt (`snapshotPrompt`).
   */
  snapshotBind: (id: number, key: string) => invoke<null>("snapshot_bind", { id, key }),
  snapshotTake: (id: number, prompt: string | null) =>
    invoke<unknown>("snapshot_take", { id, prompt }),
  /** Newest first, why the newest step was not saved, and the other sessions in the work tree. */
  snapshotList: (id: number) => invoke<unknown>("snapshot_list", { id }),
  /** What one step changed, shaped like `git_diff`. Check with `parseGitDiff`. */
  snapshotDiff: (id: number, number: number) => invoke<unknown>("snapshot_diff", { id, number }),
  /** Back to a snapshot after saving the current state. Ask first; `parseRestoreOutcome`. */
  snapshotRestore: (id: number, number: number) =>
    invoke<unknown>("snapshot_restore", { id, number }),
  /** Deletes the session's snapshots; call it once the session is closed. */
  snapshotForget: (id: number) => invoke<null>("snapshot_forget", { id }),
  /**
   * One folder of a session's file tree; `relative` is a `/` separated path below the session's
   * folder, empty for the folder itself. Links are listed but cannot be opened. Check with
   * `parseFolderListing`; rejects with a text for an unknown session or a refused path.
   */
  listSessionFolder: (id: number, relative: string) =>
    invoke<unknown>("list_session_folder", { id, relative }),
  /**
   * Every path below the session's folder for the tree's filter, bounded (20,000 paths, depth,
   * time) and without ignored files in a repository. Check with `parsePathIndex`.
   */
  listSessionPaths: (id: number) => invoke<unknown>("list_session_paths", { id }),
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
  /** Viewer window only: size and change time of its file; check with `parseFileStamp`. */
  fileViewerStamp: () => invoke<unknown>("file_viewer_stamp"),
  /** Viewer window only: its file against the last commit; check with `parseHeadDiff`. */
  fileViewerDiff: () => invoke<unknown>("file_viewer_diff"),
  /** Viewer window only: opens an http(s) link of the Markdown preview in the browser. */
  fileViewerOpenLink: (url: string) => invoke<null>("file_viewer_open_link", { url }),
  /** New branch `hangar/<slug>` and worktree for a session; check with `parseWorktree`. */
  createWorktree: (cwd: string, label: string) =>
    invoke<unknown>("create_worktree", { cwd, label }),
  /** Never forces; what stays is named in `reason`. Check with `parseRemoval`. */
  removeWorktree: (worktree: Worktree) => invoke<unknown>("remove_worktree", { worktree }),
  /** Agent arena: one worktree per label from the current commit; check with `parseArena`. */
  arenaCreate: (cwd: string, labels: string[]) => invoke<unknown>("arena_create", { cwd, labels }),
  /** A contestant's changes against the base commit; check with `parseDiffStat`. */
  arenaDiffStat: (worktree: Worktree, baseCommit: string) =>
    invoke<unknown>("arena_diff_stat", { worktree, baseCommit }),
  /**
   * Takes the winner over, uncommitted; check with `parseApplyOutcome`. `files` is the list the
   * user confirmed; the backend refuses when the changes differ from it by now.
   */
  arenaApply: (worktree: Worktree, baseCommit: string, files: string[]) =>
    invoke<unknown>("arena_apply", { worktree, baseCommit, files }),
  /** Removes an arena worktree and its branch with all changes (the user confirmed it). */
  arenaDiscard: (worktree: Worktree) => invoke<null>("arena_discard", { worktree }),
  /** The test command in a contestant's worktree; check with `parseTestRun`. */
  arenaRunTests: (worktree: Worktree, command: string) =>
    invoke<unknown>("arena_run_tests", { worktree, command }),
  /**
   * The worktrees Hangar created, with their state; `folders` add their repositories to the
   * search (at most 64). Check with `parseCleanupEntries` (worktreeCleanup.ts).
   */
  listHangarWorktrees: (folders: string[]) => invoke<unknown>("list_hangar_worktrees", { folders }),
  /** Bounded size of one listed worktree folder; check with `parseWorktreeSize`. */
  hangarWorktreeSize: (path: string) => invoke<unknown>("hangar_worktree_size", { path }),
  /**
   * Removes a listed worktree after the backend checked its state again; check with
   * `parseCleanupOutcome`.
   */
  removeHangarWorktree: (path: string, repoRoot: string) =>
    invoke<unknown>("remove_hangar_worktree", { path, repoRoot }),
  /** Drops git's record of a worktree whose folder is gone, after checking again. */
  pruneHangarWorktree: (path: string, repoRoot: string) =>
    invoke<unknown>("prune_hangar_worktree", { path, repoRoot }),
  /**
   * Turns the observation of the sessions' changed files (local claims, claims.ts) on or off.
   * While on, the backend signals `claims-changed` (no payload) on every new snapshot.
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
  /**
   * The sessions whose agent shows a permission or input dialog, as its report-only hooks tell
   * (ADR 0016); check with `parsePermissionWaiting`. Main window only.
   */
  permissionWaiting: () => invoke<unknown>("permission_waiting"),
  /** The person answered the reported dialog of session `id` with a key (ADR 0016). */
  permissionAnswered: (id: number) => invoke<null>("permission_answered", { id }),
  /** `false`: the decision no longer waits (answered elsewhere, timed out, session ended). */
  answerGate: (id: number, answer: GateAnswer) => invoke<boolean>("answer_gate", { id, answer }),
  /** The gate rules of every project; check with `parseGateRules`. */
  gateRules: () => invoke<unknown>("gate_rules"),
  /** `false`: no rule has that id (deleted meanwhile). */
  deleteGateRule: (id: number) => invoke<boolean>("delete_gate_rule", { id }),
  /**
   * Turns the command guard (backend command_guard.rs) on or off; it applies to running team
   * sessions at once, and turning it off lets every held shell command go on.
   */
  configureCommandGuard: (enabled: boolean) => invoke<null>("configure_command_guard", { enabled }),
  /** The guard's rules file, rules and problems; check with `parseGuardInfo`. */
  commandGuardInfo: () => invoke<unknown>("command_guard_info"),

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
  /** The signed-in person's picture as a `data:` URL, `null` without one. */
  accountAvatar: () => invoke<unknown>("account_avatar"),
  /** `null`: the official service. Rejects when the address is not https (or loopback http). */
  accountSetBaseUrl: (baseUrl: string | null) =>
    invoke<unknown>("account_set_base_url", { baseUrl }),
  /** Opens one of the account's own pages; the window never passes a URL. */
  accountOpen: (page: AccountPage) => invoke<null>("account_open", { page }),
  /** Tests the chosen server: account API, relay, live channel (checked in serverCheck.ts). */
  accountCheckServer: () => invoke<unknown>("account_check_server"),

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
  /** Every team's connection, the pending confirmations and the repositories to link. */
  teamSnapshot: () => invoke<unknown>("team_snapshot"),
  /** This device joined the team the person expected (they compared the safety numbers). */
  teamConfirmAdmission: (projectId: string, logIndex: number, safetyNumber: readonly string[]) =>
    invoke<unknown>("team_confirm_admission", { projectId, logIndex, safetyNumber }),
  /** Removes another device of the signed-in person from the team (a lost one). */
  teamRemoveDevice: (projectId: string, device: string) =>
    invoke<unknown>("team_remove_device", { projectId, device }),
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

  /**
   * Dictation state for `model`, which models are installed and whether a microphone is there
   * (`parseDictationStatus`).
   */
  dictationStatus: (model: DictationModel) => invoke<unknown>("dictation_status", { model }),
  /** Opens the microphone; the text waits for session `id` later. Rejects with an error kind. */
  dictationStart: (id: number, model: DictationModel, language: DictationLanguage) =>
    invoke<null>("dictation_start", { id, model, language }),
  /** Closes the microphone and transcribes; harmless when nothing records. */
  dictationStop: () => invoke<null>("dictation_stop"),
  /** The transcripts waiting, each handed out once (`parseDictationTexts`). */
  dictationTakeText: () => invoke<unknown>("dictation_take_text"),
  /** Downloads a speech model after the user agreed. Rejects with an error kind. */
  dictationInstallModel: (model: DictationModel) =>
    invoke<null>("dictation_install_model", { model }),
  dictationRemoveModel: (model: DictationModel) =>
    invoke<null>("dictation_remove_model", { model }),

  /** The answer to the usage statistics question: `null` until asked (`parseUsageConsent`). */
  telemetryStatus: () => invoke<unknown>("telemetry_status"),
  /** The person's answer; off deletes what was counted. Answers like `telemetryStatus`. */
  telemetrySetConsent: (enabled: boolean) => invoke<unknown>("telemetry_set_consent", { enabled }),
  /** The settings a daily report shows; the backend maps them to its allowlists. */
  telemetrySettings: (settings: UsageSettings) => invoke<null>("telemetry_settings", { settings }),
  /** One use of a feature; ignored while the statistics are off. */
  telemetryCount: (feature: UsageFeature) => invoke<null>("telemetry_count", { feature }),
  /** Opens the public list of every field a report carries in the browser. */
  telemetryOpenInfo: () => invoke<null>("telemetry_open_info"),
};

/** Headings of a session report in the window's language (Rust: `Labels` in report/mod.rs). */
export interface ReportLabels {
  program: string;
  folder: string;
  branch: string;
  started: string;
  lastActivity: string;
  duration: string;
  conversation: string;
  subAgent: string;
  /** Holds `{count}`. */
  tokens: string;
  compacted: string;
  earlierLeftOut: string;
  changedFiles: string;
  commits: string;
  none: string;
}

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
/** Signal without payload: the cost report changed; read it with `costReport` (costs.ts). */
export const COST_USAGE_CHANGED = "cost-usage-changed";
/** Event with the new power state, sent only on a change (`parsePowerState` in power.ts). */
export const POWER_CHANGED = "power-changed";
/**
 * Signal without payload: the claims changed; the main window reads `claimsSnapshot` (any window
 * could listen to a payload).
 */
export const CLAIMS_CHANGED = "claims-changed";
/** Signal without a payload: the gate changed; read it with `api.gateSnapshot`. */
export const GATE_CHANGED = "gate-changed";
/**
 * Signal without payload: the sessions whose agent shows a permission or input dialog changed
 * (ADR 0016); read them with `permissionWaiting`.
 */
export const PERMISSION_WAITS_CHANGED = "permission-waits-changed";
/**
 * Signal without payload: a message from a partner project's agent arrived or went (ADR 0012);
 * read them with `waitingMessages`. Other webviews may hear it too, so it carries no text.
 */
export const PARTNER_MESSAGES_CHANGED = "partner-messages-changed";
/**
 * Signal without payload: link groups, linkable sessions or link messages changed (ADR 0014);
 * read them with `linkState`. Other webviews may hear it too, so it carries no text.
 */
export const LINK_GROUPS_CHANGED = "link-groups-changed";
/**
 * Signal without payload: the account state changed; read it with `accountStatus`. Other webviews
 * may hear it too, so it carries nothing.
 */
export const ACCOUNT_CHANGED = "account-changed";
/** Signal without payload: the team snapshot changed; read it with `teamSnapshot`. */
export const TEAM_CHANGED = "team-changed";
/**
 * Dictation signals. Status and text carry nothing, since other webviews may hear them: read the
 * state with `dictationStatus` and the finished text with `dictationTakeText`. The level carries
 * one number from 0 to 1 (`parseLevel`).
 */
export const DICTATION_STATUS = "dictation-status";
export const DICTATION_LEVEL = "dictation-level";
export const DICTATION_TEXT = "dictation-text";

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

/** One path of `list_session_paths` (Rust: `IndexEntry` in file_tree.rs). */
export interface PathEntry {
  /** `/` separated below the session's folder, as the tree names it. */
  path: string;
  kind: FolderEntryKind;
}

/** Answer of `list_session_paths` (Rust: `PathIndex` in file_tree.rs). */
export interface PathIndex {
  entries: PathEntry[];
  /** Cut at a limit: some paths are missing. */
  truncated: boolean;
}

function pathEntry(value: unknown): PathEntry | null {
  if (typeof value !== "object" || value === null) return null;
  const path: unknown = Reflect.get(value, "path");
  const kind: unknown = Reflect.get(value, "kind");
  if (typeof path !== "string" || path === "" || path.startsWith("/") || path.endsWith("/")) {
    return null;
  }
  if (kind !== "dir" && kind !== "file" && kind !== "symlink") return null;
  return { path, kind };
}

/** Answer of `list_session_paths`; `null` if malformed. Malformed entries are left out. */
export function parsePathIndex(value: unknown): PathIndex | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const entries: unknown = Reflect.get(value, "entries");
  if (!Array.isArray(entries)) return null;
  return {
    entries: entries.map(pathEntry).filter((entry): entry is PathEntry => entry !== null),
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
  /** Last change on disk in milliseconds since 1970; `null` where unknown. */
  modified: number | null;
  /** The picture of a PNG, JPEG, GIF, WebP or SVG file whose content matches its name. */
  image: ImageData | null;
  /** An image larger than 8 MiB: not handed over. */
  imageTooLarge: boolean;
}

const IMAGE_MIMES = [
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
  "image/svg+xml",
] as const;
export type ImageMime = (typeof IMAGE_MIMES)[number];

/** An image for `<img src="data:...">`; never inserted as markup, an SVG neither. */
export interface ImageData {
  mime: ImageMime;
  /** Base64. */
  data: string;
}

function isImageMime(value: unknown): value is ImageMime {
  return IMAGE_MIMES.some((mime) => mime === value);
}

function imageData(value: unknown): ImageData | null {
  if (typeof value !== "object" || value === null) return null;
  const mime: unknown = Reflect.get(value, "mime");
  const data: unknown = Reflect.get(value, "data");
  if (!isImageMime(mime) || typeof data !== "string" || !/^[A-Za-z0-9+/]*={0,2}$/.test(data)) {
    return null;
  }
  return { mime, data };
}

/** Answer of `file_viewer_stamp` (Rust: `FileStamp`). */
export interface FileStamp {
  modified: number | null;
  size: number;
}

/** Answer of `file_viewer_stamp`; `null` if malformed. */
export function parseFileStamp(value: unknown): FileStamp | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const modified: unknown = Reflect.get(value, "modified");
  const size: unknown = Reflect.get(value, "size");
  if (typeof size !== "number" || !Number.isSafeInteger(size) || size < 0) return null;
  if (modified !== null && (typeof modified !== "number" || !Number.isSafeInteger(modified))) {
    return null;
  }
  return { modified, size };
}

/** How the file stands against the last commit (Rust: `DiffState`). */
export type DiffState = "notRepo" | "noHead" | "notInHead" | "same" | "changed";

/** Answer of `file_viewer_diff` (Rust: `HeadDiff`). */
export interface HeadDiff {
  state: DiffState;
  /** Unified diff, empty unless `changed`. */
  diff: string;
  /** Cut at 2 MiB on a whole line. */
  truncated: boolean;
}

const DIFF_STATES: readonly DiffState[] = ["notRepo", "noHead", "notInHead", "same", "changed"];

/** Answer of `file_viewer_diff`; `null` if malformed. */
export function parseHeadDiff(value: unknown): HeadDiff | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const state: unknown = Reflect.get(value, "state");
  const diff: unknown = Reflect.get(value, "diff");
  const found = DIFF_STATES.find((known) => known === state);
  if (found === undefined || typeof diff !== "string") return null;
  return { state: found, diff, truncated: Reflect.get(value, "truncated") === true };
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
  const modified: unknown = Reflect.get(value, "modified");
  const rawImage: unknown = Reflect.get(value, "image");
  const image = rawImage === null || rawImage === undefined ? null : imageData(rawImage);
  if (image === null && rawImage !== null && rawImage !== undefined) return null;
  return {
    name,
    path,
    relativePath,
    size,
    text,
    truncated,
    binary,
    modified: typeof modified === "number" && Number.isSafeInteger(modified) ? modified : null,
    image,
    imageTooLarge: Reflect.get(value, "imageTooLarge") === true,
  };
}

/** Result of `export_session`: the path written, `null` when cancelled. */
export function parseExportResult(value: unknown): string | null {
  if (typeof value === "string" && value !== "") return value;
  if (value === null || value === undefined) return null;
  throw new Error(t("error.unknown"));
}

/**
 * Errors from `invoke` arrive from the backend as strings (ManagerError serialized as text); a
 * leading `[code]` is shown in the window's own words (backendError.ts).
 */
export function errorText(error: unknown): string {
  if (error instanceof InputStalled) return t("terminal.inputStalled");
  if (typeof error === "string") return backendErrorText(error);
  if (error instanceof Error) return error.message;
  return t("error.unknown");
}
