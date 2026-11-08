import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
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
  parseExportResult,
  type NoteFolder,
  type Profile,
  type SessionInfo,
} from "./api";
import type { MenuGroup, MenuItem } from "./AppMenu";
import type { AttentionConfig, AttentionState } from "./attention";
import { decideWhatsNew, loadLastSeen, saveLastSeen } from "./changelog";
import { Badge } from "./Badge";
import { broadcastCandidates, defaultTargets, sendToTargets } from "./broadcast";
import { BroadcastDialog } from "./BroadcastDialog";
import { writeClipboard } from "./clipboard";
import { Dialog } from "./Dialog";
import { FolderMenu } from "./FolderMenu";
import {
  folderKey,
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
  type MessageKey,
  type Translate,
} from "./i18n";
import { formatShortTime, shortPath, statusText } from "./format";
import { CloseIcon, FolderIcon, NotesIcon } from "./icons";
import { LauncherDialog, type LauncherItem } from "./LauncherDialog";
import type { Marker } from "./marker";
import { MarkerDialog } from "./MarkerDialog";
import { usableSnippets, snippetLabel, type Snippet } from "./snippets";
import {
  EMPTY_SLOTS,
  reconcileSlots,
  showSessions,
  SPLIT_MODES,
  slotOf,
  type SplitMode,
  type SplitSlots,
} from "./split";
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
  type SidebarGroup,
} from "./layout";
import { folderId, parseNoteFolders, type NoteFolderInfo } from "./notes";
import { flushNotes, NotesPanel, type NoteScope } from "./NotesPanel";
import { dropText, shellKind } from "./pathQuote";
import { profileErrorText, shouldReportReloadError } from "./profileFile";
import { installedKey, sameProfiles } from "./profiles";
import { arrangedProfiles, resolveBadge, startableProfiles } from "./programPrefs";
import { availableVersion } from "./programUpdates";
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
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
  SHORTCUT_LABELS,
  shortcutKeys,
  type AppAction,
  type Shortcut,
} from "./shortcuts";
import { Sidebar } from "./Sidebar";
import { Splitter } from "./Splitter";
import { TaskbarFlasher } from "./taskbarFlash";
import { SessionTerminal, terminals } from "./terminal";
import { schemeColors, type TerminalLook } from "./terminalLook";
import { EmptyPane, TerminalPane } from "./TerminalPane";
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
import { useUpdater } from "./useUpdater";
import { HANGAR_BUILD } from "./buildInfo";
import { withBuild } from "./version";
import { VersionText } from "./VersionText";
import { CHANGELOG, WhatsNewDialog } from "./WhatsNewDialog";
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
/** Initial size until xterm has measured the real space (a resize follows right after). */
const INITIAL_SIZE = { cols: 120, rows: 30 };
const COPY_KEYS: Shortcut = { mods: ["ctrl"], key: "C" };
const PASTE_KEYS: Shortcut = { mods: ["ctrl"], key: "V" };

type OpenDialog = "about" | "settings" | "whatsNew" | "launcher" | "broadcast" | null;

const SPLIT_MODE_NAMES: Record<SplitMode, MessageKey> = {
  single: "menu.viewSingle",
  columns: "menu.viewColumns",
  grid: "menu.viewGrid",
};

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

function readLastCwd(): string | null {
  try {
    return localStorage.getItem(LAST_CWD_KEY);
  } catch {
    return null;
  }
}

function storeLastCwd(cwd: string): void {
  try {
    localStorage.setItem(LAST_CWD_KEY, cwd);
  } catch {
    // Without a stored value, the next start suggests the home folder.
  }
}

/** Is the user looking at the window? Visible and with keyboard focus. */
function windowWatched(): boolean {
  return document.visibilityState === "visible" && document.hasFocus();
}

export function App() {
  const [state, dispatch] = useReducer(sessionsReducer, initialSessions);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [cwd, setCwd] = useState<string | null>(readLastCwd);
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
  /** Which session sits in which pane of the split view (split.ts). */
  const [splitSlots, setSplitSlots] = useState<SplitSlots>(EMPTY_SLOTS);
  /** Session whose marker is being edited. */
  const [markerId, setMarkerId] = useState<number | null>(null);
  /** Session menu (right click on a session), open at this position. */
  const [sessionMenu, setSessionMenu] = useState<{ id: number; x: number; y: number } | null>(null);
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
  const [noteScope, setNoteScope] = useState<NoteScope>({ follow: true });
  const [notesFocusToken, setNotesFocusToken] = useState(0);
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
  /**
   * Titles of just started sessions that are not in the state yet. Started in quick succession
   * (double click, Ctrl+Shift+T), both would otherwise get the same number.
   */
  const claimedTitles = useRef(new Map<number, string>());
  const settingsRef = useRef(settings);
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
    api
      .defaultCwd()
      .then((home) => {
        setCwd((current) => current ?? home);
      })
      .catch(showError);
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
    document.documentElement.style.setProperty("--attention", settings.attentionColor);
  }, [settings.attentionColor]);

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
  const sessionFolder: NoteFolder | null = activeSession ? { path: activeSession.cwd } : null;
  const notesFolder: NoteFolder = noteScope.follow
    ? (sessionFolder ?? { path: null })
    : noteScope.folder;

  if (search !== null && search.sessionId !== state.activeId) setSearch(null);

  // Panes follow the sessions: closed ones leave, the active one is always shown and focused.
  // Adjusted during render, so no frame shows a pane without its session.
  const split = reconcileSlots(
    splitSlots,
    layout.splitMode,
    state.sessions.map((session) => session.id),
    state.activeId,
  );
  if (split !== splitSlots) setSplitSlots(split);
  // The marker dialog or session menu of a session that is gone (closed, or replaced by a restart
  // with a new ID) closes, or the modal flag would keep blocking the shortcuts.
  if (markerId !== null && !state.sessions.some((s) => s.id === markerId)) setMarkerId(null);
  if (sessionMenu !== null && !state.sessions.some((s) => s.id === sessionMenu.id)) {
    setSessionMenu(null);
  }

  // New active session: the notes follow its working folder again (adjusted during render,
  // not in an effect, so no intermediate state with the old folder becomes visible).
  const [scopeSession, setScopeSession] = useState(state.activeId);
  if (scopeSession !== state.activeId) {
    setScopeSession(state.activeId);
    if (!noteScope.follow) setNoteScope({ follow: true });
  }

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

  /** Connects a terminal to its session ID: exit, attention, visibility. */
  const wire = useCallback(
    (terminal: SessionTerminal, id: number) => {
      terminal.onExit = (code) => {
        dispatch({ type: "exited", id, code });
      };
      terminal.isWatched = () => activeIdRef.current === id && windowWatched();
      terminal.onLink = (url) => {
        setLink({ url, copied: false });
      };
      // Only state changes arrive here, never individual output chunks.
      terminal.setAttentionListener((next) => {
        flasher.attentionChanged(next, {
          enabled: settingsRef.current.taskbarFlash,
          windowFocused: document.hasFocus(),
        });
        setAttention((current) => {
          if ((current.get(id) ?? "idle") === next) return current;
          const map = new Map(current);
          map.set(id, next);
          return map;
        });
      });
      terminals.set(id, terminal);
    },
    [flasher],
  );

  const forgetAttention = useCallback((id: number) => {
    setAttention((current) => {
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

  /** `folder`: start there instead of the "Starts in" folder (quick access). */
  const start = useCallback(
    async (profile: Profile, folder?: string) => {
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
          folder ?? cwd,
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
        const title = numberedTitle(info.title, others);
        claimedTitles.current.set(info.id, title);
        dispatch({ type: "added", session: await withTitle(info, title) });
        saveLastProgram(profile.id);
        setLastProgramId(profile.id);
        // The folder the session really runs in (the backend may fall back to the home folder).
        setFolders((current) => recordRecent(current, info.cwd));
        // Very short processes may already have exited before the response arrives.
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
        }
      } catch (e) {
        terminal.dispose();
        showError(e);
      }
    },
    [
      cwd,
      layout.fontSize,
      attentionConfig,
      theme,
      terminalLook,
      settings.scrollbackLines,
      showError,
      wire,
      withTitle,
    ],
  );

  /**
   * After an update session ends: detect versions again and recheck them so the
   * update dot disappears. The backend cleared its version cache before the exit event.
   */
  const afterUpdateRef = useRef<() => void>(() => undefined);
  useEffect(() => {
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
        showError(e);
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
        session: {
          id,
          profileId: saved.profileId,
          title: saved.title.trim() === "" ? saved.profileId : saved.title,
          cwd: saved.cwd,
          status: "exited",
          exitCode: null,
          createdAt: Date.now(),
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
        );
        terminal.setId(info.id);
        wire(terminal, info.id);
        dispatch({
          type: "restored",
          order: saved.order,
          marker: { color: saved.color, emoji: saved.emoji },
          session: await withTitle(info, saved.title),
        });
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
        }
        return info.id;
      } catch (e) {
        terminal.writeNotice(tr("terminal.startFailed", { error: errorText(e) }));
        return addPlaceholder(terminal, saved);
      }
    },
    [addPlaceholder, wire, withTitle],
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
          );
          terminal.setId(info.id);
          terminals.delete(id);
          wire(terminal, info.id);
          forgetAttention(id);
          dispatch({ type: "replaced", oldId: id, session: await withTitle(info, session.title) });
          if (terminal.exitCode !== undefined) {
            dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
          }
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
    [showError, wire, forgetAttention, withTitle, t],
  );

  const close = useCallback(
    async (id: number) => {
      if (!isPlaceholder(id)) {
        try {
          await api.close(id);
        } catch (e) {
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
    },
    [showError, forgetAttention],
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
    const work = Promise.all([
      flushNotes().catch(() => undefined),
      saveWorkspaceNow().catch(() => undefined),
      saveScrollbacks(true).catch(() => undefined),
    ]);
    await Promise.race([
      work,
      new Promise((resolve) => window.setTimeout(resolve, QUIT_SAVE_TIMEOUT_MS)),
    ]);
  }, [saveWorkspaceNow, saveScrollbacks]);

  const quit = useCallback(async () => {
    if (quitting.current) return;
    quitting.current = true;
    try {
      await persistAll();
    } finally {
      try {
        await exit(0);
      } catch (e) {
        // Otherwise `quitting` would stay set and the window could never be closed again.
        quitting.current = false;
        showError(e);
      }
    }
  }, [persistAll, showError]);

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

  const selectCwd = useCallback((path: string) => {
    setCwd(path);
    storeLastCwd(path);
  }, []);

  const pickCwd = useCallback(async () => {
    try {
      const picked = await open(cwd ? { directory: true, defaultPath: cwd } : { directory: true });
      if (typeof picked === "string") {
        setCwd(picked);
        storeLastCwd(picked);
      }
    } catch (e) {
      showError(e);
    }
  }, [cwd, showError]);

  const changeFont = (delta: number | null) => {
    setLayout((current) => ({
      ...current,
      fontSize: delta === null ? FONT_SIZE.default : clampFontSize(current.fontSize + delta),
    }));
  };

  const toggleSidebar = () => {
    setLayout((current) => ({ ...current, collapsed: !current.collapsed }));
  };

  /** Opening gives focus to the editor, closing gives it to the terminal. */
  const toggleNotes = () => {
    if (layout.notesOpen) {
      setLayout((current) => ({ ...current, notesOpen: false }));
      focusTerminal();
    } else {
      setLayout((current) => ({ ...current, notesOpen: true }));
      setNotesFocusToken((token) => token + 1);
    }
  };

  /** Click on a notes scope in the sidebar. */
  const openNotes = (folder: NoteFolder) => {
    const follow = sessionFolder !== null && folderId(sessionFolder) === folderId(folder);
    setNoteScope(follow ? { follow: true } : { follow: false, folder });
    setLayout((current) => (current.notesOpen ? current : { ...current, notesOpen: true }));
    setNotesFocusToken((token) => token + 1);
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
  const setSplitMode = (mode: SplitMode) => {
    setLayout((current) =>
      current.splitMode === mode ? current : { ...current, splitMode: mode },
    );
    focusTerminal();
  };

  /** Double click on a pane header: show only this session (View menu brings the split back). */
  const maximize = (id: number) => {
    setLayout((current) => ({ ...current, splitMode: "single" }));
    dispatch({ type: "activated", id });
    focusTerminal();
  };

  /** Snippet into the visible session, like pasting: bracketed paste, no Enter. */
  const insertSnippet = (snippet: Snippet) => {
    const session = activeSession;
    const terminal = activeTerminal();
    if (!session || session.status === "exited" || !terminal) return;
    terminal.pasteText(snippet.text);
    focusTerminal();
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
    const next = showSessions(ids, layout.splitMode);
    setLayout((current) => ({ ...current, splitMode: next.mode }));
    setSplitSlots(next.split);
    const first = ids[0];
    if (first !== undefined) dispatch({ type: "activated", id: first });
    focusTerminal();
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
  /** Programs that are running right now: on Windows their update can fail on locked files. */
  const runningPrograms = new Set(
    state.sessions
      .filter((s) => s.kind !== "update" && s.status !== "exited")
      .map((s) => s.profileId),
  );

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
      case "toggleNotes":
        toggleNotes();
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
        if (nextProgram) void start(nextProgram);
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
  useEffect(() => {
    performRef.current = perform;
    openTerminalMenuRef.current = openTerminalMenu;
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // Reloading disconnects the UI from all sessions. In the terminal, the keys belong to the shell.
      if (isReloadKey(event) && !inTerminal(event.target)) {
        event.preventDefault();
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
              run: () => void start(profile),
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
          label: layout.notesOpen ? t("notes.hide") : t("notes.show"),
          shortcut: keys("toggleNotes"),
          run: toggleNotes,
        },
        {
          type: "action",
          label: theme === "dark" ? t("sidebar.toLight") : t("sidebar.toDark"),
          run: toggleTheme,
        },
        { type: "separator" },
        ...SPLIT_MODES.map((mode): MenuItem => ({
          type: "action",
          label: t(SPLIT_MODE_NAMES[mode]),
          ...(mode === layout.splitMode ? { detail: t("folders.current") } : {}),
          run: () => {
            setSplitMode(mode);
          },
        })),
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
  const notesPlacement = placeNotes(layout.notesWidth, workspaceWidth);
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
  const sessionItems: PopupItem[] =
    menuSession === undefined
      ? []
      : [
          {
            type: "action",
            key: "rename",
            label: t("session.rename"),
            run: () => {
              startRename(menuSession.id);
            },
          },
          {
            type: "action",
            key: "marker",
            label: t("menu.marker"),
            run: () => {
              setMarkerId(menuSession.id);
            },
          },
          { type: "separator", key: "sep" },
          menuSession.status === "exited"
            ? {
                type: "action",
                key: "restart",
                label: t("session.restart"),
                run: () => void restart(menuSession.id),
              }
            : {
                type: "action",
                key: "kill",
                label: t("session.kill"),
                run: () => {
                  kill(menuSession.id);
                },
              },
          {
            type: "action",
            key: "close",
            label: menuSession.status === "exited" ? t("session.close") : t("session.killAndClose"),
            run: () => {
              requestClose(menuSession.id);
            },
          },
        ];

  const markerSession =
    markerId === null ? undefined : state.sessions.find((s) => s.id === markerId);

  /** Entries of quick access; built only while it is open. */
  const launcherItems = (): LauncherItem[] => {
    const items: LauncherItem[] = [];
    for (const session of state.sessions) {
      const profile = profileById.get(session.profileId);
      items.push({
        key: `session:${String(session.id)}`,
        section: "sessions",
        label: [session.marker?.emoji, session.title].filter(Boolean).join(" "),
        detail: shortPath(session.cwd),
        keywords: [profile?.name ?? "", session.cwd].join(" "),
        primary: true,
        icon: <Badge look={badgeFor(session.profileId)} />,
        run: () => {
          dispatch({ type: "activated", id: session.id });
          focusTerminal();
        },
      });
    }
    // Program in folder: the "Starts in" folder first, then favorites and recent folders.
    const seen = new Set<string>();
    const places = [cwd, ...folders.favorites, ...folders.recent].filter((path): path is string => {
      if (path === null || seen.has(folderKey(path))) return false;
      seen.add(folderKey(path));
      return true;
    });
    for (const place of places) {
      for (const profile of startable) {
        items.push({
          key: `start:${profile.id}:${place}`,
          section: "start",
          label: t("launcher.startIn", { program: profile.name, folder: folderName(place) }),
          detail: shortPath(place),
          keywords: place,
          // Without sessions, the programs in the current folder are the obvious next step.
          primary: state.sessions.length === 0 && place === cwd,
          icon: <Badge look={badgeFor(profile.id)} />,
          run: () => void start(profile, place),
        });
      }
    }
    for (const info of noteFolders) {
      if (info.noteCount === 0) continue;
      const folder: NoteFolder = { path: info.path };
      const label = info.path === null ? t("notes.general") : folderName(info.path);
      items.push({
        key: `notes:${info.folderKey}`,
        section: "notes",
        label: t("launcher.notesIn", { folder: label }),
        ...(info.path === null ? {} : { detail: shortPath(info.path) }),
        icon: <NotesIcon size={14} />,
        run: () => {
          openNotes(folder);
        },
      });
    }
    for (const snippet of snippets) {
      items.push({
        key: `snippet:${snippet.id}`,
        section: "snippets",
        label: snippetLabel(snippet),
        keywords: snippet.text.slice(0, 200),
        primary: true,
        disabled: !canInsert,
        ...(canInsert ? {} : { detail: t("launcher.noSession") }),
        run: () => {
          insertSnippet(snippet);
        },
      });
    }
    for (const mode of SPLIT_MODES) {
      items.push({
        key: `view:${mode}`,
        section: "commands",
        label: t("launcher.view", { mode: t(SPLIT_MODE_NAMES[mode]) }),
        ...(mode === layout.splitMode ? { detail: t("folders.current") } : {}),
        primary: true,
        run: () => {
          setSplitMode(mode);
        },
      });
    }
    items.push({
      key: "broadcast",
      section: "commands",
      label: t("menu.broadcast"),
      primary: true,
      disabled: broadcastable.length === 0,
      run: () => {
        setDialog("broadcast");
      },
    });
    return items;
  };

  /** Header and position of each visible pane in split view. */
  const paneSplit = (session: SessionView) => {
    const mode = layout.splitMode;
    if (mode === "single") return null;
    const slot = slotOf(split, session.id);
    if (slot === null) return null;
    const paneState = attention.get(session.id) ?? "idle";
    return {
      mode,
      slot,
      session,
      look: badgeFor(session.profileId),
      state: paneState,
      status: statusText(session, t, paneState),
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
          notesOpen={layout.notesOpen}
          onToggleNotes={toggleNotes}
          noteFolders={noteFolders}
          notesFolder={layout.notesOpen ? notesFolder : null}
          onOpenNotes={openNotes}
          closedGroups={layout.closedGroups}
          onToggleGroup={(group: SidebarGroup) => {
            setLayout((current) => toggleGroup(current, group));
          }}
          onFocusTerminal={focusTerminal}
          version={version}
          theme={theme}
          onToggleTheme={toggleTheme}
          cwd={cwd}
          collapsed={layout.collapsed}
          width={layout.sidebarWidth}
          footerHeight={layout.footerHeight}
          menu={menu}
          update={updater.state}
          attention={attention}
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
          onStart={(profile) => {
            void start(profile);
          }}
          runningPrograms={runningPrograms}
          onRunUpdate={(profileId) => {
            void startUpdate(profileId);
          }}
          onPickCwd={() => {
            void pickCwd();
          }}
          folders={folders}
          onSelectCwd={selectCwd}
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
            <div className={state.sessions.length > 0 ? "panes filled" : "panes"}>
              {state.sessions.map((session) => {
                const terminal = terminals.get(session.id);
                if (!terminal) return null;
                const active = session.id === state.activeId;
                const paneInfo = paneSplit(session);
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
                    onMaximize={maximize}
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
              {layout.splitMode !== "single" &&
                state.sessions.length > 0 &&
                split.slots.map((id, slot) =>
                  id === null && layout.splitMode !== "single" ? (
                    <EmptyPane key={`empty-${String(slot)}`} mode={layout.splitMode} slot={slot} />
                  ) : null,
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
                              onClick={() => void start(profile)}
                            >
                              <Badge look={badgeFor(profile.id)} />
                              <span className="empty-program-name">{profile.name}</span>
                              {programVersion && (
                                <span className="empty-program-version">{programVersion}</span>
                              )}
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  )}
                  <p className="empty-cwd">
                    <FolderIcon />
                    <span className="empty-cwd-path" title={cwd ?? undefined}>
                      {t("empty.startsIn", { folder: cwd ? shortPath(cwd) : "…" })}
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
                  </p>
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
                    <Keys shortcut={SHORTCUT_LABELS.toggleNotes} t={t} /> {t("empty.notesHint")}
                    <br />
                    <Keys shortcut={SHORTCUT_LABELS.toggleSidebar} t={t} /> {t("empty.sidebarHint")}
                    <br />
                    <Keys shortcut={SHORTCUT_LABELS.openSettings} t={t} /> {t("empty.settingsHint")}
                  </p>
                </div>
              )}
            </div>
            {layout.notesOpen && (
              <>
                {notesPlacement.mode === "docked" && (
                  <Splitter
                    orientation="vertical"
                    className="notes-edge"
                    value={layout.notesWidth}
                    direction={-1}
                    label={t("notes.width")}
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
                <NotesPanel
                  folder={notesFolder}
                  following={noteScope.follow}
                  sessionFolder={sessionFolder}
                  folders={noteFolders}
                  width={notesPlacement.width}
                  overlay={notesPlacement.mode === "overlay"}
                  focusToken={notesFocusToken}
                  onScope={setNoteScope}
                  onChanged={refreshNoteFolders}
                  onClose={toggleNotes}
                  onFocusTerminal={focusTerminal}
                />
              </>
            )}
          </div>
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
        </main>
        {emptyFolderMenu !== null && (
          <FolderMenu
            cwd={cwd}
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
        {/* For screen readers: waiting sessions are announced without moving focus. */}
        <div className="sr-only" role="status">
          {waiting.map((s) => t("attention.announce", { title: s.title })).join(". ")}
        </div>
        {closingSession && (
          <Dialog labelledBy="close-title" onClose={cancelClose}>
            <h2 id="close-title">{t("close.confirm", { name: closingSession.title })}</h2>
            <p className="muted">{t("close.confirmBody")}</p>
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
            runningPrograms={runningPrograms}
            onRunUpdate={(profileId) => {
              // The dialog is modal; the new session should be visible right away.
              setDialog(null);
              void startUpdate(profileId);
            }}
            onChange={updateSettings}
            onClearScrollbacks={clearScrollbacks}
            onOpenProfileFile={openProfileFile}
            onReloadProfiles={reloadProfilesFromSettings}
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
