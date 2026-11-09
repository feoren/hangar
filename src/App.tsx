import { getVersion } from "@tauri-apps/api/app";
import { emitTo, listen } from "@tauri-apps/api/event";
import { Image } from "@tauri-apps/api/image";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { getCurrentWindow, UserAttentionType, Window } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { exit } from "@tauri-apps/plugin-process";
import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import {
  api,
  errorText,
  NOTIFICATION_ACTIVATED,
  parseActivatedSession,
  parseExportResult,
  TRAY_QUIT,
  type NoteFolder,
  type Profile,
  type SessionInfo,
  type Worktree,
} from "./api";
import type { MenuGroup, MenuItem } from "./AppMenu";
import {
  systemClock,
  type AttentionConfig,
  type AttentionReason,
  type AttentionState,
} from "./attention";
import { attentionEdges, attentionTint, textOnColor } from "./attentionLook";
import { chimeFor, ChimeScheduler } from "./chime";
import { claimsOf, markerText, overlapNotice, overlapsOf, type Overlap } from "./claims";
import { playChime } from "./chimeAudio";
import { drawBadgePng } from "./badgeImage";
import { NO_ACTIVITY } from "./statusLine";
import { notificationText, shouldNotify } from "./desktopNotify";
import { branchPaths } from "./gitBranches";
import { decideWhatsNew, loadLastSeen, saveLastSeen } from "./changelog";
import { pasteFromApp, type PasteWarning } from "./appPaste";
import { Badge } from "./Badge";
import { broadcastCandidates, defaultTargets, sendToTargets } from "./broadcast";
import { BroadcastDialog } from "./BroadcastDialog";
import { writeClipboard } from "./clipboard";
import { Dialog } from "./Dialog";
import { FolderMenu } from "./FolderMenu";
import {
  folderName,
  loadFolderPrefs,
  recordRecent,
  saveFolderPrefs,
  toggleFavorite,
  type FolderPrefs,
} from "./folders";
import {
  createTranslator,
  detectLanguage,
  I18nContext,
  setCurrentLanguage,
  type Translate,
} from "./i18n";
import { formatShortTime, statusText } from "./format";
import { CloseIcon, FolderIcon, NotesIcon } from "./icons";
import { LauncherDialog } from "./LauncherDialog";
import { buildLauncherItems } from "./launcherItems";
import type { Marker } from "./marker";
import { MarkerDialog } from "./MarkerDialog";
import { sessionMenuEntries, type SessionAction } from "./sessionMenu";
import { usableSnippets, snippetLabel, type Snippet } from "./snippets";
import { dropLine } from "./sessionDrag";
import { SnapBar } from "./SnapBar";
import { LAYOUT_IDS, LAYOUT_NAMES, type SnapTarget } from "./snapLayouts";
import { loadLastProgram, pickProgram, quickPrograms, saveLastProgram } from "./launch";
import {
  clampFontSize,
  clampNotesWidth,
  clampSidebarWidth,
  DEFAULT_LAYOUT,
  FONT_SIZE,
  loadLayout,
  NOTES_WIDTH,
  placeNotes,
  saveLayout,
  SIDEBAR_RAIL_WIDTH,
  toggleGroup,
  type Layout,
  type SideTab,
  type SidebarGroup,
} from "./layout";
import { parseNoteFolders, type NoteFolderInfo } from "./notes";
import {
  loadWindowPrefs,
  NOTES_COMMAND,
  NOTES_MESSAGE,
  NOTES_WINDOW_LABEL,
  parseNotesMessage,
  saveContext,
  saveWindowPrefs,
  WINDOW_PREFS_KEY,
  type NotesCommand,
  type NotesWindowPrefs,
} from "./notesBridge";
import {
  flushNotes,
  NotesPanel,
  type NotesHandle,
  type NotesRequest,
  type SendTarget,
} from "./NotesPanel";
import { SidePanel } from "./SidePanel";
import { dropText, shellKind } from "./pathQuote";
import { profileErrorText, shouldReportReloadError } from "./profileFile";
import { installedKey, sameProfiles } from "./profiles";
import { arrangedProfiles, resolveBadge, startableProfiles } from "./programPrefs";
import {
  availableVersion,
  isPlanChanged,
  outcomeNotice,
  parseUpdateOutcome,
  PROGRAM_UPDATE_RESULT,
  type OutcomeNotice,
  type UpdateOutcome,
} from "./programUpdates";
import type { UpdateActions } from "./ProgramUpdateOffer";
import {
  resolveStartTarget,
  runningProjects,
  startPlaces,
  targetTooltip,
  worktreeChance,
} from "./startTarget";
import { WorktreeHint } from "./Sidebar";
import {
  backgroundSessions,
  initialSessions,
  nextWaitingId,
  numberedTitle,
  runningUpdateId,
  sessionsReducer,
  type SessionView,
} from "./sessions";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import { SearchBar } from "./SearchBar";
import {
  exportEol,
  exportFileName,
  MAX_EXPORT_BYTES,
  utf8Bytes,
  type ExportFormat,
} from "./sessionExport";
import { hasStoredSettings, loadSettings, saveSettings, type Settings } from "./settings";
import { SettingsDialog } from "./SettingsDialog";
import {
  focusCycleStep,
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
  nextFocusZone,
  SHORTCUT_LABELS,
  shortcutKeys,
  type AppAction,
  type Shortcut,
} from "./shortcuts";
import { Sidebar } from "./Sidebar";
import { Splitter } from "./Splitter";
import { badgeKind, TaskbarBadge } from "./taskbarBadge";
import { TaskbarFlasher } from "./taskbarFlash";
import { trayLabels, TrayState } from "./tray";
import { SessionTerminal, setFocusCycleActive, terminals } from "./terminal";
import { schemeColors, type TerminalLook } from "./terminalLook";
import { EmptyPane, paneStyle, TerminalPane } from "./TerminalPane";
import {
  isDue,
  ORPHANS_REFRESH_MIN_MS,
  PROFILES_REFRESH_MIN_MS,
  SCROLLBACK_QUIET_MS,
} from "./timing";
import {
  applyDocumentTheme,
  resolveTheme,
  subscribeSystemTheme,
  systemPrefersDark,
  toggledTheme,
} from "./theme";
import { usePrograms } from "./usePrograms";
import { useProgramUpdates } from "./useProgramUpdates";
import { useGitBranches } from "./useGitBranches";
import { useSessionDrag } from "./useSessionDrag";
import { useSplitView } from "./useSplitView";
import { useSubAgents } from "./useSubAgents";
import { planSubAgentPanes } from "./subAgents";
import { SubAgentPanes } from "./SubAgentPanes";
import { useUpdater } from "./useUpdater";
import { useClaims } from "./useClaims";
import { HANGAR_BUILD } from "./buildInfo";
import { withBuild } from "./version";
import { VersionText } from "./VersionText";
import { CHANGELOG, WhatsNewDialog } from "./WhatsNewDialog";
import { ChangesPanel } from "./ChangesPanel";
import { ClaimNoticeView } from "./ClaimsView";
import { revealInvisible } from "./invisible";
import {
  createErrorText,
  parseRemoval,
  parseWorktree,
  removalErrorNotice,
  orphanedWorktrees,
  removalNotice,
  worktreeInUse,
  worktreeLabel,
  worktreeTitle,
  type WorktreeNotice,
} from "./worktree";
import {
  buildWorkspace,
  EMPTY_WORKSPACE,
  mergeWorkspace,
  parseWorkspace,
  restorable,
  sameWorkspace,
  scrollbackDue,
  type SavedSession,
  type Workspace,
} from "./workspace";

const LAST_CWD_KEY = "hangar.lastCwd";
/** How long "Claude Code starts in hangar" stays after Ctrl+Shift+T. */
const START_HINT_MS = 2500;
/** How long a successful update result stays. */
const UPDATE_RESULT_MS = 12_000;
/** Initial size until xterm has measured the real space (a resize follows right after). */
const INITIAL_SIZE = { cols: 120, rows: 30 };
const COPY_KEYS: Shortcut = { mods: ["ctrl"], key: "C" };
const PASTE_KEYS: Shortcut = { mods: ["ctrl"], key: "V" };

type OpenDialog = "about" | "settings" | "whatsNew" | "launcher" | "broadcast" | null;

/** Enter after a broadcast paste comes separately, so TUIs do not take it as part of the paste. */
const BROADCAST_ENTER_DELAY_MS = 80;
/** Snippets listed directly in the terminal context menu; more are in quick access. */
const MENU_SNIPPETS = 5;

/** Debounced workspace save: several changes in quick succession result in one write. */
const WORKSPACE_SAVE_DELAY_MS = 400;
/** Save the scrollback of changed sessions regularly so that a crash costs little. */
const SCROLLBACK_SAVE_INTERVAL_MS = 30_000;
/** Quitting waits no longer than this for the save. */
const QUIT_SAVE_TIMEOUT_MS = 3000;
/** Quit waits at most this long for the taskbar badge to be removed. */
const BADGE_CLEAR_TIMEOUT_MS = 300;
/** Restore: delay between the starts of the remaining sessions after the active one. */
const RESTORE_STAGGER_MS = 150;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

/** Gives the browser time to paint and handle input before the next piece of work runs. */
function idle(): Promise<void> {
  return new Promise((resolve) => {
    if ("requestIdleCallback" in window) {
      window.requestIdleCallback(
        () => {
          resolve();
        },
        { timeout: 500 },
      );
    } else {
      // WebKit (macOS, Linux) has no requestIdleCallback.
      globalThis.setTimeout(resolve, 0);
    }
  });
}

function subscribeWindowWidth(onChange: () => void): () => void {
  window.addEventListener("resize", onChange);
  return () => {
    window.removeEventListener("resize", onChange);
  };
}

function windowWidth(): number {
  return window.innerWidth;
}

/** The event target is inside the terminal (xterm); keys and clicks there belong to the program. */
function inTerminal(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(".xterm") !== null;
}

/** Input fields and the note editor keep the context menu (cut, copy, paste). */
function editable(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])') !== null
  );
}

/** Session ID of the terminal pane at this point (CSS pixels), if one is there. */
function paneAt(x: number, y: number): number | null {
  const pane = document.elementFromPoint(x, y)?.closest<HTMLElement>(".pane[data-session]");
  const id = Number(pane?.dataset.session);
  return Number.isInteger(id) ? id : null;
}

/**
 * Restore once per page load. Lives outside React because StrictMode runs effects
 * twice.
 */
let restoreStarted = false;
/** Placeholder sessions (program missing or start failed) get negative IDs. */
let nextPlaceholderId = -1;

function isPlaceholder(id: number): boolean {
  return id < 0;
}

/** Start folder of older versions; only a fallback until the first start is recorded as recent. */
function readLastCwd(): string | null {
  try {
    return localStorage.getItem(LAST_CWD_KEY);
  } catch {
    return null;
  }
}

/**
 * How a session ended, for the chime: the exit code may reach the terminal before the reducer
 * has stored it, so the terminal's own code counts too.
 */
function sessionExit(
  session: SessionView,
  terminal: SessionTerminal,
): Pick<SessionView, "status" | "exitCode" | "stoppedByUser"> {
  const code = terminal.exitCode;
  if (code === undefined) return session;
  return { status: "exited", exitCode: code, stoppedByUser: session.stoppedByUser === true };
}

/** Is the user looking at the window? Visible and with keyboard focus. */
function windowWatched(): boolean {
  return document.visibilityState === "visible" && document.hasFocus();
}

export function App() {
  const [state, dispatch] = useReducer(sessionsReducer, initialSessions);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  /** Folder chosen in the folder menu for the next start only (startTarget.ts). */
  const [chosenCwd, setChosenCwd] = useState<string | null>(null);
  const [home, setHome] = useState<string | null>(null);
  const [legacyLastCwd] = useState(readLastCwd);
  const [error, setError] = useState<string | null>(null);
  const [layout, setLayout] = useState<Layout>(loadLayout);
  const [settings, setSettings] = useState<Settings>(() =>
    loadSettings(detectLanguage(navigator.language)),
  );
  const [version, setVersion] = useState<string | null>(null);
  const [profilePath, setProfilePath] = useState<string | null>(null);
  const [dialog, setDialog] = useState<OpenDialog>(null);
  /** What's New starting at this version; `null` shows all entries (opened from the menu). */
  const [whatsNewSince, setWhatsNewSince] = useState<string | null>(null);
  // Read before the settings are saved for the first time: has Hangar run here before?
  const [knownUser] = useState(hasStoredSettings);
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [attention, setAttention] = useState<ReadonlyMap<number, AttentionState>>(() => new Map());
  /** Why a waiting session waits (attention.ts); only sessions in "attention" have an entry. */
  const [reasons, setReasons] = useState<ReadonlyMap<number, AttentionReason>>(() => new Map());
  /** Profile list loaded once (or failed): only then does the restore run. */
  const [profilesReady, setProfilesReady] = useState(false);
  /** Restore finished; nothing is saved before that, otherwise an empty start would overwrite the saved state. */
  const [restored, setRestored] = useState(false);
  /** Startup finished (restored or failed): only then do the program queries start. */
  const [startupDone, setStartupDone] = useState(false);
  /** Sessions in the backend that the UI does not know about after a reload. */
  const [orphans, setOrphans] = useState<SessionInfo[]>([]);
  /** Session whose closing is waiting for confirmation. */
  const [closingId, setClosingId] = useState<number | null>(null);
  /** Context menu of the terminal, open at this position in the window. */
  const [terminalMenu, setTerminalMenu] = useState<{ x: number; y: number } | null>(null);
  /** Most recently started program (Ctrl+Shift+T, empty state). */
  const [lastProgramId, setLastProgramId] = useState(loadLastProgram);
  /** Clicked terminal link, for as long as its hint is shown. */
  const [link, setLink] = useState<{ url: string; copied: boolean } | null>(null);
  /** Text from the app waiting for "Send anyway" (appPaste.ts); the answer goes to the ref. */
  const [pasteCheck, setPasteCheck] = useState<{ warning: PasteWarning; name: string } | null>(
    null,
  );
  const pasteAnswer = useRef<((send: boolean) => void) | null>(null);
  const pasteCancelButton = useRef<HTMLButtonElement | null>(null);
  const viewportWidth = useSyncExternalStore(subscribeWindowWidth, windowWidth);
  const [noteFolders, setNoteFolders] = useState<NoteFolderInfo[]>([]);
  /** Favorite and recent working folders ("Starts in" menu). */
  const [folders, setFolders] = useState<FolderPrefs>(loadFolderPrefs);
  /** "Starts in" menu of the empty state, open at this position. */
  const [emptyFolderMenu, setEmptyFolderMenu] = useState<{
    x: number;
    y: number;
    flipAbove: number;
  } | null>(null);
  const emptyChangeButton = useRef<HTMLButtonElement>(null);
  /** Search bar of a session; `token` changes on every Ctrl+Shift+F to refocus the field. */
  const [search, setSearch] = useState<{ sessionId: number; token: number } | null>(null);
  /** Pane under files dragged from the file manager. */
  const [dropTarget, setDropTarget] = useState<number | null>(null);
  /** Session whose marker is being edited. */
  const [markerId, setMarkerId] = useState<number | null>(null);
  /** Session menu (right click on a session), open at this position. */
  const [sessionMenu, setSessionMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  /**
   * Session whose sub-agents the body shows (sub-agent view), or `null`. Only while that session
   * is the active one; switching sessions or arranging the panes ends the view.
   */
  const [subAgentsOf, setSubAgentsOf] = useState<number | null>(null);
  /** Layout picker (View, quick access) for the active session; `preview` is its chosen zone. */
  const [snapPicker, setSnapPicker] = useState<{ preview: SnapTarget | null } | null>(null);
  /** Screen reader message after a session moved in the sidebar. */
  const [moveAnnouncement, setMoveAnnouncement] = useState("");
  // Async so that a synchronous error (browser without Tauri) also arrives as a rejection.
  const [flasher] = useState(
    () =>
      new TaskbarFlasher(async (kind) => {
        const type =
          kind === null
            ? null
            : kind === "critical"
              ? UserAttentionType.Critical
              : UserAttentionType.Informational;
        await getCurrentWindow().requestUserAttention(type);
      }),
  );
  const [badge] = useState(() => {
    const kind = badgeKind(navigator.userAgent);
    return new TaskbarBadge(kind, async (count, text, color) => {
      const appWindow = getCurrentWindow();
      if (kind === "count") {
        await appWindow.setBadgeCount(text === null ? undefined : count);
        return;
      }
      if (text === null) {
        await appWindow.setOverlayIcon(undefined);
        return;
      }
      const png = await drawBadgePng(text, color);
      const image = await Image.fromBytes(png);
      try {
        await appWindow.setOverlayIcon(image);
      } finally {
        await image.close();
      }
    });
  });
  const [tray] = useState(() => new TrayState(api.configureTray));
  /** Request for the notes panel (quick note, focus, terminal output); a new token each time. */
  const [notesRequest, setNotesRequest] = useState<NotesRequest | null>(null);
  const notesTokens = useRef(0);
  /** Goes up when the notes window changed notes: the docked panel reloads. */
  const [notesRefresh, setNotesRefresh] = useState(0);
  const notesHandle = useRef<NotesHandle | null>(null);
  /** Notes in their own window, kept on top; remembered per machine. */
  const [notesWindow, setNotesWindow] = useState<NotesWindowPrefs>(loadWindowPrefs);
  /** Changes tab of the right panel (Ctrl+Shift+D): focus on opening. */
  const [changesFocusToken, setChangesFocusToken] = useState(0);
  /** Short confirmation after Ctrl+Shift+T: which program starts where. */
  const [startHint, setStartHint] = useState<string | null>(null);
  /** Program whose update waits for "Update anyway" because it is running. */
  const [confirmUpdate, setConfirmUpdate] = useState<string | null>(null);
  /** Result of the last update session, until dismissed. */
  const [updateResult, setUpdateResult] = useState<
    (OutcomeNotice & { sessionId: number | null; key: number }) | null
  >(null);
  /** The update plan changed since it was shown: the program's name, until checked again. */
  const [planChanged, setPlanChanged] = useState<string | null>(null);
  /** A worktree is being created (checking out can take a while). */
  const [creatingWorktree, setCreatingWorktree] = useState(false);
  /** What stayed after closing a worktree session, with its path to copy. */
  const [worktreeNotice, setWorktreeNotice] = useState<
    (WorktreeNotice & { copied: boolean }) | null
  >(null);
  /** A new collision of two sessions on one file (claims.ts), until dismissed or replaced. */
  const [claimNotice, setClaimNotice] = useState<{ overlap: Overlap; key: number } | null>(null);
  const updater = useUpdater(version, settings.autoUpdate);
  const installed = installedKey(profiles);
  const programs = usePrograms(installed, settings.showVersions, settings.showIcons, startupDone);
  const programUpdates = useProgramUpdates(
    installed,
    settings.checkProgramUpdates,
    programs.versionsSettled && startupDone,
  );
  const systemDark = useSyncExternalStore(subscribeSystemTheme, systemPrefersDark);
  const theme = resolveTheme(settings.theme, systemDark);
  const terminalLook = useMemo<TerminalLook>(
    () => ({
      scheme: settings.terminalScheme,
      font: settings.terminalFont,
      cursorStyle: settings.cursorStyle,
      cursorBlink: settings.cursorBlink,
    }),
    [settings.terminalScheme, settings.terminalFont, settings.cursorStyle, settings.cursorBlink],
  );

  const t = useMemo(() => createTranslator(settings.language), [settings.language]);
  const attentionConfig = useMemo<AttentionConfig>(
    () => ({ enabled: settings.attentionEnabled, quietMs: settings.quietSeconds * 1000 }),
    [settings.attentionEnabled, settings.quietSeconds],
  );

  // For callbacks outside React (terminal timers, window events).
  const activeIdRef = useRef(state.activeId);
  const stateRef = useRef(state);
  /** Sessions visible in a pane (kept in sync below), read by the attention listener. */
  const shownRef = useRef<ReadonlySet<number>>(new Set());
  /**
   * Titles of just started sessions that are not in the state yet. Started in quick succession
   * (double click, Ctrl+Shift+T), both would otherwise get the same number.
   */
  const claimedTitles = useRef(new Map<number, string>());
  const settingsRef = useRef(settings);
  /** One soft sound for sessions that start waiting out of sight (chime.ts). */
  const [chimes] = useState(() => new ChimeScheduler(systemClock, playChime));
  const profilesRef = useRef(profiles);
  /** Values for new terminals, also from asynchronous flows (restore). */
  const terminalConfig = useRef({
    fontSize: layout.fontSize,
    attentionConfig,
    theme,
    look: terminalLook,
    scrollback: settings.scrollbackLines,
  });
  const lastSavedWorkspace = useRef<Workspace | null>(null);
  /**
   * The workspace may only be saved after the restore; before that, an empty start would
   * overwrite the saved state.
   */
  const savingAllowed = useRef(false);
  /**
   * Saved state when only the UI was reloaded and the backend still has sessions it does not
   * know about. Saving then merges (mergeWorkspace), it never overwrites.
   */
  const rememberedWorkspace = useRef<Workspace | null>(null);
  const quitting = useRef(false);
  /** A modal dialog is open; app shortcuts then have no effect behind it. */
  const modalRef = useRef(false);
  /** Running query of the profile list; while it runs, no second one starts. */
  const profilesLoading = useRef<Promise<void> | null>(null);
  /** Start of the last profile list query (performance.now). */
  const profilesLoadedAt = useRef<number | null>(null);
  /** Last reported error while reloading profiles.toml; `null` after a success. */
  const reloadErrorShown = useRef<string | null>(null);
  /**
   * The profile file was opened for editing: the next return to the window reloads it
   * without throttling so that the saved changes show up in the list right away.
   */
  const profileEditPending = useRef(false);
  /** Background session hint visible: only then does returning to the window check again. */
  const orphansShown = useRef(false);
  /** Last query of the background sessions (performance.now). */
  const orphansCheckedAt = useRef<number | null>(null);
  /** Periodic scrollback save is running; a second one waits for the next interval. */
  const scrollbackSaving = useRef(false);

  const showError = useCallback((e: unknown) => {
    setError(errorText(e));
  }, []);

  /**
   * Loads the profile list. Focus and visibility often both fire on return; the running
   * query acts as a lock, so two responses never race each other.
   */
  const loadProfiles = useCallback((onError: (e: unknown) => void) => {
    if (profilesLoading.current) return;
    profilesLoadedAt.current = performance.now();
    profilesLoading.current = api
      .listProfiles()
      .then((next) => {
        setProfiles((current) => (sameProfiles(current, next) ? current : next));
      })
      .catch(onError)
      .finally(() => {
        profilesLoading.current = null;
        setProfilesReady(true);
      });
  }, []);

  /**
   * Rereads profiles.toml in the backend (button in Settings, return to the window).
   * If a query is already running, it waits for that one so two responses never race each other.
   * On error the current list stays; the rejection goes to the caller.
   */
  const reloadProfiles = useCallback(async (): Promise<Profile[]> => {
    while (profilesLoading.current) await profilesLoading.current;
    profilesLoadedAt.current = performance.now();
    profileEditPending.current = false;
    const task = api.reloadProfiles();
    // Registered before the caller's `await`, so it runs first: the list is already in place by then.
    profilesLoading.current = task
      .then(
        (next) => {
          reloadErrorShown.current = null;
          setProfiles((current) => (sameProfiles(current, next) ? current : next));
        },
        () => undefined,
      )
      .finally(() => {
        profilesLoading.current = null;
      });
    return task;
  }, []);

  /** For Settings: an error shown there does not reappear when switching windows. */
  const reloadProfilesFromSettings = useCallback(async () => {
    try {
      await reloadProfiles();
    } catch (e: unknown) {
      reloadErrorShown.current = profileErrorText(
        e,
        createTranslator(settingsRef.current.language),
      );
      throw e;
    }
  }, [reloadProfiles]);

  const openProfileFile = useCallback(async () => {
    await api.openProfileFile();
    profileEditPending.current = true;
  }, []);

  useEffect(() => {
    loadProfiles(showError);
    api.defaultCwd().then(setHome).catch(showError);
    getVersion()
      .then((v) => {
        setVersion(withBuild(v, HANGAR_BUILD));
        // After an update, show once what's new. The version is stored right away so the
        // dialog does not come back even after a crash or reload.
        const decision = decideWhatsNew(CHANGELOG, v, loadLastSeen(), knownUser);
        if (decision.remember !== null) saveLastSeen(decision.remember);
        if (!decision.show) return;
        setWhatsNewSince(decision.since);
        setDialog((current) => current ?? "whatsNew");
      })
      .catch(showError);
    // Only for the About dialog; without a response it shows a text without a path.
    api
      .profilePath()
      .then(setProfilePath)
      .catch(() => undefined);
  }, [loadProfiles, showError, knownUser]);

  useEffect(() => {
    saveLayout(layout);
  }, [layout]);

  useEffect(() => {
    stateRef.current = state;
    for (const session of state.sessions) claimedTitles.current.delete(session.id);
    settingsRef.current = settings;
    profilesRef.current = profiles;
    terminalConfig.current = {
      fontSize: layout.fontSize,
      attentionConfig,
      theme,
      look: terminalLook,
      scrollback: settings.scrollbackLines,
    };
  });

  useEffect(() => {
    saveSettings(settings);
  }, [settings]);

  useEffect(() => {
    setCurrentLanguage(settings.language);
    document.documentElement.lang = settings.language;
  }, [settings.language]);

  useEffect(() => {
    applyDocumentTheme(theme);
    // The area around the terminals takes the scheme's background, so no frame shows.
    document.documentElement.style.setProperty(
      "--terminal-bg",
      schemeColors(terminalLook.scheme, theme).background,
    );
    // Change options only: xterm repaints, content and scrollback stay.
    for (const terminal of terminals.values()) terminal.setLook(terminalLook, theme);
  }, [theme, terminalLook]);

  useEffect(() => {
    saveFolderPrefs(folders);
  }, [folders]);

  useEffect(() => {
    modalRef.current = dialog !== null || closingId !== null || markerId !== null;
  }, [dialog, closingId, markerId]);

  useEffect(() => {
    orphansShown.current = orphans.length > 0;
  }, [orphans]);

  useEffect(() => {
    for (const terminal of terminals.values()) terminal.setScrollback(settings.scrollbackLines);
  }, [settings.scrollbackLines]);

  useEffect(() => {
    const style = document.documentElement.style;
    const tint = attentionTint(settings.attentionColor, theme);
    style.setProperty("--attention", settings.attentionColor);
    style.setProperty("--on-attention", textOnColor(settings.attentionColor));
    style.setProperty("--attention-pulse-low", `${String(tint.low)}%`);
    style.setProperty("--attention-pulse-high", `${String(tint.high)}%`);
    // Edge, ring and chip keep 3 : 1 against the sidebar in both themes (attentionLook.ts).
    const edges = attentionEdges(settings.attentionColor, theme);
    style.setProperty("--attention-strong", edges.strong);
    style.setProperty("--attention-ring", edges.ring);
    style.setProperty("--attention-chip-bg", edges.chipBg);
    style.setProperty("--attention-chip-fg", edges.chipFg);
  }, [settings.attentionColor, theme]);

  useEffect(() => {
    for (const terminal of terminals.values()) terminal.attention.setConfig(attentionConfig);
  }, [attentionConfig]);

  useEffect(() => {
    if (!settings.taskbarFlash || !settings.attentionEnabled) flasher.stop();
  }, [flasher, settings.taskbarFlash, settings.attentionEnabled]);

  useEffect(() => {
    for (const terminal of terminals.values()) terminal.setFontSize(layout.fontSize);
  }, [layout.fontSize]);

  // Whoever shows a session has seen its hint.
  useEffect(() => {
    activeIdRef.current = state.activeId;
    if (state.activeId !== null && windowWatched()) {
      terminals.get(state.activeId)?.attention.seen();
    }
  }, [state.activeId]);

  useEffect(() => {
    const onReturn = () => {
      const id = activeIdRef.current;
      if (id !== null && windowWatched()) terminals.get(id)?.attention.seen();
      // Window is in front: stop the taskbar flash.
      if (document.hasFocus()) flasher.stop();
      // Anyone who installed an agent or changed profiles.toml in the meantime sees it
      // without a restart. The backend reads the file and searches the PATH asynchronously. An error
      // in the file appears once, not on every window switch; the current list stays.
      // Throttled: focus and visibility change often, and each PATH search has a noticeable cost.
      // After "Open profile file" without throttling so the saved changes appear right away.
      const due =
        profileEditPending.current ||
        isDue(profilesLoadedAt.current, performance.now(), PROFILES_REFRESH_MIN_MS);
      if (document.visibilityState === "visible" && due) {
        reloadProfiles().catch((e: unknown) => {
          const text = profileErrorText(e, createTranslator(settingsRef.current.language));
          if (!shouldReportReloadError(reloadErrorShown.current, text)) return;
          reloadErrorShown.current = text;
          setError(text);
        });
      }
      // Background sessions may have ended in the meantime (e.g. from outside): refresh the count,
      // at zero the hint disappears. Throttled like the profile list, but shorter, because the
      // query only reads the backend's list.
      const now = performance.now();
      if (orphansShown.current && isDue(orphansCheckedAt.current, now, ORPHANS_REFRESH_MIN_MS)) {
        orphansCheckedAt.current = now;
        api
          .listSessions()
          .then((running) => {
            const known = new Set(stateRef.current.sessions.map((session) => session.id));
            const next = backgroundSessions(running, known);
            setOrphans((current) => (current.length === 0 ? current : next));
          })
          .catch(() => undefined);
      }
    };
    window.addEventListener("focus", onReturn);
    document.addEventListener("visibilitychange", onReturn);
    return () => {
      window.removeEventListener("focus", onReturn);
      document.removeEventListener("visibilitychange", onReturn);
    };
  }, [reloadProfiles, flasher]);

  const activeTerminal = () =>
    state.activeId === null ? undefined : terminals.get(state.activeId);
  const activeSession = state.sessions.find((s) => s.id === state.activeId);
  /** Session name for the claims texts; a closed session keeps its number. */
  const sessionName = (id: number) =>
    state.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
  // Local claims: a calm notice in the window for each new collision, and a desktop notification
  // while Hangar is in the background, under the same settings as for waiting sessions.
  const claims = useClaims(settings.claimsEnabled, (fresh) => {
    const first = fresh[0];
    if (first === undefined) return;
    setClaimNotice({ overlap: first, key: Date.now() });
    const current = settingsRef.current;
    if (!current.notifyDesktop || !current.attentionEnabled || document.hasFocus()) return;
    const name = (id: number) =>
      stateRef.current.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
    const text = overlapNotice(first, name, createTranslator(current.language));
    api.notifySession(first.sessionId, text.title, text.body).catch(() => undefined);
  });
  /** Sidebar marker text per session that collides with another one on a file. */
  const overlapMarkers = useMemo(() => {
    const name = (id: number) => state.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
    const markers = new Map<number, string>();
    for (const session of state.sessions) {
      const text = markerText(overlapsOf(claims, session.id), name, t);
      if (text !== null) markers.set(session.id, text);
    }
    return markers;
  }, [claims, state.sessions, t]);
  /** Where the next session starts: the active project, a chosen folder, the last one, home. */
  const target = resolveStartTarget({
    chosen: chosenCwd,
    active: activeSession,
    lastUsed: folders.recent[0] ?? legacyLastCwd,
    home,
  });
  const targetPath = target.path;
  /** For callbacks that start sessions; they run after the render that showed this target. */
  const targetRef = useRef(targetPath);
  useEffect(() => {
    targetRef.current = targetPath;
  });
  const running = runningProjects(state.sessions);
  const sessionFolder: NoteFolder | null = activeSession ? { path: activeSession.cwd } : null;
  /** Running sessions that can receive text from a note. */
  const sendTargets = useMemo(
    (): SendTarget[] =>
      state.sessions
        .filter((session) => session.status !== "exited")
        .map((session) => ({ id: session.id, title: session.title })),
    [state.sessions],
  );
  // Detached: the button and menu bring the notes window to the front, they hide nothing.
  const notesShown = !notesWindow.detached && layout.sideOpen && layout.sideTab === "notes";
  const changesShown = layout.sideOpen && layout.sideTab === "changes";

  if (search !== null && search.sessionId !== state.activeId) setSearch(null);

  // The marker dialog or session menu of a session that is gone (closed, or replaced by a restart
  // with a new ID) closes, or the modal flag would keep blocking the shortcuts.
  if (markerId !== null && !state.sessions.some((s) => s.id === markerId)) setMarkerId(null);
  if (sessionMenu !== null && !state.sessions.some((s) => s.id === sessionMenu.id)) {
    setSessionMenu(null);
  }
  // The sub-agent view belongs to the active session: another one becoming active, or the
  // session closing, ends it (and with it the watch).
  if (subAgentsOf !== null && subAgentsOf !== state.activeId) setSubAgentsOf(null);

  /**
   * Focus back to the terminal after a menu, dialog or rename. If another element has taken
   * focus in the meantime (input field, dialog, clicked button), it stays there.
   */
  const focusActive = useCallback(() => {
    requestAnimationFrame(() => {
      const focused = document.activeElement;
      const free = !focused || focused === document.body || focused.closest(".app-menu") !== null;
      const id = activeIdRef.current;
      if (free && id !== null) terminals.get(id)?.focus();
    });
  }, []);

  /** After a mouse click on a button: focus back to the terminal, even if the button has it. */
  const focusTerminal = useCallback(() => {
    requestAnimationFrame(() => {
      const id = activeIdRef.current;
      if (id !== null) terminals.get(id)?.focus();
    });
  }, []);

  /** Shows the warning for the session `name` and resolves with the user's answer. */
  const confirmPaste = useCallback(
    (name: string) =>
      (warning: PasteWarning): Promise<boolean> => {
        // A newer send replaces a question still open: the older text is not sent.
        pasteAnswer.current?.(false);
        return new Promise<boolean>((resolve) => {
          pasteAnswer.current = resolve;
          setPasteCheck({ warning, name });
        });
      },
    [],
  );

  /** Jump from a claims warning or notice to the other session. */
  const showSession = (id: number) => {
    if (!state.sessions.some((s) => s.id === id)) return;
    dispatch({ type: "activated", id });
    focusTerminal();
  };

  const answerPaste = (send: boolean) => {
    const answer = pasteAnswer.current;
    pasteAnswer.current = null;
    setPasteCheck(null);
    answer?.(send);
    focusTerminal();
  };

  // The question takes the keyboard, so Enter typed next cannot land in the terminal unseen.
  useEffect(() => {
    if (pasteCheck !== null) pasteCancelButton.current?.focus();
  }, [pasteCheck]);

  /** Note or snippet text into a session, after a warning when it would run lines or hides some. */
  const pasteIntoSession = useCallback(
    (id: number, text: string) => {
      const terminal = terminals.get(id);
      if (!terminal) return;
      const name = stateRef.current.sessions.find((s) => s.id === id)?.title ?? "";
      // The session may close while the question is open: then nothing is sent.
      const session = {
        bracketedPaste: () => terminal.bracketedPaste(),
        pasteText: (clean: string) => {
          if (terminals.get(id) === terminal) terminal.pasteText(clean);
        },
      };
      void pasteFromApp(session, text, confirmPaste(name)).then((pasted) => {
        if (pasted) focusTerminal();
      });
    },
    [confirmPaste, focusTerminal],
  );

  const splitView = useSplitView({
    layout: layout.snapLayout,
    sessions: state.sessions,
    activeId: state.activeId,
    setLayout,
    activate: (id) => {
      dispatch({ type: "activated", id });
    },
    focusTerminal,
    onArrange: () => {
      setSubAgentsOf(null);
    },
  });

  // Sub-agent view: the watch runs while the view is open and the session has sub-agents; a
  // restart or resume fallback with a new conversation id starts a new watch.
  const subAgentSession =
    subAgentsOf === null ? undefined : state.sessions.find((s) => s.id === subAgentsOf);
  const subAgentsAvailable =
    subAgentSession?.subAgents === true && !isPlaceholder(subAgentsOf ?? 0);
  const subAgentWatch = useSubAgents(
    subAgentSession !== undefined && subAgentsAvailable
      ? {
          sessionId: subAgentSession.id,
          cliSessionId: subAgentSession.cliSessionId ?? null,
          excerpts: settings.subAgentExcerpts,
        }
      : null,
  );
  const subAgentNotice =
    subAgentWatch.error !== null
      ? t("subAgents.error", { error: subAgentWatch.error })
      : !subAgentsAvailable
        ? t("subAgents.unavailable")
        : subAgentWatch.view.located === null
          ? t("subAgents.loading")
          : !subAgentWatch.view.located
            ? t("subAgents.notLocated")
            : t("subAgents.none");
  const subAgentPlan =
    subAgentSession === undefined
      ? null
      : planSubAgentPanes(
          subAgentWatch.view,
          subAgentWatch.error !== null ||
            !subAgentsAvailable ||
            subAgentWatch.view.located !== true,
        );
  // Sessions shown in a pane right now; the chime stays silent for them while the window has focus.
  const shown = new Set<number>(state.activeId === null ? [] : [state.activeId]);
  if (subAgentPlan === null) {
    for (const id of splitView.split.slots) if (id !== null) shown.add(id);
  }
  const shownKey = [...shown].sort((a, b) => a - b).join(",");
  useEffect(() => {
    shownRef.current = new Set(
      shownKey === "" ? [] : shownKey.split(",").map((part) => Number(part)),
    );
  }, [shownKey]);

  /** Opens or closes the sub-agent view of a session; opening makes the session active. */
  const toggleSubAgents = (id: number) => {
    if (subAgentsOf === id) {
      setSubAgentsOf(null);
    } else {
      dispatch({ type: "activated", id });
      setSubAgentsOf(id);
    }
    focusTerminal();
  };
  const closeSubAgents = () => {
    setSubAgentsOf(null);
    focusTerminal();
  };

  /** New place of a session in the sidebar (drag or Alt+arrow); the workspace saves the order. */
  const moveSession = (id: number, index: number) => {
    const session = state.sessions.find((s) => s.id === id);
    dispatch({ type: "moved", id, index });
    if (session) {
      const count = state.sessions.length;
      const position = Math.min(Math.max(index, 0), count - 1) + 1;
      setMoveAnnouncement(t("session.moved", { title: session.title, position, count }));
    }
  };

  const {
    drag: sessionDragView,
    begin: beginSessionDrag,
    attachGhost: attachDragGhost,
  } = useSessionDrag({
    layout: layout.snapLayout,
    order: state.sessions.map((s) => s.id),
    onMove: moveSession,
    onPlace: splitView.place,
  });
  const draggedId = sessionDragView?.id ?? null;
  const draggedSession =
    draggedId === null ? undefined : state.sessions.find((s) => s.id === draggedId);

  const openSnapPicker =
    state.activeId === null
      ? null
      : () => {
          setSnapPicker({ preview: null });
        };
  const closeSnapPicker = () => {
    setSnapPicker(null);
    focusTerminal();
  };

  /** Connects a terminal to its session ID: exit, attention, visibility. */
  const wire = useCallback(
    (terminal: SessionTerminal, id: number) => {
      terminal.onExit = (code) => {
        dispatch({ type: "exited", id, code });
      };
      terminal.onFallback = (fallback) => {
        dispatch({ type: "fellBack", id, ...fallback });
      };
      terminal.isWatched = () => activeIdRef.current === id && windowWatched();
      terminal.onLink = (url) => {
        setLink({ url, copied: false });
      };
      // Only state changes arrive here, never individual output chunks. A change of the reason
      // alone (a bell or an exit while already waiting) does not flash or notify again.
      let previous: AttentionState = "idle";
      let previousReason: AttentionReason | null = null;
      terminal.setAttentionListener((next, reason) => {
        const entered = previous !== "attention" || next !== "attention";
        const session = stateRef.current.sessions.find((s) => s.id === id);
        const chime =
          session === undefined || isPlaceholder(id)
            ? null
            : chimeFor({ previous, previousReason, next, reason }, sessionExit(session, terminal), {
                enabled: settingsRef.current.chime && settingsRef.current.attentionEnabled,
                windowFocused: windowWatched(),
                shown: shownRef.current.has(id),
              });
        previous = next;
        previousReason = reason;
        if (chime !== null) chimes.request(chime, settingsRef.current.chimeVolume);
        setReasons((current) => {
          if ((current.get(id) ?? null) === reason) return current;
          const map = new Map(current);
          if (reason === null) map.delete(id);
          else map.set(id, reason);
          return map;
        });
        if (!entered) return;
        flasher.attentionChanged(next, {
          enabled: settingsRef.current.taskbarFlash,
          windowFocused: document.hasFocus(),
        });
        const notify = shouldNotify(next, {
          enabled: settingsRef.current.notifyDesktop,
          windowFocused: document.hasFocus(),
        });
        if (notify && session && !isPlaceholder(id)) {
          const text = notificationText(
            session,
            terminal.exitCode !== undefined,
            createTranslator(settingsRef.current.language),
          );
          api.notifySession(id, text.title, text.body).catch(() => undefined);
        }
        setAttention((current) => {
          if ((current.get(id) ?? "idle") === next) return current;
          const map = new Map(current);
          map.set(id, next);
          return map;
        });
      });
      terminals.set(id, terminal);
    },
    [flasher, chimes],
  );

  /**
   * Events that arrived before `create_session` answered and the session existed: a very short
   * process has already exited, or resuming has already fallen back to a normal start.
   */
  const catchUp = useCallback((terminal: SessionTerminal, id: number) => {
    if (terminal.fallback !== undefined) dispatch({ type: "fellBack", id, ...terminal.fallback });
    if (terminal.exitCode !== undefined) dispatch({ type: "exited", id, code: terminal.exitCode });
  }, []);

  const forgetAttention = useCallback((id: number) => {
    setAttention((current) => {
      if (!current.has(id)) return current;
      const map = new Map(current);
      map.delete(id);
      return map;
    });
    setReasons((current) => {
      if (!current.has(id)) return current;
      const map = new Map(current);
      map.delete(id);
      return map;
    });
  }, []);

  /** Desired title (restored or numbered); if the rename fails, the program name stays. */
  const withTitle = useCallback(async (info: SessionInfo, title: string) => {
    if (title.trim() === "" || title === info.title) return info;
    try {
      return await api.rename(info.id, title);
    } catch {
      return info;
    }
  }, []);

  /**
   * `folder`: start there instead of the start target (quick access). `ownWorktree`: first a
   * git worktree with a branch of its own is created from that folder, and the session runs in it.
   * A start in the target uses up a chosen folder: the next one follows the active session again.
   */
  const start = useCallback(
    async (profile: Profile, folder?: string, ownWorktree = false) => {
      const base = folder ?? targetRef.current;
      let worktree: Worktree | undefined;
      if (ownWorktree && base !== null) {
        setCreatingWorktree(true);
        try {
          const made = parseWorktree(await api.createWorktree(base, worktreeLabel(profile.name)));
          if (made === null) throw new Error(t("error.unknown"));
          worktree = made;
        } catch (e) {
          setError(createErrorText(e, t));
          return;
        } finally {
          setCreatingWorktree(false);
        }
      }
      const terminal = new SessionTerminal({
        fontSize: layout.fontSize,
        attention: attentionConfig,
        theme,
        look: terminalLook,
        scrollback: settings.scrollbackLines,
      });
      try {
        const info = await api.createSession(
          profile.id,
          worktree?.path ?? base,
          INITIAL_SIZE,
          terminal.createChannel(),
        );
        terminal.setId(info.id);
        wire(terminal, info.id);
        // The same program twice: "PowerShell 7 (2)", so the sidebar can tell them apart.
        // A real name, not a display trick: rename and restore keep it.
        const others = [
          ...stateRef.current.sessions.map((session) => session.title),
          ...claimedTitles.current.values(),
        ];
        // In an own worktree the branch tells sessions apart: "Claude Code · 1a2b3c".
        const title = numberedTitle(
          worktree ? worktreeTitle(info.title, worktree.branch) : info.title,
          others,
        );
        claimedTitles.current.set(info.id, title);
        if (folder === undefined) setChosenCwd(null);
        dispatch({
          type: "added",
          session: await withTitle(info, title),
          ...(worktree ? { worktree } : {}),
        });
        saveLastProgram(profile.id);
        setLastProgramId(profile.id);
        // The folder the session really runs in (the backend may fall back to the home folder);
        // for a worktree the repository's folder, so recent folders do not fill with worktrees.
        const used = worktree === undefined ? info.cwd : base;
        if (used !== null) setFolders((current) => recordRecent(current, used));
        // Very short processes may already have exited before the response arrives.
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
        }
      } catch (e) {
        terminal.dispose();
        showError(e);
        // Nothing runs in the new worktree yet: take it away again (never forced).
        if (worktree) api.removeWorktree(worktree).catch(() => undefined);
      }
    },
    [
      layout.fontSize,
      attentionConfig,
      theme,
      terminalLook,
      settings.scrollbackLines,
      showError,
      wire,
      withTitle,
      t,
    ],
  );

  /**
   * After an update session ends: detect versions again and recheck them so the
   * update dot disappears. The backend cleared its version cache before the exit event.
   */
  const afterUpdateRef = useRef<() => void>(() => undefined);
  /** Update plans as last shown, for the fingerprint sent with "Update". */
  const updatesRef = useRef(programUpdates.updates);
  useEffect(() => {
    updatesRef.current = programUpdates.updates;
    afterUpdateRef.current = () => {
      programs.refreshVersions();
      programUpdates.recheck();
    };
  });

  /**
   * Start a program's update as a session of its own. The backend derives the command from the
   * profile; the session is not added to the saved workspace and so does not repeat the update
   * on the next start.
   */
  const pendingUpdates = useRef(new Set<string>());
  const startUpdate = useCallback(
    async (profileId: string) => {
      // At most one update per program: a second one would work on the same files.
      const running = runningUpdateId(stateRef.current.sessions, profileId);
      if (running !== null) {
        dispatch({ type: "activated", id: running });
        return;
      }
      if (pendingUpdates.current.has(profileId)) return;
      pendingUpdates.current.add(profileId);
      const terminal = new SessionTerminal({
        fontSize: layout.fontSize,
        attention: attentionConfig,
        theme,
        look: terminalLook,
        scrollback: settings.scrollbackLines,
      });
      try {
        // No working folder from here: the backend uses the profile's or the home folder.
        const info = await api.createUpdateSession(
          profileId,
          INITIAL_SIZE,
          terminal.createChannel(),
          updatesRef.current.get(profileId)?.planId ?? null,
        );
        terminal.setId(info.id);
        wire(terminal, info.id);
        // Also holds after every restart, because `restart` does not rewire the terminal.
        terminal.onExit = (code) => {
          dispatch({ type: "exited", id: info.id, code });
          afterUpdateRef.current();
        };
        const others = [
          ...stateRef.current.sessions.map((session) => session.title),
          ...claimedTitles.current.values(),
        ];
        // The backend's title is not translated; the sidebar shows it in the app's language.
        const name = profilesRef.current.find((p) => p.id === profileId)?.name;
        const base = name === undefined ? info.title : t("update.sessionTitle", { name });
        const title = numberedTitle(base, others);
        claimedTitles.current.set(info.id, title);
        dispatch({ type: "added", session: await withTitle(info, title) });
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
          afterUpdateRef.current();
        }
      } catch (e) {
        terminal.dispose();
        if (isPlanChanged(errorText(e))) {
          // Never run what was not shown: name it and offer a fresh check.
          const name = profilesRef.current.find((p) => p.id === profileId)?.name ?? profileId;
          setPlanChanged(name);
          afterUpdateRef.current();
        } else {
          showError(e);
        }
      } finally {
        pendingUpdates.current.delete(profileId);
      }
    },
    [
      layout.fontSize,
      attentionConfig,
      theme,
      terminalLook,
      settings.scrollbackLines,
      showError,
      wire,
      withTitle,
      t,
    ],
  );

  /** Session without a process, e.g. because the program is missing. "Restart" tries again. */
  const addPlaceholder = useCallback(
    (terminal: SessionTerminal, saved: SavedSession): number => {
      const id = nextPlaceholderId;
      nextPlaceholderId -= 1;
      terminal.exitCode = null;
      wire(terminal, id);
      dispatch({
        type: "restored",
        order: saved.order,
        marker: { color: saved.color, emoji: saved.emoji },
        ...(saved.worktree ? { worktree: saved.worktree } : {}),
        session: {
          id,
          profileId: saved.profileId,
          title: saved.title.trim() === "" ? saved.profileId : saved.title,
          cwd: saved.cwd,
          status: "exited",
          exitCode: null,
          createdAt: Date.now(),
          // Kept, so the next save does not lose it and "Restart" resumes this conversation.
          cliSessionId: saved.cliSessionId,
        },
      });
      return id;
    },
    [wire],
  );

  /**
   * Recreate a saved session: first the scrollback into the terminal, the divider line below it,
   * then the process with `resume`. If the program is missing or the start fails, the session
   * stays with a hint instead of disappearing.
   */
  const restoreSession = useCallback(
    async (saved: SavedSession, banner: string): Promise<number> => {
      const config = terminalConfig.current;
      const terminal = new SessionTerminal({
        fontSize: config.fontSize,
        attention: config.attentionConfig,
        theme: config.theme,
        look: config.look,
        scrollback: config.scrollback,
        key: saved.key,
      });
      // Same width as when the process starts, so long lines wrap correctly.
      terminal.term.resize(INITIAL_SIZE.cols, INITIAL_SIZE.rows);
      let data: string | null;
      try {
        const value = await api.loadScrollback(saved.key);
        data = typeof value === "string" && value.length > 0 ? value : null;
      } catch {
        data = null;
      }
      if (data !== null) await terminal.restore(data, banner);
      else terminal.writeNotice(banner);

      const tr = createTranslator(settingsRef.current.language);
      const profile = profilesRef.current.find((p) => p.id === saved.profileId);
      if (!profile?.installed) {
        terminal.writeNotice(
          profile
            ? tr("terminal.notInstalled", { name: profile.name })
            : tr("terminal.unknownProfile", { name: saved.profileId }),
        );
        return addPlaceholder(terminal, saved);
      }
      try {
        const info = await api.createSession(
          saved.profileId,
          saved.cwd,
          INITIAL_SIZE,
          terminal.createChannel(),
          true,
          saved.cliSessionId,
        );
        terminal.setId(info.id);
        wire(terminal, info.id);
        dispatch({
          type: "restored",
          order: saved.order,
          marker: { color: saved.color, emoji: saved.emoji },
          ...(saved.worktree ? { worktree: saved.worktree } : {}),
          session: await withTitle(info, saved.title),
        });
        catchUp(terminal, info.id);
        return info.id;
      } catch (e) {
        terminal.writeNotice(tr("terminal.startFailed", { error: errorText(e) }));
        return addPlaceholder(terminal, saved);
      }
    },
    [addPlaceholder, wire, withTitle, catchUp],
  );

  /** Ended by the user: the exit is no news to them, so the session does not pulse. */
  const kill = useCallback(
    (id: number) => {
      const terminal = terminals.get(id);
      terminal?.attention.stopRequested();
      dispatch({ type: "stopRequested", id });
      api.kill(id).catch((e: unknown) => {
        terminal?.attention.stopFailed();
        dispatch({ type: "stopFailed", id });
        showError(e);
      });
    },
    [showError],
  );

  const restart = useCallback(
    async (id: number) => {
      const terminal = terminals.get(id);
      if (!terminal) return;
      if (isPlaceholder(id)) {
        // Placeholder: new process with a new ID, same terminal with its scrollback.
        const session = stateRef.current.sessions.find((s) => s.id === id);
        if (!session) return;
        try {
          terminal.markRestarted();
          const info = await api.createSession(
            session.profileId,
            session.cwd,
            terminal.size(),
            terminal.createChannel(),
            true,
            session.cliSessionId ?? null,
          );
          terminal.setId(info.id);
          terminals.delete(id);
          wire(terminal, info.id);
          forgetAttention(id);
          dispatch({ type: "replaced", oldId: id, session: await withTitle(info, session.title) });
          catchUp(terminal, info.id);
        } catch (e) {
          terminal.exitCode = null;
          terminal.writeNotice(t("terminal.startFailed", { error: errorText(e) }));
        }
        return;
      }
      try {
        terminal.markRestarted();
        const info = await api.restart(id, terminal.size(), terminal.createChannel());
        dispatch({ type: "updated", session: info });
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id, code: terminal.exitCode });
        }
      } catch (e) {
        showError(e);
      }
    },
    [showError, wire, forgetAttention, withTitle, catchUp, t],
  );

  /**
   * After a worktree session closed: remove its worktree. git never forces, so changes stay; the
   * notice then says what stayed and where.
   */
  const removeSessionWorktree = useCallback((worktree: Worktree) => {
    const tr = createTranslator(settingsRef.current.language);
    api
      .removeWorktree(worktree)
      .then((value) => {
        const removal = parseRemoval(value);
        const notice =
          removal === null
            ? removalErrorNotice(new Error(tr("error.unknown")), worktree, tr)
            : removalNotice(removal, worktree, tr);
        if (notice) setWorktreeNotice({ ...notice, copied: false });
      })
      .catch((e: unknown) => {
        setWorktreeNotice({ ...removalErrorNotice(e, worktree, tr), copied: false });
      });
  }, []);

  const close = useCallback(
    async (id: number) => {
      const worktree = stateRef.current.sessions.find((s) => s.id === id)?.worktree;
      let closed = true;
      if (!isPlaceholder(id)) {
        try {
          await api.close(id);
        } catch (e) {
          closed = false;
          showError(e);
        }
      }
      // Remove locally even on error: the backend no longer knows the session anyway.
      const terminal = terminals.get(id);
      if (terminal) {
        // Closed means: it does not come back after a restart either.
        api.deleteScrollback(terminal.key).catch(() => undefined);
        terminal.dispose();
      }
      terminals.delete(id);
      dispatch({ type: "removed", id });
      forgetAttention(id);
      setRenamingId((current) => (current === id ? null : current));
      if (worktree) {
        // The process may still run in it (close failed), or another session works there: git
        // would delete files under a running program, so the worktree stays.
        const tr = createTranslator(settingsRef.current.language);
        if (!closed) {
          setWorktreeNotice({
            message: tr("worktree.keptOpen"),
            path: worktree.path,
            copied: false,
          });
        } else if (worktreeInUse(worktree, stateRef.current.sessions, id)) {
          setWorktreeNotice({
            message: tr("worktree.keptInUse"),
            path: worktree.path,
            copied: false,
          });
        } else {
          removeSessionWorktree(worktree);
        }
      }
    },
    [showError, forgetAttention, removeSessionWorktree],
  );

  /**
   * Close with confirmation while the process runs; exited sessions and placeholders close
   * immediately. Applies to the sidebar, the menu and Ctrl+Shift+W.
   */
  const requestClose = useCallback(
    (id: number) => {
      const session = stateRef.current.sessions.find((s) => s.id === id);
      if (!session) return;
      if (session.status === "exited" || isPlaceholder(id)) void close(id);
      else setClosingId(id);
    },
    [close],
  );

  const rename = useCallback(
    async (id: number, title: string) => {
      if (isPlaceholder(id)) {
        const session = stateRef.current.sessions.find((s) => s.id === id);
        if (session) dispatch({ type: "updated", session: { ...session, title } });
        return;
      }
      try {
        dispatch({ type: "updated", session: await api.rename(id, title) });
      } catch (e) {
        showError(e);
      }
    },
    [showError],
  );

  // Save workspace and scrollback (docs/wiki/pages/syntheses/session-restore.md).

  const saveWorkspaceNow = useCallback(async () => {
    if (!savingAllowed.current || !settingsRef.current.restoreSessions) return;
    const current = stateRef.current;
    const entries = current.sessions.flatMap((session) => {
      const terminal = terminals.get(session.id);
      return terminal
        ? [
            {
              key: terminal.key,
              profileId: session.profileId,
              cwd: session.cwd,
              title: session.title,
              kind: session.kind,
              color: session.marker?.color ?? null,
              emoji: session.marker?.emoji ?? null,
              worktree: session.worktree ?? null,
              cliSessionId: session.cliSessionId ?? null,
            },
          ]
        : [];
    });
    const activeKey =
      current.activeId === null ? null : (terminals.get(current.activeId)?.key ?? null);
    const workspace = mergeWorkspace(
      buildWorkspace(entries, activeKey),
      rememberedWorkspace.current,
    );
    const last = lastSavedWorkspace.current;
    if (last !== null && sameWorkspace(last, workspace)) return;
    await api.saveWorkspace(workspace);
    lastSavedWorkspace.current = workspace;
  }, []);

  /**
   * Only sessions with new output; an error leaves the session pending for the next attempt.
   * Periodic (`force` off): only sessions that have been quiet for SCROLLBACK_QUIET_MS, and at most
   * one per idle tick, because serializing a session can take tens of milliseconds.
   * On quit (`force`): all changed ones right away, the window is going away anyway.
   */
  const saveScrollbacks = useCallback(async (force: boolean) => {
    const current = settingsRef.current;
    if (!current.restoreSessions) return;
    const save = (terminal: SessionTerminal) => {
      terminal.dirty = false;
      const data = terminal.serialize(current.scrollbackLines);
      return api.saveScrollback(terminal.key, data).catch(() => {
        terminal.dirty = true;
      });
    };
    // Update sessions do not come back; their scrollback would be an orphaned file.
    const ids = stateRef.current.sessions.filter(restorable).map((session) => session.id);
    if (force) {
      const jobs = ids.flatMap((id) => {
        const terminal = terminals.get(id);
        return terminal && scrollbackDue(terminal, 0, 0, true) ? [save(terminal)] : [];
      });
      await Promise.all(jobs);
      return;
    }
    if (scrollbackSaving.current) return;
    scrollbackSaving.current = true;
    try {
      for (const id of ids) {
        // Wait first, then check: the session may have been closed in the meantime.
        await idle();
        const terminal = terminals.get(id);
        if (!terminal) continue;
        if (!scrollbackDue(terminal, performance.now(), SCROLLBACK_QUIET_MS, false)) continue;
        await save(terminal);
      }
    } finally {
      scrollbackSaving.current = false;
    }
  }, []);

  /** Save everything that would be lost on quit; waits at most QUIT_SAVE_TIMEOUT_MS. */
  const persistAll = useCallback(async () => {
    // The notes window saves its pending input too and says so (or the timeout ends the wait).
    // A window that is gone (crash) cannot answer, so the quit does not wait for it.
    const notesWindowSaved = loadWindowPrefs().detached
      ? Window.getByLabel(NOTES_WINDOW_LABEL)
          .then((found) =>
            found === null
              ? undefined
              : new Promise<void>((resolve) => {
                  notesSavedRef.current = resolve;
                  emitTo(NOTES_WINDOW_LABEL, NOTES_COMMAND, { kind: "flush" }).catch(() => {
                    resolve();
                  });
                }),
          )
          .catch(() => undefined)
      : Promise.resolve();
    const work = Promise.all([
      notesWindowSaved,
      flushNotes().catch(() => undefined),
      saveWorkspaceNow().catch(() => undefined),
      saveScrollbacks(true).catch(() => undefined),
    ]);
    await Promise.race([
      work,
      new Promise((resolve) => window.setTimeout(resolve, QUIT_SAVE_TIMEOUT_MS)),
    ]);
  }, [saveWorkspaceNow, saveScrollbacks]);

  /**
   * Without "Remember sessions" nothing brings worktree sessions back, so their worktrees would be
   * left behind: end those sessions, then remove each worktree (never forced; changes stay).
   */
  const removeOrphanedWorktrees = useCallback(async () => {
    const orphaned = orphanedWorktrees(
      stateRef.current.sessions,
      settingsRef.current.restoreSessions,
    );
    await Promise.allSettled(
      orphaned.map(async ({ ids, worktree }) => {
        // A failed close rejects here, so the worktree of a still running program stays.
        await Promise.all(ids.filter((id) => !isPlaceholder(id)).map((id) => api.close(id)));
        await api.removeWorktree(worktree);
      }),
    );
  }, []);

  const quit = useCallback(async () => {
    if (quitting.current) return;
    quitting.current = true;
    try {
      await persistAll();
      await Promise.race([removeOrphanedWorktrees(), delay(QUIT_SAVE_TIMEOUT_MS)]);
    } finally {
      // The waiting count must not stay on a Dock or launcher icon; bounded, quitting comes first.
      await Promise.race([badge.clear(), delay(BADGE_CLEAR_TIMEOUT_MS)]);
      try {
        await exit(0);
      } catch (e) {
        // Otherwise `quitting` would stay set and the window could never be closed again.
        quitting.current = false;
        showError(e);
      }
    }
  }, [persistAll, removeOrphanedWorktrees, showError, badge]);

  /** Setting "Remember sessions" off: discard what was saved so nothing comes back. */
  const forgetWorkspace = useCallback(() => {
    lastSavedWorkspace.current = null;
    rememberedWorkspace.current = null;
    api.saveWorkspace(EMPTY_WORKSPACE).catch(() => undefined);
    api.clearScrollbacks().catch(() => undefined);
  }, []);

  const clearScrollbacks = useCallback(async () => {
    try {
      await api.clearScrollbacks();
    } catch (e) {
      throw new Error(errorText(e), { cause: e });
    }
  }, []);

  // Restore on startup: only if the backend has no sessions yet. Otherwise only the UI was
  // reloaded (development, WebView restart) and the old processes are still running: then a
  // hint appears, and saving merges with the saved state.
  useEffect(() => {
    if (!profilesReady || restoreStarted) return;
    restoreStarted = true;
    /**
     * `false`: after a reload the saved state could not be read. Saving then stays off
     * (with a visible error message), because it would otherwise overwrite that state.
     */
    const run = async (): Promise<boolean> => {
      const running = await api.listSessions();
      if (running.length > 0) {
        setOrphans(backgroundSessions(running, new Set()));
        if (!settingsRef.current.restoreSessions) return true;
        try {
          rememberedWorkspace.current = parseWorkspace(await api.loadWorkspace());
        } catch (e) {
          showError(e);
          return false;
        }
        return true;
      }
      if (!settingsRef.current.restoreSessions) return true;
      const workspace = parseWorkspace(await api.loadWorkspace());
      if (!workspace || workspace.sessions.length === 0) return true;
      const language = settingsRef.current.language;
      const banner = createTranslator(language)("terminal.restored", {
        time: formatShortTime(new Date(), language),
      });
      // The active session first so it is usable right away; the others afterwards in parallel and
      // slightly staggered, instead of each waiting for the previous one to start.
      const active = workspace.sessions.find((saved) => saved.key === workspace.activeKey);
      if (active) {
        const id = await restoreSession(active, banner);
        dispatch({ type: "activated", id });
      }
      const rest = workspace.sessions.filter((saved) => saved !== active);
      await Promise.allSettled(
        rest.map(async (saved, index) => {
          await delay(index * RESTORE_STAGGER_MS);
          await restoreSession(saved, banner);
        }),
      );
      return true;
    };
    void run()
      .catch((e: unknown) => {
        showError(e);
        return true;
      })
      .then((allowed) => {
        setStartupDone(true);
        if (!allowed) return;
        savingAllowed.current = true;
        setRestored(true);
      });
  }, [profilesReady, restoreSession, showError]);

  useEffect(() => {
    if (!restored || !settings.restoreSessions) return;
    const timer = window.setTimeout(() => {
      saveWorkspaceNow().catch(() => undefined);
    }, WORKSPACE_SAVE_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [state.sessions, state.activeId, restored, settings.restoreSessions, saveWorkspaceNow]);

  useEffect(() => {
    if (!restored || !settings.restoreSessions) return;
    const interval = window.setInterval(() => {
      saveScrollbacks(false).catch(() => undefined);
    }, SCROLLBACK_SAVE_INTERVAL_MS);
    return () => {
      window.clearInterval(interval);
    };
  }, [restored, settings.restoreSessions, saveScrollbacks]);

  // Closing the window: save first, then quit. Without preventDefault the API would destroy the
  // window itself; quitting is handled here by `exit`, which also shuts down all sessions cleanly.
  // With the notification area icon, the backend hides the window instead and nothing quits.
  const quitRef = useRef(quit);
  useEffect(() => {
    quitRef.current = quit;
  });
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    try {
      getCurrentWindow()
        .onCloseRequested(async (event) => {
          event.preventDefault();
          if (tray.active) return;
          await quitRef.current();
        })
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there is no window event.
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tray]);

  // Events from the backend: a click on a desktop notification brings its session to the front
  // (the backend has already shown and focused the window); the notification area's Quit quits
  // the usual way.
  const activateRef = useRef<(id: number) => void>(() => undefined);
  useEffect(() => {
    activateRef.current = (id) => {
      if (!stateRef.current.sessions.some((s) => s.id === id)) return;
      dispatch({ type: "activated", id });
      focusTerminal();
    };
  });
  // The start hint is a short confirmation; a good update result fades on its own, a problem
  // stays until dismissed.
  useEffect(() => {
    if (startHint === null) return;
    const timer = window.setTimeout(() => {
      setStartHint(null);
    }, START_HINT_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [startHint]);
  useEffect(() => {
    if (updateResult?.tone !== "ok") return;
    const timer = window.setTimeout(() => {
      setUpdateResult(null);
    }, UPDATE_RESULT_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [updateResult]);

  /** After an update session: a calm notice with the outcome, and fresh versions. */
  /** Notes window events: the current handlers, for the listener attached once below. */
  const notesChangedRef = useRef<() => void>(() => undefined);
  const sendFromNotesRef = useRef<(text: string, sessionId: number | null) => void>(
    () => undefined,
  );
  /** Resolves the wait on quit once the notes window reports that it saved. */
  const notesSavedRef = useRef<(() => void) | null>(null);
  const updateResultRef = useRef<(outcome: UpdateOutcome) => void>(() => undefined);
  useEffect(() => {
    updateResultRef.current = (outcome) => {
      const name =
        profilesRef.current.find((p) => p.id === outcome.profileId)?.name ?? outcome.profileId;
      setUpdateResult({
        ...outcomeNotice(outcome, name, t),
        sessionId: outcome.sessionId,
        key: Date.now(),
      });
      afterUpdateRef.current();
    };
  });
  useEffect(() => {
    const stops: (() => void)[] = [];
    let disposed = false;
    const keep = (stop: () => void) => {
      if (disposed) stop();
      else stops.push(stop);
    };
    try {
      listen(NOTIFICATION_ACTIVATED, (event) => {
        const id = parseActivatedSession(event.payload);
        if (id !== null) activateRef.current(id);
      })
        .then(keep)
        .catch(() => undefined);
      listen(TRAY_QUIT, () => void quitRef.current())
        .then(keep)
        .catch(() => undefined);
      listen(NOTES_MESSAGE, (event) => {
        // Checked by the backend (notes_to_main) and here again before anything happens.
        const message = parseNotesMessage(event.payload);
        if (message?.kind === "sendToTerminal") {
          sendFromNotesRef.current(message.text, message.sessionId);
        } else if (message?.kind === "notesChanged") {
          notesSavedRef.current?.();
          notesSavedRef.current = null;
          notesChangedRef.current();
        }
      })
        .then(keep)
        .catch(() => undefined);
      listen(PROGRAM_UPDATE_RESULT, (event) => {
        const outcome = parseUpdateOutcome(event.payload);
        if (outcome !== null) updateResultRef.current(outcome);
      })
        .then(keep)
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there are no backend events.
    }
    return () => {
      disposed = true;
      for (const stop of stops) stop();
    };
  }, []);

  // Files dragged from the file manager onto a pane: their quoted paths go into that session.
  const dragRef = useRef<(event: DragDropEvent) => void>(() => undefined);
  useEffect(() => {
    dragRef.current = (event) => {
      if (event.type === "leave") {
        setDropTarget(null);
        return;
      }
      // Tauri reports physical pixels relative to the webview; the DOM works in CSS pixels.
      const ratio = window.devicePixelRatio || 1;
      const id = modalRef.current
        ? null
        : paneAt(event.position.x / ratio, event.position.y / ratio);
      const session = id === null ? undefined : stateRef.current.sessions.find((s) => s.id === id);
      const target = session && session.status !== "exited" ? session : undefined;
      if (event.type !== "drop") {
        setDropTarget(target?.id ?? null);
        return;
      }
      setDropTarget(null);
      const terminal = target ? terminals.get(target.id) : undefined;
      if (!target || !terminal) return;
      const profile = profilesRef.current.find((p) => p.id === target.profileId);
      const text = dropText(event.paths, shellKind(profile?.command ?? ""));
      if (text === "") return;
      terminal.pasteText(text);
      if (target.id !== activeIdRef.current) dispatch({ type: "activated", id: target.id });
      focusTerminal();
    };
  });
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          dragRef.current(event.payload);
        })
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there are no drag and drop events.
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Notes: scopes for the sidebar.
  const refreshNoteFolders = useCallback(() => {
    api
      .listNoteFolders()
      .then((value) => {
        setNoteFolders(parseNoteFolders(value));
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refreshNoteFolders();
  }, [refreshNoteFolders]);

  useEffect(() => {
    notesChangedRef.current = () => {
      refreshNoteFolders();
      setNotesRefresh((value) => value + 1);
    };
  });

  /** Notes changed here: the notes window reloads too. */
  const notesChangedHere = useCallback(() => {
    refreshNoteFolders();
    if (loadWindowPrefs().detached) {
      emitTo(NOTES_WINDOW_LABEL, NOTES_COMMAND, { kind: "refresh" }).catch(() => undefined);
    }
  }, [refreshNoteFolders]);

  // The notes window reads the sessions from here: "This project" and "Send to …".
  const activeCwd = activeSession?.cwd ?? null;
  useEffect(() => {
    saveContext({
      sessionFolder: activeCwd,
      activeId: state.activeId,
      sessions: sendTargets,
    });
  }, [activeCwd, state.activeId, sendTargets]);

  // Closing the notes window (its own close button) brings the notes back here.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === WINDOW_PREFS_KEY) setNotesWindow(loadWindowPrefs());
    };
    // A window that closed without telling (crash): the notes are docked again.
    const onFocus = () => {
      if (!loadWindowPrefs().detached) return;
      Window.getByLabel(NOTES_WINDOW_LABEL)
        .then((found) => {
          if (found === null) {
            const next = { ...loadWindowPrefs(), detached: false };
            saveWindowPrefs(next);
            setNotesWindow(next);
          }
        })
        .catch(() => undefined);
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  // Detached last time: the notes window comes back with the app.
  useEffect(() => {
    const prefs = loadWindowPrefs();
    if (prefs.detached) api.openNotesWindow(prefs.onTop).catch(() => undefined);
  }, []);

  /** A folder from the menu or the dialog: it applies to the next start (startTarget.ts). */
  const selectCwd = useCallback((path: string) => {
    setChosenCwd(path);
  }, []);

  const pickCwd = useCallback(async () => {
    try {
      const current = targetRef.current;
      const picked = await open(
        current ? { directory: true, defaultPath: current } : { directory: true },
      );
      if (typeof picked === "string") setChosenCwd(picked);
    } catch (e) {
      showError(e);
    }
  }, [showError]);

  const changeFont = (delta: number | null) => {
    setLayout((current) => ({
      ...current,
      fontSize: delta === null ? FONT_SIZE.default : clampFontSize(current.fontSize + delta),
    }));
  };

  const toggleSidebar = () => {
    setLayout((current) => ({ ...current, collapsed: !current.collapsed }));
  };

  const askNotes = (make: (token: number) => NotesRequest) => {
    notesTokens.current += 1;
    setNotesRequest(make(notesTokens.current));
  };

  /** Command for the detached notes window; the window opens (or comes to the front) first. */
  const toNotesWindow = (command: NotesCommand | null) => {
    api
      .openNotesWindow(notesWindow.onTop)
      .then(() => (command ? emitTo(NOTES_WINDOW_LABEL, NOTES_COMMAND, command) : undefined))
      .catch(showError);
  };

  const openSide = (tab: SideTab) => {
    setLayout((current) =>
      current.sideOpen && current.sideTab === tab
        ? current
        : { ...current, sideOpen: true, sideTab: tab },
    );
  };

  const closeSide = () => {
    setLayout((current) => (current.sideOpen ? { ...current, sideOpen: false } : current));
    focusTerminal();
  };

  /** Is the focus in the notes of the right panel right now? */
  const focusInNotes = () =>
    document.activeElement?.closest("#side-body-notes, .side-tab") != null &&
    layout.sideTab === "notes";

  /**
   * Ctrl+Shift+N: a new quick note, wherever you are. Pressed again inside the notes, it closes
   * them and the terminal gets the focus back.
   */
  const quickNote = () => {
    if (notesWindow.detached) {
      toNotesWindow({ kind: "quickNote" });
      return;
    }
    if (layout.sideOpen && focusInNotes()) {
      closeSide();
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "quick" }));
  };

  /** Show or hide the notes (menu, sidebar button): opening focuses the list or open note. */
  const toggleNotes = () => {
    if (notesWindow.detached) {
      toNotesWindow(null);
      return;
    }
    if (layout.sideOpen && layout.sideTab === "notes") {
      closeSide();
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "focus" }));
  };

  const toggleChanges = () => {
    if (layout.sideOpen && layout.sideTab === "changes") {
      closeSide();
      return;
    }
    openSide("changes");
    setChangesFocusToken((token) => token + 1);
  };

  /** Quick access "Notes in …": that project's notes. */
  const openNotes = (folder: NoteFolder) => {
    if (notesWindow.detached) {
      toNotesWindow(null);
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "openFolder", folder }));
  };

  const updateNotesWindow = (next: NotesWindowPrefs) => {
    setNotesWindow(next);
    saveWindowPrefs(next);
  };

  /** "Open in own window": the notes move into a window of their own. */
  const detachNotes = () => {
    void flushNotes().then(() => {
      updateNotesWindow({ ...notesWindow, detached: true });
      setLayout((current) => ({ ...current, sideOpen: false }));
      toNotesWindow(null);
    });
  };

  /** "Bring back": the notes window closes, the notes are in the right panel again. */
  const bringBackNotes = () => {
    updateNotesWindow({ ...notesWindow, detached: false });
    emitTo(NOTES_WINDOW_LABEL, NOTES_COMMAND, { kind: "close" }).catch(() => undefined);
    openSide("notes");
    askNotes((token) => ({ token, kind: "focus" }));
  };

  /** Text from a note into a session, as pasted (bracketed paste, no Enter); that session shows. */
  const sendFromNotes = (text: string, sessionId: number | null) => {
    const id = sessionId ?? activeIdRef.current;
    if (id === null) return;
    if (!terminals.has(id)) return;
    // That session shows first, so a warning is read next to the session it is about.
    if (id !== activeIdRef.current) dispatch({ type: "activated", id });
    pasteIntoSession(id, text);
  };

  useEffect(() => {
    sendFromNotesRef.current = sendFromNotes;
  });

  /** "Add selection to note": into the open note, otherwise a new quick note. */
  const captureSelection = () => {
    const terminal = activeTerminal();
    const text = terminal?.selectionText() ?? "";
    if (text.trim() === "") return;
    const source = activeSession?.title ?? "";
    if (notesWindow.detached) {
      toNotesWindow({ kind: "capture", text, source });
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "capture", text, source }));
  };

  /**
   * F6 and Shift+F6 while the notes are open: terminal, list, editor. Returns whether it moved
   * the focus (otherwise F6 stays with the program in the terminal).
   */
  const cycleFocus = (step: 1 | -1, fromTerminal: boolean): boolean => {
    const panel = notesHandle.current;
    if (!layout.sideOpen || layout.sideTab !== "notes" || notesWindow.detached || !panel) {
      return false;
    }
    const zones = ["terminal" as const, ...panel.zones()];
    const zone = nextFocusZone(zones, fromTerminal ? "terminal" : panel.zone(), step);
    if (zone === "terminal" || zone === null) focusTerminal();
    else panel.focusZone(zone);
    return true;
  };

  /** Inline rename in the sidebar; a collapsed sidebar expands for it. */
  const startRename = (id: number) => {
    setLayout((current) => (current.collapsed ? { ...current, collapsed: false } : current));
    setRenamingId(id);
  };

  const closeDialog = () => {
    setDialog(null);
    focusActive();
  };

  const cancelClose = () => {
    setClosingId(null);
    focusActive();
  };

  const confirmClose = () => {
    const id = closingId;
    setClosingId(null);
    if (id !== null) void close(id);
    focusActive();
  };

  /** End background sessions after a reload; their saved state is kept. */
  const endOrphans = () => {
    const ids = orphans.map((session) => session.id);
    setOrphans([]);
    void Promise.allSettled(ids.map((id) => api.close(id))).then((results) => {
      const failed = results.find((result) => result.status === "rejected");
      if (failed) showError(failed.reason);
    });
    focusActive();
  };

  const copyLink = () => {
    if (link === null) return;
    const url = link.url;
    writeClipboard(url)
      .then(() => {
        setLink((current) => (current?.url === url ? { url, copied: true } : current));
      })
      .catch(showError);
  };

  const copyWorktreePath = () => {
    if (worktreeNotice === null) return;
    const path = worktreeNotice.path;
    writeClipboard(path)
      .then(() => {
        setWorktreeNotice((current) =>
          current?.path === path ? { ...current, copied: true } : current,
        );
      })
      .catch(showError);
  };

  /** A profile was saved or deleted in Settings: the backend's new list applies at once. */
  const applyProfiles = (next: Profile[]) => {
    setProfiles((current) => (sameProfiles(current, next) ? current : next));
  };

  const updateSettings = (patch: Partial<Settings>) => {
    if (patch.restoreSessions === false && settings.restoreSessions) forgetWorkspace();
    setSettings((current) => ({ ...current, ...patch }));
  };

  /** Toggle in the sidebar; if the setting is System, it becomes a fixed choice. */
  const toggleTheme = () => {
    updateSettings({ theme: toggledTheme(theme) });
  };

  const keys = (action: AppAction) => formatShortcut(SHORTCUT_LABELS[action], t);

  /** Opens the search of the visible session, or focuses its field again. */
  const openSearch = () => {
    const id = state.activeId;
    if (id === null || !terminals.has(id)) return;
    setSearch((current) => ({
      sessionId: id,
      token: current?.sessionId === id ? current.token + 1 : 0,
    }));
  };

  const closeSearch = () => {
    setSearch(null);
    focusTerminal();
  };

  /**
   * Saves the visible session as text or HTML. The backend asks for the place in its save dialog;
   * a cancelled dialog is no error.
   */
  const exportSession = (format: ExportFormat) => {
    const session = activeSession;
    const terminal = activeTerminal();
    if (!session || !terminal) return;
    const content =
      format === "html"
        ? terminal.exportHtml(session.title)
        : terminal.exportText(exportEol(navigator.userAgent));
    if (utf8Bytes(content) > MAX_EXPORT_BYTES) {
      setError(t("export.tooLarge"));
      return;
    }
    api
      .exportSession(format, content, exportFileName(session.title, new Date(), format))
      .then(parseExportResult)
      .catch(showError)
      .finally(focusActive);
  };
  /** Snippet into the visible session, like pasting: bracketed paste, no Enter. */
  const insertSnippet = (snippet: Snippet) => {
    const session = activeSession;
    const terminal = activeTerminal();
    if (!session || session.status === "exited" || !terminal) return;
    pasteIntoSession(session.id, snippet.text);
  };

  /**
   * Text to several sessions: each gets it as a bracketed paste, then Enter as a separate write.
   * Afterwards the split view shows up to four of them.
   */
  const sendToSeveral = (text: string, ids: number[]) => {
    setDialog(null);
    const targets = ids.flatMap((id) => {
      const terminal = terminals.get(id);
      return terminal ? [terminal] : [];
    });
    void sendToTargets(targets, text, () => delay(BROADCAST_ENTER_DELAY_MS));
    splitView.showOnly(ids);
  };

  const setMarker = (id: number, marker: Marker) => {
    dispatch({ type: "marked", id, marker });
  };

  const closeMarker = () => {
    setMarkerId(null);
    focusActive();
  };

  const noActive = activeSession === undefined;
  const startable = startableProfiles(profiles, settings.programs);
  const allHidden =
    startable.length === 0 && arrangedProfiles(profiles, settings.programs).length > 0;
  const nextProgram = pickProgram(startable, lastProgramId);
  const waiting = state.sessions.filter((s) => attention.get(s.id) === "attention");
  const waitingCount = waiting.length;

  // Taskbar badge with the number of waiting sessions, in the attention color.
  useEffect(() => {
    badge.update(waitingCount, settings.attentionColor);
  }, [badge, waitingCount, settings.attentionColor]);

  // Notification area icon: added, relabeled (language, waiting count) or removed. Nothing is
  // sent while the setting was never on.
  const trayUsed = useRef(false);
  useEffect(() => {
    if (!settings.keepInTray && !trayUsed.current) return;
    trayUsed.current = settings.keepInTray;
    void tray.configure(settings.keepInTray, trayLabels(t, waitingCount));
  }, [tray, settings.keepInTray, t, waitingCount]);

  /** Times for the status line; read from the terminals, which collect them outside React. */
  const activityOf = (id: number, now: number) => terminals.get(id)?.activity(now) ?? NO_ACTIVITY;
  /** Folders to start in: the target, running projects, favorites and recent folders. */
  const places = startPlaces(targetPath, running, folders.favorites, folders.recent);
  /**
   * Working folders whose git branch the status line shows (first, so they always fit), then the
   * places to start in: a branch there means an own worktree is possible.
   */
  const branches = useGitBranches(
    branchPaths([...state.sessions, ...places.map((path) => ({ cwd: path }))]),
  );
  const worktree = worktreeChance(branches, targetPath);
  /** Start in the target; `ownWorktree` only where git allows it. */
  const startHere = (profile: Profile, ownWorktree = false) => {
    void start(profile, undefined, ownWorktree && worktree === "yes");
  };
  /** Programs that are running right now: on Windows their update can fail on locked files. */
  const runningPrograms = new Set(
    state.sessions
      .filter((s) => s.kind !== "update" && s.status !== "exited")
      .map((s) => s.profileId),
  );
  /** Programs whose update session runs right now. */
  const updatingPrograms = new Set(
    state.sessions
      .filter((s) => s.kind === "update" && s.status !== "exited")
      .map((s) => s.profileId),
  );
  /**
   * "Update" on a program: one click, unless the program runs; then the row asks first whether
   * to close its sessions or update anyway (locked files on Windows). From the rail the sidebar
   * opens so the question is visible where the program is.
   */
  const updateActions: UpdateActions = {
    updating: updatingPrograms,
    confirming: confirmUpdate,
    request: (profileId) => {
      setPlanChanged(null);
      if (runningPrograms.has(profileId) && !updatingPrograms.has(profileId)) {
        setConfirmUpdate(profileId);
        setLayout((current) =>
          current.collapsed || current.closedGroups.includes("newSession")
            ? {
                ...current,
                collapsed: false,
                closedGroups: current.closedGroups.filter((group) => group !== "newSession"),
              }
            : current,
        );
        return;
      }
      setConfirmUpdate(null);
      void startUpdate(profileId);
    },
    confirm: (profileId) => {
      setConfirmUpdate(null);
      void startUpdate(profileId);
    },
    cancel: () => {
      setConfirmUpdate(null);
    },
  };

  /** Running sessions that can receive text from "Send to several". */
  const broadcastable = broadcastCandidates(state.sessions);
  const snippets = usableSnippets(settings.snippets);
  /** Snippets can go into the visible session while its process runs. */
  const canInsert = activeSession !== undefined && activeSession.status !== "exited";

  const perform = (action: AppAction) => {
    switch (action) {
      case "nextSession":
        dispatch({ type: "cycled", step: 1 });
        break;
      case "prevSession":
        dispatch({ type: "cycled", step: -1 });
        break;
      case "toggleSidebar":
        toggleSidebar();
        break;
      case "quickNote":
        quickNote();
        break;
      case "toggleChanges":
        toggleChanges();
        break;
      case "closeSession":
        if (state.activeId !== null) requestClose(state.activeId);
        break;
      case "fontBigger":
        changeFont(1);
        break;
      case "fontSmaller":
        changeFont(-1);
        break;
      case "fontReset":
        changeFont(null);
        break;
      case "openSettings":
        setDialog("settings");
        break;
      case "nextWaiting": {
        const id = nextWaitingId(
          state.sessions,
          state.activeId,
          (sessionId) => attention.get(sessionId) === "attention",
        );
        if (id !== null && id !== state.activeId) dispatch({ type: "activated", id });
        else if (id !== null) focusTerminal();
        break;
      }
      case "newSession":
        if (nextProgram) {
          startHere(nextProgram);
          // The shortcut shows no folder: say briefly where it starts.
          setStartHint(
            t("start.started", {
              name: nextProgram.name,
              folder: targetPath === null ? "" : folderName(targetPath),
            }),
          );
        }
        break;
      case "find":
        openSearch();
        break;
      case "launcher":
        setDialog("launcher");
        break;
    }
  };

  /**
   * Right click or context menu key in the terminal. Via keyboard (no mouse position) the
   * menu appears at the top left of the terminal.
   */
  const openTerminalMenu = (event: MouseEvent) => {
    if (state.activeId === null) return;
    if (event.button === 2 || event.clientX !== 0 || event.clientY !== 0) {
      setTerminalMenu({ x: event.clientX, y: event.clientY });
      return;
    }
    const pane = document.querySelector(".pane.active")?.getBoundingClientRect();
    setTerminalMenu({ x: (pane?.left ?? 0) + 24, y: (pane?.top ?? 0) + 24 });
  };

  // The key listener is attached to the window once and always calls the current version.
  const performRef = useRef(perform);
  const openTerminalMenuRef = useRef(openTerminalMenu);
  const cycleFocusRef = useRef(cycleFocus);
  useEffect(() => {
    performRef.current = perform;
    openTerminalMenuRef.current = openTerminalMenu;
    cycleFocusRef.current = cycleFocus;
  });
  // Same condition as cycleFocus: while it holds, F6 in a terminal is for the app alone.
  const notesDocked = layout.sideOpen && layout.sideTab === "notes" && !notesWindow.detached;
  useEffect(() => {
    setFocusCycleActive(notesDocked);
  }, [notesDocked]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // Reloading disconnects the UI from all sessions. In the terminal, the keys belong to the shell.
      if (isReloadKey(event) && !inTerminal(event.target)) {
        event.preventDefault();
        return;
      }
      const step = focusCycleStep(event);
      if (step !== null && !modalRef.current) {
        if (cycleFocusRef.current(step, inTerminal(event.target))) event.preventDefault();
        return;
      }
      const action = matchShortcut(event);
      if (!action) return;
      // Held key: intercept it, but do not run it again (otherwise one session per repeat).
      if (event.repeat && ignoresRepeat(action)) {
        event.preventDefault();
        return;
      }
      // An open dialog is modal: switching sessions, closing and font size have no effect behind it.
      if (modalRef.current && action !== "openSettings") return;
      event.preventDefault();
      performRef.current(action);
    };
    // The WebView's default menu offers "Reload" and "Inspect"; neither belongs in the
    // app. Input fields and the note editor keep their edit menu.
    const onContextMenu = (event: MouseEvent) => {
      if (editable(event.target) && !inTerminal(event.target)) return;
      event.preventDefault();
      if (inTerminal(event.target) && !modalRef.current) openTerminalMenuRef.current(event);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContextMenu);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContextMenu);
    };
  }, []);

  const profileById = new Map(profiles.map((profile) => [profile.id, profile]));
  const badgeFor = (profileId: string) =>
    resolveBadge(
      profileId,
      profileById.get(profileId),
      programs.icons.get(profileId),
      settings.programs,
    );
  const openWhatsNew = () => {
    setWhatsNewSince(null);
    setDialog("whatsNew");
  };
  const menu: MenuGroup[] = [
    {
      label: t("menu.file"),
      items: [
        {
          type: "submenu",
          label: t("menu.newSession"),
          disabled: startable.length === 0,
          items: startable.map((profile) => {
            const version = programs.versions.get(profile.id);
            const latest = availableVersion(programUpdates.updates, profile.id);
            const detail =
              version === undefined
                ? undefined
                : latest === null
                  ? version
                  : `${version} → ${latest}`;
            return {
              type: "action",
              label: profile.name,
              ...(detail === undefined ? {} : { detail }),
              run: () => {
                startHere(profile);
              },
            };
          }),
        },
        {
          type: "action",
          label: nextProgram
            ? t("menu.newSessionWith", { name: nextProgram.name })
            : t("menu.newSession"),
          shortcut: keys("newSession"),
          disabled: nextProgram === undefined,
          run: () => {
            perform("newSession");
          },
        },
        { type: "action", label: t("menu.pickCwd"), run: () => void pickCwd() },
        {
          type: "action",
          label: t("menu.launcher"),
          shortcut: keys("launcher"),
          run: () => {
            perform("launcher");
          },
        },
        {
          type: "action",
          label: t("menu.broadcast"),
          disabled: broadcastable.length === 0,
          run: () => {
            setDialog("broadcast");
          },
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.renameSession"),
          disabled: noActive,
          run: () => {
            if (activeSession) startRename(activeSession.id);
          },
        },
        {
          type: "action",
          label: t("menu.marker"),
          disabled: noActive,
          run: () => {
            if (activeSession) setMarkerId(activeSession.id);
          },
        },
        {
          type: "action",
          label: t("menu.restartSession"),
          disabled: activeSession?.status !== "exited",
          run: () => {
            if (activeSession) void restart(activeSession.id);
          },
        },
        {
          type: "submenu",
          label: t("menu.exportSession"),
          disabled: noActive,
          items: [
            {
              type: "action",
              label: t("menu.exportText"),
              run: () => {
                exportSession("text");
              },
            },
            {
              type: "action",
              label: t("menu.exportHtml"),
              run: () => {
                exportSession("html");
              },
            },
          ],
        },
        {
          type: "action",
          label: t("menu.killProcess"),
          disabled: noActive || activeSession.status === "exited",
          run: () => {
            if (activeSession) kill(activeSession.id);
          },
        },
        {
          type: "action",
          label: t("menu.closeSession"),
          shortcut: keys("closeSession"),
          disabled: noActive,
          run: () => {
            perform("closeSession");
          },
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.settings"),
          shortcut: keys("openSettings"),
          run: () => {
            setDialog("settings");
          },
        },
        { type: "separator" },
        { type: "action", label: t("menu.quit"), run: () => void quit() },
      ],
    },
    {
      label: t("menu.edit"),
      items: [
        {
          type: "action",
          label: t("menu.copy"),
          shortcut: formatShortcut(COPY_KEYS, t),
          disabled: noActive,
          run: () => activeTerminal()?.copySelection(),
        },
        {
          type: "action",
          label: t("menu.paste"),
          shortcut: formatShortcut(PASTE_KEYS, t),
          disabled: noActive,
          run: () => activeTerminal()?.pasteClipboard(),
        },
        {
          type: "action",
          label: t("menu.selectAll"),
          disabled: noActive,
          run: () => activeTerminal()?.selectAll(),
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.find"),
          shortcut: keys("find"),
          disabled: noActive,
          run: openSearch,
        },
        {
          type: "action",
          label: t("menu.clear"),
          disabled: noActive,
          run: () => activeTerminal()?.clear(),
        },
      ],
    },
    {
      label: t("menu.view"),
      items: [
        {
          type: "action",
          label: layout.collapsed ? t("sidebar.expand") : t("sidebar.collapse"),
          shortcut: keys("toggleSidebar"),
          run: toggleSidebar,
        },
        {
          type: "action",
          label: t("notes.quickNoteMenu"),
          shortcut: keys("quickNote"),
          run: quickNote,
        },
        {
          type: "action",
          label: notesShown ? t("notes.hide") : t("notes.show"),
          run: toggleNotes,
        },
        {
          type: "action",
          label: changesShown ? t("changes.hide") : t("changes.show"),
          shortcut: keys("toggleChanges"),
          run: toggleChanges,
        },
        {
          type: "action",
          label: theme === "dark" ? t("sidebar.toLight") : t("sidebar.toDark"),
          run: toggleTheme,
        },
        { type: "separator" },
        ...LAYOUT_IDS.map((id): MenuItem => ({
          type: "action",
          label: t(LAYOUT_NAMES[id]),
          ...(id === layout.snapLayout ? { detail: t("folders.current") } : {}),
          run: () => {
            splitView.setSnapLayout(id);
          },
        })),
        {
          type: "action",
          label: t("snap.choose"),
          disabled: openSnapPicker === null,
          run: () => {
            openSnapPicker?.();
          },
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.nextSession"),
          shortcut: keys("nextSession"),
          disabled: state.sessions.length < 2,
          run: () => {
            perform("nextSession");
          },
        },
        {
          type: "action",
          label: t("menu.prevSession"),
          shortcut: keys("prevSession"),
          disabled: state.sessions.length < 2,
          run: () => {
            perform("prevSession");
          },
        },
        {
          type: "action",
          label: t("menu.nextWaiting"),
          shortcut: keys("nextWaiting"),
          disabled: waiting.length === 0,
          run: () => {
            perform("nextWaiting");
          },
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.fontBigger"),
          shortcut: keys("fontBigger"),
          disabled: layout.fontSize >= FONT_SIZE.max,
          run: () => {
            changeFont(1);
          },
        },
        {
          type: "action",
          label: t("menu.fontSmaller"),
          shortcut: keys("fontSmaller"),
          disabled: layout.fontSize <= FONT_SIZE.min,
          run: () => {
            changeFont(-1);
          },
        },
        {
          type: "action",
          label: t("menu.fontReset"),
          shortcut: keys("fontReset"),
          run: () => {
            changeFont(null);
          },
        },
        { type: "separator" },
        {
          type: "action",
          label: t("menu.resetLayout"),
          run: () => {
            setLayout(DEFAULT_LAYOUT);
          },
        },
      ],
    },
    {
      label: t("menu.help"),
      items: [
        {
          type: "action",
          label: t("menu.checkUpdates"),
          disabled: updater.state.kind === "installing" || updater.state.kind === "checking",
          run: () => void updater.check(true),
        },
        { type: "separator" },
        { type: "action", label: t("menu.whatsNew"), run: openWhatsNew },
        {
          type: "action",
          label: t("menu.about"),
          run: () => {
            setDialog("about");
          },
        },
      ],
    },
  ];

  const sidebarColumn = layout.collapsed ? SIDEBAR_RAIL_WIDTH : layout.sidebarWidth;
  const workspaceWidth = viewportWidth - sidebarColumn;
  const notesPlacement = placeNotes(layout.notesWidth, workspaceWidth, viewportWidth);
  const runningOrphans = orphans.filter((session) => session.status !== "exited").length;
  const closingSession =
    closingId === null ? undefined : state.sessions.find((s) => s.id === closingId);
  const quick = quickPrograms(startable);
  const searchTerminal = search === null ? undefined : terminals.get(search.sessionId);
  const terminalItems: PopupItem[] = [
    {
      type: "action",
      key: "copy",
      label: t("menu.copy"),
      shortcut: formatShortcut(COPY_KEYS, t),
      disabled: activeTerminal()?.hasSelection() !== true,
      run: () => activeTerminal()?.copySelection(),
    },
    {
      type: "action",
      key: "paste",
      label: t("menu.paste"),
      shortcut: formatShortcut(PASTE_KEYS, t),
      run: () => activeTerminal()?.pasteClipboard(),
    },
    {
      type: "action",
      key: "selectAll",
      label: t("menu.selectAll"),
      run: () => activeTerminal()?.selectAll(),
    },
    {
      type: "action",
      key: "toNote",
      label: t("notes.addSelection"),
      disabled: activeTerminal()?.hasSelection() !== true,
      run: captureSelection,
    },
    { type: "separator", key: "sep" },
    {
      type: "action",
      key: "find",
      label: t("menu.find"),
      shortcut: keys("find"),
      run: openSearch,
    },
    {
      type: "action",
      key: "clear",
      label: t("menu.clear"),
      run: () => activeTerminal()?.clear(),
    },
    ...(snippets.length > 0 && canInsert
      ? [
          { type: "separator", key: "sep-snippets" } as const,
          { type: "heading", key: "snippets", label: t("snippets.insert") } as const,
          ...snippets.slice(0, MENU_SNIPPETS).map((snippet): PopupItem => ({
            type: "action",
            key: `snippet:${snippet.id}`,
            label: snippetLabel(snippet),
            title: snippet.text,
            run: () => {
              insertSnippet(snippet);
            },
          })),
          ...(snippets.length > MENU_SNIPPETS
            ? [
                {
                  type: "action",
                  key: "snippets-more",
                  label: t("snippets.more"),
                  shortcut: keys("launcher"),
                  run: () => {
                    setDialog("launcher");
                  },
                } as const,
              ]
            : []),
        ]
      : []),
  ];

  const menuSession =
    sessionMenu === null ? undefined : state.sessions.find((s) => s.id === sessionMenu.id);
  const runSessionAction = (action: SessionAction, id: number) => {
    switch (action) {
      case "rename":
        startRename(id);
        break;
      case "marker":
        setMarkerId(id);
        break;
      case "subAgents":
        toggleSubAgents(id);
        break;
      case "restart":
        void restart(id);
        break;
      case "kill":
        kill(id);
        break;
      case "close":
        requestClose(id);
        break;
    }
  };
  const sessionItems: PopupItem[] =
    menuSession === undefined
      ? []
      : sessionMenuEntries(menuSession, t, subAgentsOf === menuSession.id).map(
          (entry): PopupItem => {
            if (entry.type === "separator") return entry;
            const action = entry.key;
            const id = menuSession.id;
            return {
              ...entry,
              run: () => {
                runSessionAction(action, id);
              },
            };
          },
        );

  const markerSession =
    markerId === null ? undefined : state.sessions.find((s) => s.id === markerId);

  /** Entries of quick access; built only while it is open. */
  const launcherItems = () =>
    buildLauncherItems({
      sessions: state.sessions,
      programName: (profileId) => profileById.get(profileId)?.name,
      startable,
      places: places.map((path) => ({ path, worktree: worktreeChance(branches, path) === "yes" })),
      noteFolders,
      snippets,
      canInsert,
      snapLayout: layout.snapLayout,
      canBroadcast: broadcastable.length > 0,
      changesOpen: changesShown,
      t,
      badge: (profileId) => <Badge look={badgeFor(profileId)} />,
      notesIcon: <NotesIcon size={14} />,
      activate: (id) => {
        dispatch({ type: "activated", id });
        focusTerminal();
      },
      start: (profile, folder) => void start(profile, folder),
      startInWorktree: (profile, folder) => void start(profile, folder, true),
      toggleChanges,
      openNotes,
      insertSnippet,
      setSnapLayout: splitView.setSnapLayout,
      openSnapPicker,
      openBroadcast: () => {
        setDialog("broadcast");
      },
      subAgents:
        activeSession !== undefined &&
        (activeSession.subAgents === true || subAgentsOf === activeSession.id)
          ? {
              shown: subAgentsOf === activeSession.id,
              toggle: () => {
                toggleSubAgents(activeSession.id);
              },
            }
          : null,
    });

  /** Pane header contents in split view: badge, attention state and status text. */
  const paneLook = (session: SessionView) => () => {
    const paneState = attention.get(session.id) ?? "idle";
    return {
      look: badgeFor(session.profileId),
      state: paneState,
      status: statusText(session, t, paneState, reasons.get(session.id) ?? null),
    };
  };

  return (
    <I18nContext.Provider value={t}>
      <div className="app" style={{ gridTemplateColumns: `${String(sidebarColumn)}px 1fr` }}>
        <Sidebar
          sessions={state.sessions}
          activeId={state.activeId}
          profiles={profiles}
          startable={startable}
          allHidden={allHidden}
          badgeFor={badgeFor}
          versions={programs.versions}
          updates={programUpdates.updates}
          notesOpen={notesShown}
          onToggleNotes={toggleNotes}
          closedGroups={layout.closedGroups}
          onToggleGroup={(group: SidebarGroup) => {
            setLayout((current) => toggleGroup(current, group));
          }}
          onFocusTerminal={focusTerminal}
          version={version}
          theme={theme}
          onToggleTheme={toggleTheme}
          target={target}
          runningProjects={running}
          worktree={worktree}
          collapsed={layout.collapsed}
          width={layout.sidebarWidth}
          footerHeight={layout.footerHeight}
          menu={menu}
          update={updater.state}
          attention={attention}
          reasons={reasons}
          activityOf={activityOf}
          branches={branches}
          overlaps={overlapMarkers}
          language={settings.language}
          renamingId={renamingId}
          onRenameStart={startRename}
          onRenameEnd={() => {
            setRenamingId(null);
            focusActive();
          }}
          onWidth={(width) => {
            setLayout((current) => ({ ...current, sidebarWidth: clampSidebarWidth(width) }));
          }}
          onFooterHeight={(footerHeight) => {
            setLayout((current) => ({ ...current, footerHeight }));
          }}
          onToggleCollapsed={toggleSidebar}
          onMenuClose={focusActive}
          onActivate={(id) => {
            dispatch({ type: "activated", id });
          }}
          onStart={startHere}
          updatingPrograms={updatingPrograms}
          confirmUpdate={confirmUpdate}
          onRequestUpdate={updateActions.request}
          onConfirmUpdate={updateActions.confirm}
          onCancelUpdate={updateActions.cancel}
          onPickCwd={() => {
            void pickCwd();
          }}
          folders={folders}
          onSelectCwd={selectCwd}
          onResetTarget={() => {
            setChosenCwd(null);
          }}
          onToggleFavorite={(path) => {
            setFolders((current) => toggleFavorite(current, path));
          }}
          onRestart={(id) => {
            void restart(id);
          }}
          onKill={kill}
          onClose={requestClose}
          onRename={(id, title) => {
            void rename(id, title);
          }}
          onSessionMenu={(id, x, y) => {
            setSessionMenu({ id, x, y });
          }}
          drag={
            sessionDragView === null
              ? null
              : {
                  id: sessionDragView.id,
                  line:
                    sessionDragView.target?.kind === "list"
                      ? dropLine(
                          state.sessions.map((s) => s.id),
                          sessionDragView.id,
                          sessionDragView.target.index,
                        )
                      : null,
                }
          }
          onDragStart={beginSessionDrag}
          onMove={moveSession}
          onInstallUpdate={() => {
            if (updater.state.kind !== "available") return;
            const update = updater.state.update;
            // The installer quits Hangar: save first so the sessions come back afterwards.
            void persistAll().then(() => updater.install(update));
          }}
          onDismissUpdate={updater.dismiss}
        />
        <main className="body">
          {error && (
            <div className="error" role="alert">
              <span>{error}</span>
              <button
                type="button"
                className="icon-button"
                aria-label={t("error.dismiss")}
                onClick={() => {
                  setError(null);
                }}
              >
                <CloseIcon />
              </button>
            </div>
          )}
          {runningOrphans > 0 && (
            <div className="orphans" role="status">
              <span className="orphans-text">
                <strong>
                  {runningOrphans === 1
                    ? t("orphans.one")
                    : t("orphans.many", { count: runningOrphans })}
                </strong>{" "}
                {t("orphans.help")}
              </span>
              <button type="button" className="secondary-button" onClick={endOrphans}>
                {t("orphans.end")}
              </button>
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setOrphans([]);
                  focusActive();
                }}
              >
                {t("update.later")}
              </button>
            </div>
          )}
          <div
            className={notesPlacement.mode === "overlay" ? "workspace notes-overlaid" : "workspace"}
          >
            <div className={state.sessions.length > 0 ? "panes filled" : "panes"} data-panes="">
              {state.sessions.map((session) => {
                const terminal = terminals.get(session.id);
                if (!terminal) return null;
                const active = session.id === state.activeId;
                // The sub-agent view shows only its session, in the main zone of its own layout;
                // the slots of the split view stay as they were for when it closes.
                const paneInfo =
                  subAgentPlan === null
                    ? splitView.paneSplit(session, paneLook(session))
                    : session.id === subAgentsOf
                      ? { layout: subAgentPlan.layout, slot: 0, session, ...paneLook(session)() }
                      : null;
                return (
                  <TerminalPane
                    key={session.id}
                    terminal={terminal}
                    sessionId={session.id}
                    active={active}
                    visible={active || paneInfo !== null}
                    split={paneInfo}
                    dropping={dropTarget === session.id}
                    onFocusPane={(id) => {
                      dispatch({ type: "activated", id });
                    }}
                    onMaximize={splitView.maximize}
                    onHeaderMenu={(id, x, y) => {
                      setSessionMenu({ id, x, y });
                    }}
                  >
                    {searchTerminal === terminal && search !== null && (
                      <SearchBar
                        key={search.sessionId}
                        terminal={searchTerminal}
                        focusToken={search.token}
                        onClose={closeSearch}
                      />
                    )}
                  </TerminalPane>
                );
              })}
              {subAgentPlan !== null && subAgentSession !== undefined && (
                <SubAgentPanes
                  title={subAgentSession.title}
                  plan={subAgentPlan}
                  view={subAgentWatch.view}
                  notice={subAgentNotice}
                  excerpts={settings.subAgentExcerpts}
                  onClose={closeSubAgents}
                />
              )}
              {state.sessions.length > 0 &&
                subAgentPlan === null &&
                splitView.split.slots.map((id, slot) =>
                  id === null && splitView.layout !== "single" ? (
                    <EmptyPane
                      key={`empty-${String(slot)}`}
                      layout={splitView.layout}
                      slot={slot}
                    />
                  ) : null,
                )}
              {sessionDragView?.overBody === true && (
                <>
                  {sessionDragView.target !== null && sessionDragView.target.kind !== "list" && (
                    <div
                      className="snap-preview"
                      style={
                        sessionDragView.target.kind === "snap"
                          ? paneStyle(sessionDragView.target.layout, sessionDragView.target.zone)
                          : paneStyle(splitView.layout, sessionDragView.target.zone)
                      }
                      aria-hidden="true"
                    >
                      <span className="snap-preview-label">{t("snap.showHere")}</span>
                    </div>
                  )}
                  <SnapBar
                    mode="drag"
                    current={splitView.layout}
                    highlight={
                      sessionDragView.target?.kind === "snap"
                        ? {
                            layout: sessionDragView.target.layout,
                            zone: sessionDragView.target.zone,
                          }
                        : null
                    }
                  />
                </>
              )}
              {snapPicker !== null && activeSession && (
                <>
                  {snapPicker.preview !== null && (
                    <div
                      className="snap-preview"
                      style={paneStyle(snapPicker.preview.layout, snapPicker.preview.zone)}
                      aria-hidden="true"
                    />
                  )}
                  <SnapBar
                    mode="pick"
                    current={splitView.layout}
                    initial={{ layout: splitView.layout, zone: splitView.split.focus }}
                    title={activeSession.title}
                    onActive={(target) => {
                      setSnapPicker({ preview: target });
                    }}
                    onPick={(target) => {
                      setSnapPicker(null);
                      splitView.place(activeSession.id, target.layout, target.zone);
                    }}
                    onClose={closeSnapPicker}
                  />
                </>
              )}
              {state.sessions.length === 0 && (
                <div className="empty">
                  <h1>{t("empty.title")}</h1>
                  <p>{allHidden ? t("sidebar.allHidden") : t("empty.body")}</p>
                  {quick.length > 0 && (
                    <ul className="empty-programs" aria-label={t("empty.programs")}>
                      {quick.map((profile) => {
                        const programVersion = programs.versions.get(profile.id);
                        return (
                          <li key={profile.id}>
                            <button
                              type="button"
                              className="empty-program"
                              title={[profile.command, ...profile.args].join(" ")}
                              onClick={(event) => {
                                startHere(profile, event.shiftKey);
                              }}
                            >
                              <Badge look={badgeFor(profile.id)} />
                              <span className="empty-program-name">{profile.name}</span>
                              {programVersion && (
                                <span className="empty-program-version">{programVersion}</span>
                              )}
                            </button>
                            {worktree === "yes" && (
                              <button
                                type="button"
                                className="link-button empty-worktree"
                                aria-label={t("start.withWorktree", { name: profile.name })}
                                title={t("start.withWorktree", { name: profile.name })}
                                onClick={() => {
                                  startHere(profile, true);
                                }}
                              >
                                {t("start.worktreeButton")}
                              </button>
                            )}
                          </li>
                        );
                      })}
                    </ul>
                  )}
                  <p className={target.source === "chosen" ? "empty-cwd chosen" : "empty-cwd"}>
                    <FolderIcon />
                    <span className="empty-cwd-path" title={targetTooltip(target, t)}>
                      {target.source === "chosen" ? t("start.nextIn") : t("sidebar.startsIn")}{" "}
                      <strong className="empty-cwd-name">
                        {targetPath === null ? "…" : folderName(targetPath)}
                      </strong>
                    </span>
                    <span aria-hidden="true">·</span>
                    <button
                      ref={emptyChangeButton}
                      type="button"
                      className="link-button empty-change"
                      aria-label={t("empty.changeLabel")}
                      aria-haspopup="menu"
                      aria-expanded={emptyFolderMenu !== null}
                      onClick={(event) => {
                        if (emptyFolderMenu !== null) {
                          setEmptyFolderMenu(null);
                          return;
                        }
                        const rect = event.currentTarget.getBoundingClientRect();
                        setEmptyFolderMenu({
                          x: rect.left,
                          y: rect.bottom + 4,
                          flipAbove: rect.top - 4,
                        });
                      }}
                    >
                      {t("empty.change")}
                    </button>
                    {target.source === "chosen" && (
                      <>
                        <span aria-hidden="true">·</span>
                        <button
                          type="button"
                          className="link-button empty-change"
                          onClick={() => {
                            setChosenCwd(null);
                          }}
                        >
                          {t("start.reset")}
                        </button>
                      </>
                    )}
                  </p>
                  <WorktreeHint chance={worktree} />
                  <p className="keys">
                    {nextProgram && (
                      <>
                        <Keys shortcut={SHORTCUT_LABELS.newSession} t={t} />{" "}
                        {t("empty.newSessionHint", { name: nextProgram.name })}
                        <br />
                      </>
                    )}
                    <Keys shortcut={SHORTCUT_LABELS.launcher} t={t} /> {t("empty.launcherHint")}
                    <br />
                    <Keys shortcut={SHORTCUT_LABELS.quickNote} t={t} /> {t("empty.notesHint")}
                    <br />
                    <Keys shortcut={SHORTCUT_LABELS.toggleSidebar} t={t} /> {t("empty.sidebarHint")}
                    <br />
                    <Keys shortcut={SHORTCUT_LABELS.openSettings} t={t} /> {t("empty.settingsHint")}
                  </p>
                </div>
              )}
            </div>
            {layout.sideOpen && notesPlacement.mode === "docked" && (
              <Splitter
                orientation="vertical"
                className="notes-edge"
                value={layout.notesWidth}
                direction={-1}
                label={t("side.width")}
                onChange={(width) => {
                  setLayout((current) => ({
                    ...current,
                    notesWidth: clampNotesWidth(width, workspaceWidth),
                  }));
                }}
                onReset={() => {
                  setLayout((current) => ({ ...current, notesWidth: NOTES_WIDTH.default }));
                }}
              />
            )}
            {layout.sideOpen && (
              <SidePanel
                tab={layout.sideTab}
                onTab={(tab) => {
                  openSide(tab);
                  if (tab === "changes") setChangesFocusToken((token) => token + 1);
                }}
                mode={notesPlacement.mode}
                width={notesPlacement.width}
                detached={notesWindow.detached}
                onDetach={detachNotes}
                onClose={closeSide}
                onEscape={focusTerminal}
                notes={
                  notesWindow.detached ? (
                    <div className="notes-detached">
                      <p>{t("notes.detached")}</p>
                      <div className="link-actions">
                        <button type="button" className="primary-button" onClick={bringBackNotes}>
                          {t("notes.bringBack")}
                        </button>
                        <button
                          type="button"
                          className="secondary-button"
                          onClick={() => {
                            toNotesWindow(null);
                          }}
                        >
                          {t("notes.showWindow")}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <NotesPanel
                      language={settings.language}
                      sessionFolder={sessionFolder}
                      folders={noteFolders}
                      sessions={sendTargets}
                      activeSessionId={state.activeId}
                      request={notesRequest}
                      refreshToken={notesRefresh}
                      handle={notesHandle}
                      onSend={sendFromNotes}
                      onChanged={notesChangedHere}
                      onEscape={() => {
                        if (notesPlacement.mode === "overlay") closeSide();
                        else focusTerminal();
                      }}
                    />
                  )
                }
                changes={
                  <ChangesPanel
                    claims={activeSession ? claimsOf(claims, activeSession.id) : null}
                    overlaps={activeSession ? overlapsOf(claims, activeSession.id) : []}
                    nameOf={sessionName}
                    onShowSession={showSession}
                    cwd={activeSession?.cwd ?? null}
                    trigger={activeSession ? (attention.get(activeSession.id) ?? "idle") : "none"}
                    focusToken={changesFocusToken}
                  />
                }
              />
            )}
          </div>
          <div className="notices">
            {pasteCheck !== null && (
              <div
                className="link-notice paste-check"
                role="alert"
                onKeyDown={(event) => {
                  if (event.key !== "Escape") return;
                  event.preventDefault();
                  event.stopPropagation();
                  answerPaste(false);
                }}
              >
                {pasteCheck.warning.runsLines && (
                  <span>{t("paste.runsLines", { name: pasteCheck.name })}</span>
                )}
                {pasteCheck.warning.hidden && <span>{t("paste.hidden")}</span>}
                <div className="link-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    ref={pasteCancelButton}
                    onClick={() => {
                      answerPaste(false);
                    }}
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => {
                      answerPaste(true);
                    }}
                  >
                    {t("paste.sendAnyway")}
                  </button>
                </div>
              </div>
            )}
            {startHint !== null && (
              <div className="link-notice start-hint" role="status" key={startHint}>
                <span>{startHint}</span>
              </div>
            )}
            {planChanged !== null && (
              <div className="link-notice" role="alert">
                <span>{t("update.planChanged", { name: planChanged })}</span>
                <div className="link-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      setPlanChanged(null);
                      programUpdates.checkNow();
                      focusActive();
                    }}
                  >
                    {t("update.checkAgain")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setPlanChanged(null);
                      focusActive();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
            {updateResult !== null && (
              <div
                className={
                  updateResult.tone === "ok" ? "link-notice" : "link-notice update-problem"
                }
                role="status"
                key={updateResult.key}
              >
                <span>{updateResult.message}</span>
                {updateResult.paths.length > 0 && (
                  <details className="update-offer-copies">
                    <summary>{t("updateOutcome.copies")}</summary>
                    <ol className="update-offer-paths selectable">
                      {updateResult.paths.map((path) => (
                        <li key={path}>{path}</li>
                      ))}
                    </ol>
                  </details>
                )}
                <div className="link-actions">
                  {updateResult.sessionId !== null &&
                    state.sessions.some((s) => s.id === updateResult.sessionId) && (
                      <button
                        type="button"
                        className="secondary-button"
                        onClick={() => {
                          const id = updateResult.sessionId;
                          setUpdateResult(null);
                          if (id !== null) dispatch({ type: "activated", id });
                          focusTerminal();
                        }}
                      >
                        {t("updateOutcome.showSession")}
                      </button>
                    )}
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setUpdateResult(null);
                      focusActive();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
            {claimNotice !== null && (
              <ClaimNoticeView
                key={claimNotice.key}
                overlap={claimNotice.overlap}
                activeId={state.activeId}
                nameOf={sessionName}
                onShow={(id) => {
                  setClaimNotice(null);
                  showSession(id);
                }}
                onDismiss={() => {
                  setClaimNotice(null);
                  focusActive();
                }}
              />
            )}
            {creatingWorktree && (
              <div className="link-notice" role="status">
                <span>{t("worktree.creating")}</span>
              </div>
            )}
            {worktreeNotice !== null && (
              <div className="link-notice" role="status">
                <span>{worktreeNotice.message}</span>
                <span className="link-url selectable">{revealInvisible(worktreeNotice.path)}</span>
                <div className="link-actions">
                  <button type="button" className="secondary-button" onClick={copyWorktreePath}>
                    {worktreeNotice.copied ? t("link.copied") : t("menu.copy")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setWorktreeNotice(null);
                      focusActive();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
            {link !== null && (
              <div className="link-notice" role="status">
                <span>{t("link.notice")}</span>
                <span className="link-url selectable">{link.url}</span>
                <div className="link-actions">
                  <button type="button" className="secondary-button" onClick={copyLink}>
                    {link.copied ? t("link.copied") : t("menu.copy")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setLink(null);
                      focusTerminal();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
          </div>
        </main>
        {emptyFolderMenu !== null && (
          <FolderMenu
            target={targetPath}
            running={running}
            folders={folders}
            position={emptyFolderMenu}
            flipAbove={emptyFolderMenu.flipAbove}
            anchor={emptyChangeButton}
            onSelect={selectCwd}
            onChoose={() => void pickCwd()}
            onToggleFavorite={(path) => {
              setFolders((current) => toggleFavorite(current, path));
            }}
            onClose={(reason) => {
              setEmptyFolderMenu(null);
              // No terminal to return to: the keyboard stays at the button.
              if (reason === "dismiss" || state.sessions.length === 0) {
                emptyChangeButton.current?.focus();
              }
            }}
          />
        )}
        {terminalMenu !== null && (
          <PopupMenu
            items={terminalItems}
            label={t("terminal.menu")}
            position={terminalMenu}
            onClose={() => {
              setTerminalMenu(null);
              focusTerminal();
            }}
          />
        )}
        {sessionMenu !== null && menuSession !== undefined && (
          <PopupMenu
            items={sessionItems}
            label={t("session.menu")}
            position={sessionMenu}
            onClose={() => {
              setSessionMenu(null);
              focusActive();
            }}
          />
        )}
        {markerSession && (
          <MarkerDialog
            title={markerSession.title}
            marker={markerSession.marker}
            onChange={(marker) => {
              setMarker(markerSession.id, marker);
            }}
            onClose={closeMarker}
          />
        )}
        {dialog === "launcher" && (
          <LauncherDialog
            buildItems={launcherItems}
            onClose={closeDialog}
            onRun={(item) => {
              closeDialog();
              item.run();
            }}
          />
        )}
        {dialog === "broadcast" && (
          <BroadcastDialog
            candidates={broadcastable}
            initial={defaultTargets(
              broadcastable,
              (profileId) => profileById.get(profileId)?.command,
            )}
            badgeFor={badgeFor}
            onSend={sendToSeveral}
            onClose={closeDialog}
          />
        )}
        <div className="sr-only" role="status">
          {moveAnnouncement}
        </div>
        {draggedSession && (
          <div ref={attachDragGhost} className="drag-ghost" aria-hidden="true">
            <Badge look={badgeFor(draggedSession.profileId)} />
            <span className="drag-ghost-title">{draggedSession.title}</span>
          </div>
        )}
        {/* For screen readers: waiting sessions are announced without moving focus. */}
        <div className="sr-only" role="status">
          {waiting.map((s) => t("attention.announce", { title: s.title })).join(". ")}
        </div>
        {closingSession && (
          <Dialog labelledBy="close-title" onClose={cancelClose}>
            <h2 id="close-title">{t("close.confirm", { name: closingSession.title })}</h2>
            <p className="muted">{t("close.confirmBody")}</p>
            {closingSession.worktree && (
              <p className="muted">
                {t("close.worktree", { branch: revealInvisible(closingSession.worktree.branch) })}
              </p>
            )}
            <div className="dialog-actions">
              <button
                type="button"
                className="secondary-button"
                data-autofocus
                onClick={cancelClose}
              >
                {t("common.cancel")}
              </button>
              <button type="button" className="primary-button danger" onClick={confirmClose}>
                {t("session.killAndClose")}
              </button>
            </div>
          </Dialog>
        )}
        {dialog === "about" && (
          <Dialog labelledBy="about-title" onClose={closeDialog}>
            <h2 id="about-title">Hangar {version !== null && <VersionText version={version} />}</h2>
            <p>{t("about.body")}</p>
            <p className="muted selectable">
              {profilePath === null
                ? t("about.profilesFallback")
                : t("about.profiles", { path: profilePath })}
            </p>
            <div className="dialog-actions">
              <button type="button" className="primary-button" data-autofocus onClick={closeDialog}>
                {t("common.close")}
              </button>
            </div>
          </Dialog>
        )}
        {dialog === "whatsNew" && (
          <WhatsNewDialog
            language={settings.language}
            since={whatsNewSince}
            current={version}
            onClose={closeDialog}
          />
        )}
        {dialog === "settings" && (
          <SettingsDialog
            settings={settings}
            profiles={profiles}
            versions={programs.versions}
            badgeFor={badgeFor}
            fontSize={layout.fontSize}
            theme={theme}
            update={updater.state}
            version={version}
            programUpdates={programUpdates}
            updateActions={{
              ...updateActions,
              // The question stays in Settings; a started update closes the dialog, because it
              // is modal and the new session should be visible right away.
              request: (profileId) => {
                if (runningPrograms.has(profileId) && !updatingPrograms.has(profileId)) {
                  setConfirmUpdate(profileId);
                  return;
                }
                setDialog(null);
                updateActions.request(profileId);
              },
              confirm: (profileId) => {
                setDialog(null);
                updateActions.confirm(profileId);
              },
            }}
            onChange={updateSettings}
            onClearScrollbacks={clearScrollbacks}
            onOpenProfileFile={openProfileFile}
            onReloadProfiles={reloadProfilesFromSettings}
            onProfiles={applyProfiles}
            onFontSize={(fontSize) => {
              setLayout((current) => ({ ...current, fontSize }));
            }}
            onResetLayout={() => {
              setLayout(DEFAULT_LAYOUT);
            }}
            onCheckUpdates={() => void updater.check(true)}
            onClose={closeDialog}
          />
        )}
      </div>
    </I18nContext.Provider>
  );
}

function Keys({ shortcut, t }: { shortcut: Shortcut; t: Translate }) {
  return (
    <span className="key-combo">
      {shortcutKeys(shortcut, t).map((key, index) => (
        <span key={key}>
          {index > 0 && " + "}
          <kbd>{key}</kbd>
        </span>
      ))}
    </span>
  );
}
