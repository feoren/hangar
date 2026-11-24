import { getVersion } from "@tauri-apps/api/app";
import { activityFolders } from "./activity";
import { ActivityCalendar } from "./ActivityCalendar";
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
  LINK_GROUPS_CHANGED,
  NOTIFICATION_ACTIVATED,
  PARTNER_MESSAGES_CHANGED,
  parseActivatedSession,
  parseExportResult,
  TRAY_QUIT,
  parsePathIndex,
  type AgentCommand,
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
import { resolvedColor } from "./stateLook";
import { lookVariables } from "./stateLookCss";
import { chimeFor, ChimeScheduler } from "./chime";
import { claimsOf, markerText, overlapNotice, overlapsBySession, type Overlap } from "./claims";
import { playChime } from "./chimeAudio";
import { drawBadgePng } from "./badgeImage";
import { NO_ACTIVITY } from "./statusLine";
import { notificationText, shouldNotify } from "./desktopNotify";
import { branchPaths, parseBranches } from "./gitBranches";
import {
  decideWhatsNew,
  loadLastSeen,
  saveLastSeen,
  whatsNewNeedsEntries,
  type ChangelogEntry,
} from "./changelog";
import { loadChangelog } from "./changelogData";
import { pasteFromApp, type PasteWarning } from "./appPaste";
import { agentIcon } from "./agentIcons";
import { Badge } from "./Badge";
import { AgentFigure } from "./AgentFigure";
import { SessionLook } from "./SessionLook";
import { agentOrigin } from "./agentStartMenu";
import { defFigure, defsFor, type AgentDef } from "./subAgentLibrary";
import { setLibraryFolders, useSubAgentLibrary } from "./subAgentLibraryStore";
import { broadcastCandidates, defaultTargets, sendToTargets } from "./broadcast";
import { BroadcastDialog } from "./BroadcastDialog";
import {
  arenaErrorText,
  contestantState,
  newContestant,
  others,
  parseApplyOutcome,
  parseArena,
  parseTestRun,
  promptReady,
  testsDue,
  trackAttention,
  type ApplyOutcome,
  type ArenaRun,
  type ArenaSetup,
  type Contestant,
} from "./arena";
import { ArenaDialog, type ArenaInput } from "./ArenaDialog";
import { writeClipboard } from "./clipboard";
import { Dialog } from "./Dialog";
import {
  EMPTY_FOLDERS,
  folderName,
  forgetFolder,
  moveFavorite,
  recordRecent,
  samePath,
  toggleFavorite,
  type FolderPrefs,
} from "./folders";
import {
  createTranslator,
  detectLanguage,
  I18nContext,
  setCurrentLanguage,
  type MessageKey,
} from "./i18n";
import { formatShortTime, statusText } from "./format";
import {
  CloseIcon,
  FileIcon,
  FolderIcon,
  NotesIcon,
  SourceControlIcon,
  TemplateIcon,
} from "./icons";
import { LauncherDialog } from "./LauncherDialog";
import { buildLauncherItems, commitItems, fileItems } from "./launcherItems";
import { LinkAddress } from "./LinkAddress";
import type { Marker } from "./marker";
import { MarkerDialog } from "./MarkerDialog";
import { BudgetNotice } from "./BudgetNotice";
import type { BudgetWarned } from "./costs";
import { MorningNotice } from "./MorningNotice";
import { UsageStatsNotice } from "./UsageStatsNotice";
import { countFeature, parseUsageConsent, usageSettings, type UsageConsent } from "./usageStats";

import { CrashNotice } from "./CrashNotice";
import { crashSummary, type RestoredSession } from "./crashRestore";
import { CostDialog } from "./CostDialog";
import {
  buildHandover,
  canHandOver,
  canRequestSummary,
  handoverStart,
  handoverTargets,
  NOTES_MAX as HANDOVER_NOTES_MAX,
  summaryRequest,
  waitUntilReady,
} from "./handover";
import { HandoverDialog } from "./HandoverDialog";
import { PartnerMessageDialog, type PartnerSend, type TargetView } from "./PartnerMessageDialog";
import { LinkDialog, LinkGroupDialog, type AddedMember } from "./LinkDialog";
import { LinkHistoryDialog } from "./LinkHistoryDialog";
import {
  BRAKE_WINDOW_MS,
  CLAIM_TIMEOUT_MS,
  deliverAutomatically,
  EMPTY_LINKS,
  groupOf,
  groupsWithRoom,
  INTRO_PATIENCE_MS,
  leadOf,
  selectableForLink,
  selectionPlan,
  MESSAGE_PATIENCE_MS,
  TYPING_PATIENCE_MS,
  heldBySession,
  linkCandidates,
  linkMarks,
  linkOffer,
  newlyBraked,
  nextDeliveries,
  parseLinkHistory,
  parseLinkState,
  templateGroup,
  templateLinkRequest,
  templateMembers,
  type LinkGroup,
  type LinkHistoryEntry,
  type LinkMessage,
  type LinkState,
  type StartedMember,
} from "./links";
import { memberAliases, type AliasSource } from "./linkAliases";
import { introKind, introSummary, type IntroKind } from "./linkIntro";
import {
  CardSends,
  cardNotice,
  discardFromCard,
  latestOnly,
  pruneHidden,
  sendFromCard,
  type CardSend,
} from "./messageCard";
import { currentPlatform } from "./platform";
import { deliverMessage, deliveryPlan, parsePartnerTargets } from "./partnerMessage";
import {
  inboxBySession,
  parseWaitingMessages,
  unshownMessages,
  type WaitingMessage,
} from "./partnerInbox";
import {
  EMPTY_BOOK,
  parsePartnerBook,
  partnersOf,
  projectOfSession,
  type PartnerBook,
} from "./partners";
import type { PartnerActions } from "./ProjectPartners";
import { noteExcerpt } from "./noteText";
import { queueOffer, sessionMenuEntries, stallOffer, type SessionAction } from "./sessionMenu";
import { canExportReport, reportLabels, utcOffsetMinutes } from "./sessionReport";
import {
  asksQuestion,
  awaitsAnswer,
  commandBusy,
  commandMenuEntries,
  compactRepeated,
  sendCommand,
  sessionCommands,
} from "./agentCommands";
import { AgentCommandConfirm } from "./AgentCommandConfirm";
import {
  newSnippetId,
  usableSnippets,
  snippetLabel,
  snippetTextFromSelection,
  type Snippet,
} from "./snippets";
import { SaveSnippetDialog } from "./SaveSnippetDialog";
import { dropLine } from "./sessionDrag";
import { groupedOrder, groupPosition, sessionGroupKey } from "./sessionGroups";
import { SnapBar } from "./SnapBar";
import { LAYOUT_IDS, LAYOUT_NAMES, type Ratios, type SnapTarget } from "./snapLayouts";
import { pickProgram } from "./launch";
import {
  clampFontSize,
  clampNotesWidth,
  clampSidebarWidth,
  DEFAULT_LAYOUT,
  FONT_SIZE,
  isGroupOpen,
  loadLayout,
  NOTES_WIDTH,
  openGroup,
  placeNotes,
  saveLayout,
  SIDEBAR_RAIL_WIDTH,
  toggleAgentGroup,
  toggleGroup,
  type Layout,
  type SideTab,
  type SidebarGroup,
} from "./layout";
import { parseNoteFolders, parseNotes, type NoteFolderInfo } from "./notes";
import {
  loadWindowPrefs,
  PANEL_COMMAND,
  PANEL_CONTEXT,
  PANEL_MESSAGE,
  PANEL_WINDOW_LABEL,
  panelContextPayload,
  panelSessionIsAgent,
  parsePanelMessage,
  updateWindowPrefs,
  WINDOW_PREFS_KEY,
  type PanelCommand,
  type PanelContext,
  type PanelWindowPrefs,
} from "./panelBridge";
import {
  flushNotes,
  NotesPanel,
  type NotesHandle,
  type NotesRequest,
  type SendTarget,
} from "./NotesPanel";
import { SidePanel } from "./SidePanel";
import { dropText, insertPathText, shellKind, unsafeDropPath, type TreePath } from "./pathQuote";
import { promptVerdict } from "./promptTitle";
import { pushEventFor, PushScheduler, PUSH_TEXTS } from "./phonePush";
import { isQuietTime, noticesAt } from "./quietHours";
import {
  isPowerSaving,
  onPowerSavingChange,
  setPowerSavingEnabled,
  startPowerWatch,
} from "./usePower";
import { setFigureMotion } from "./useFigureMotion";
import { pollInterval } from "./power";
import { observeResources, setResourcesEnabled } from "./useSessionResources";
import { setTreeFlow } from "./treeFlowStore";
import {
  discardMessage,
  EMPTY_QUEUE,
  guardedEnd,
  phaseEnded,
  phaseEnd,
  saveMessage,
  queueMessage,
  restoredQueue,
  workStarted,
  type MessageQueue,
  type QueuedMessage,
} from "./messageQueue";
import { looksLikeQuestion } from "./promptGuard";
import { QueueDialog } from "./QueueDialog";
import { StallMonitor, stallThresholdMs, wallClock } from "./stall";
import { WaitMonitor, waitingIds, waitThresholdMs } from "./waitHint";
import { loadProjectItems, searchNoteItems } from "./launcherSearch";
import { isOneLine, toggleOneLine } from "./oneLineRows";
import type { SearchHit } from "./notes";
import { setSystemUsageEnabled } from "./useSystemUsage";
import { sessionLabels } from "./sessionLabel";
import {
  doneFileCount,
  DONE_NOTICE_MS,
  finishedWork,
  offersDoneNotice,
  type DoneNotice,
} from "./doneNotice";
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
import { startOptions } from "./startOptions";
import { resolveStartTarget, runningProjects, startPlaces, worktreeChance } from "./startTarget";
import {
  DEFAULT_START_MODE,
  effectiveMode,
  sessionMode,
  startModeFor,
  type StartMode,
} from "./startMode";
import { offersTeam, showsSupportLink, teamAccess } from "./teamPlan";
import { useAccount } from "./useAccount";
import {
  backgroundSessions,
  initialSessions,
  moveItem,
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
import {
  legacyMainPrefs,
  loadMainPrefs,
  loadProtectedSettings,
  loadSecuritySwitches,
  loadRecentSessions,
  loadStoredTemplates,
  migrateQueuedMessages,
  saveMainPrefs,
  saveProtectedSettings,
  saveRecentSessions,
  saveStoredTemplates,
  type MainPrefs,
} from "./mainStore";
import {
  browserStorage,
  markSeeded,
  needsSeed,
  pushRecent,
  recentEntry,
  seedRecent,
  shownRecent,
  UNKNOWN_TIME,
  withoutRecent,
  type RecentSession,
} from "./recentlyOpened";
import {
  gateActive,
  hasStoredSettings,
  legacyProtected,
  legacySwitches,
  loadSettings,
  noticesOf,
  protectedOf,
  readStoredSettingsText,
  saveSettings,
  switchesOf,
  type SecuritySwitches,
  type Settings,
} from "./settings";
import { SettingsDialog } from "./SettingsDialog";
import type { SettingsSection } from "./settingsSections";
import {
  focusCycleStep,
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
  nextFocusZone,
  SHORTCUT_LABELS,
  type AppAction,
  type Shortcut,
} from "./shortcuts";
import { barTitle, MainBar } from "./MainBar";
import { Sidebar } from "./Sidebar";
import { useSidebarMotion, useWindowResizing } from "./sidebarMotion";
import { useSidePanelMotion } from "./sidePanelMotion";
import { Splitter } from "./Splitter";
import { badgeKind, TaskbarBadge } from "./taskbarBadge";
import { TaskbarFlasher } from "./taskbarFlash";
import { trayLabels, TrayState } from "./tray";
import { SessionTerminal, setDictationKey, setFocusCycleActive, terminals } from "./terminal";
import { busySession, pasteForm } from "./dictation";
import { DictationIndicator, type IndicatorMode } from "./DictationIndicator";
import { useDictationKey, useDictationStatus, type DictationProblem } from "./useDictation";
import { openLink } from "./terminalLinkOpen";
import { takesSnapshots, timelineAvailability } from "./timeline";
import { fontStack, schemeColors, type TerminalLook } from "./terminalLook";
import { osc8DecisionOf } from "./termSafety";
import { EmptyPane, paneStyle, TerminalPane } from "./TerminalPane";
import { PaneSplitters } from "./PaneSplitters";
import { ratiosFor, sameRatios, SUB_AGENT_LAYOUT, subAgentRatios } from "./paneRatios";
import type { PanePlacement } from "./paneGlyph";
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
import { subAgentsNow, useSubAgentSummary, useSubAgentWatch } from "./useSubAgents";
import {
  dismissedIn,
  dismissFinished,
  visibleAgents,
  watchedSession,
  type Dismissed,
} from "./subAgents";
import { SubAgentPane } from "./SubAgentPane";
import { crewWatchWanted, waitingSubAgents, waitsForDecision } from "./crew";
import { useUpdater } from "./useUpdater";
import { useClaims } from "./useClaims";
import { useTeam } from "./useTeam";
import { TeamWindow } from "./TeamWindow";
import {
  clockTime,
  freshTaskNotices,
  freshTeammateOverlaps,
  personTag,
  releaseRequestKey,
  releaseRequests,
  rememberSeen,
  taskNoticeText,
  taskTitle,
  sessionStates,
  teammateMarkerText,
  teammateNotice,
  teammateNoticeDue,
  teammateOverlapKey,
  teammateOverlapsBySession,
  type TeammateOverlap,
} from "./team";
import { gateReady, useGate } from "./useGate";
import { withPermission } from "./permissionWaits";
import { secretLabels } from "./secretScan";
import { usePermissionWaits } from "./usePermissionWaits";
import { useRuleNotes } from "./useRuleNotes";
import {
  decisionText,
  decisionToShow,
  waitingSessions,
  withGate,
  type GateAnswer,
  type PendingDecision,
} from "./gate";
import { GateCard, GateRuleNote } from "./GateCard";
import { GlyphBackground } from "./GlyphBackground";
import { HANGAR_BUILD } from "./buildInfo";
import { withBuild } from "./version";
import { VersionText } from "./VersionText";
import { WhatsNewDialog } from "./WhatsNewDialog";
import { ChangesPanel } from "./ChangesPanel";
import { COMMIT_PAGE, parseGitLog, type ChangesView, type CommitFocus } from "./gitHistory";
import { OverviewDialog, type OverviewTile } from "./OverviewDialog";
import { WorktreeCleanupDialog } from "./WorktreeCleanupDialog";
import { cleanupFolders } from "./worktreeCleanup";
import { ProjectOverview } from "./ProjectOverview";
import { DailyReviewDialog } from "./DailyReviewDialog";
import { PhaseRecorder, type WorkPhase } from "./dailyReview";
import { tailLines } from "./overview";
import { PEEK_LINES } from "./peek";
import { edgeClass, rowEdge, runsAgent, sessionChip } from "./sessionChip";
import { SessionSearchDialog } from "./SessionSearchDialog";
import type { LineMatch } from "./sessionSearch";
import { useActivityRecorder } from "./activityStore";
import { localSessionsOf } from "./fileActivity";
import { FilesPanel, type PathDrag } from "./FilesPanel";
import { mentionText } from "./fileTree";
import { fileOf, forgetFile, rememberFile } from "./fileContext";
import { parseGitDiff } from "./gitDiff";
import {
  capDiff,
  expandVariables,
  missingSummary,
  usedVariables,
  type SnippetVariable,
  type VariableValues,
} from "./snippetVariables";
import {
  deleteTemplate,
  planTemplate,
  renameTemplate,
  storeTemplate,
  templateFolders,
  templateReport,
  templateSession,
  type TemplateLink,
  type TemplateSession,
  type WorkspaceTemplate,
} from "./templates";
import {
  RenameTemplateDialog,
  SaveTemplateDialog,
  type TemplateCandidate,
} from "./TemplateDialogs";
import { ClaimNoticeView, TeammateNoticeView } from "./ClaimsView";
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
  queuedMessagesOf,
  restorable,
  sameWorkspace,
  scrollbackDue,
  type SavedSession,
  type Workspace,
} from "./workspace";

/** How long "Claude Code starts in hangar" stays after Ctrl+Shift+T. */
const START_HINT_MS = 2500;
/** How long a successful update result stays. */
const UPDATE_RESULT_MS = 12_000;
/** Initial size until xterm has measured the real space (a resize follows right after). */
const INITIAL_SIZE = { cols: 120, rows: 30 };
const COPY_KEYS: Shortcut = { mods: ["ctrl"], key: "C" };
const PASTE_KEYS: Shortcut = { mods: ["ctrl"], key: "V" };

/**
 * A worktree made before a session starts. `fresh`: made for this start (the arena's), so it is
 * taken away again when the session does not run; a reused one (a handover) never is.
 */
type PreparedWorktree = { worktree: Worktree; fresh: boolean; onStarted?: (id: number) => void };

type OpenDialog =
  | "about"
  | "settings"
  | "whatsNew"
  | "launcher"
  | "broadcast"
  | "searchAll"
  | "overview"
  | "costs"
  | "arena"
  | "worktreeCleanup"
  | "dailyReview"
  | null;

/**
 * Working phases of the sessions for the work log (ADR 0011): one event when a phase ends,
 * never per output chunk. Module wide like `terminals`: one window, one recorder.
 */
const phases = new PhaseRecorder();

function reportPhase(phase: WorkPhase | null): Promise<void> {
  if (phase === null) return Promise.resolve();
  // The log is a convenience; a failed write must not disturb the session.
  return api.logWorkPhase(phase.id, phase.from, phase.to).then(
    () => undefined,
    () => undefined,
  );
}

/** Enter after a broadcast paste comes separately, so TUIs do not take it as part of the paste. */
const BROADCAST_ENTER_DELAY_MS = 80;
/** How often the arena looks whether a contestant is ready for its task. */
const ARENA_POLL_MS = 500;

/** The running arena once its sessions started; its clock starts now. */
function arenaRun(setup: ArenaSetup, input: ArenaInput, contestants: Contestant[]): ArenaRun {
  return {
    id: setup.id,
    baseCommit: setup.baseCommit,
    repoRoot: setup.repoRoot,
    prompt: input.prompt,
    testCommand: input.testCommand,
    startedAt: Date.now(),
    contestants,
  };
}
/** Snippets listed directly in the terminal context menu; more are in quick access. */
const MENU_SNIPPETS = 5;

/** Why a snippet variable has no value, for the notice that stops sending. */
const MISSING_REASONS: Record<SnippetVariable, MessageKey> = {
  file: "snippets.missingFile",
  branch: "snippets.missingBranch",
  selection: "snippets.missingSelection",
  diff: "snippets.missingDiff",
  folder: "snippets.missingFolder",
};

/** A text with its variables filled for one session, or what is missing there. */
type Filled = { ok: true; text: string } | { ok: false; details: string };

/** The card title of Hangar's introduction of a link group, by its kind (linkIntro.ts). */
const INTRO_TITLES: Record<IntroKind, MessageKey> = {
  linked: "links.intro",
  rolesChanged: "links.introRoles",
  joined: "links.introJoined",
};

/** Debounced workspace save: several changes in quick succession result in one write. */
const WORKSPACE_SAVE_DELAY_MS = 400;
/** The usage statistics take the settings after they stood still this long. */
const USAGE_SETTINGS_DELAY_MS = 2000;
/**
 * Save the scrollback of changed sessions regularly so that a crash costs little: at most this
 * much output is lost. Only sessions with new output that have been quiet for a moment are
 * written (`saveScrollbacks`), so idle sessions cost nothing and a busy one at most one file per
 * interval.
 */
const SCROLLBACK_SAVE_INTERVAL_MS = 10_000;
/** Quitting waits no longer than this for the save. */
const QUIT_SAVE_TIMEOUT_MS = 3000;
/** Quit waits at most this long for the taskbar badge to be removed. */
const BADGE_CLEAR_TIMEOUT_MS = 300;
/** Restore: delay between the starts of the remaining sessions after the active one. */
const RESTORE_STAGGER_MS = 150;
/** One stable empty list, so a session without overlaps does not re-render its panel. */
const NO_OVERLAPS: readonly Overlap[] = [];
const NO_TEAMMATES: readonly TeammateOverlap[] = [];
/** Session states reach the team's presence at most this often (state changes come in bursts). */
const TEAM_STATES_DELAY_MS = 1000;

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
  /** Private or team session for the next start, as the switch shows it (startMode.ts). */
  const [startMode, setStartMode] = useState<StartMode>(DEFAULT_START_MODE);
  const accountStore = useAccount();
  /** Team sessions only for people in a team (teamPlan.ts); Free is for one person. */
  const teamOffered = offersTeam(teamAccess(accountStore.view, accountStore.projects));
  const [home, setHome] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [layout, setLayout] = useState<Layout>(loadLayout);
  /** The app grid; its sidebar column glides when the sidebar folds (sidebarMotion.ts). */
  const appGrid = useRef<HTMLDivElement>(null);
  useSidebarMotion(appGrid, layout.collapsed, layout.sidebarWidth);
  useWindowResizing();
  /** What an older version stored of the security switches, read before anything saves. */
  const [oldSwitches] = useState(() => legacySwitches(readStoredSettingsText()));
  const [settings, setSettings] = useState<Settings>(() =>
    loadSettings(detectLanguage(navigator.language)),
  );
  /**
   * The security switches as the backend keeps them (mainStore.ts); `null` until they are read.
   * Until then the gate and the command guard are not configured.
   */
  const savedSwitches = useRef<SecuritySwitches | null>(null);
  const [switchesReady, setSwitchesReady] = useState(false);
  useEffect(() => {
    void loadSecuritySwitches(oldSwitches).then((switches) => {
      savedSwitches.current = switches;
      setSettings((current) => ({ ...current, ...switches }));
      setSwitchesReady(true);
    });
  }, [oldSwitches]);
  /** What an older version stored of the protected settings, read before anything saves. */
  const [oldProtected] = useState(() =>
    legacyProtected(readStoredSettingsText(), detectLanguage(navigator.language)),
  );
  /**
   * The protected settings (paste warning, updates, HANGAR.md, snippets) as the backend keeps
   * them (mainStore.ts), as JSON for the comparison; `null` until they are read. Sessions are
   * restored only afterwards, so they start with the user's HANGAR.md choice.
   */
  const savedProtected = useRef<string | null>(null);
  const [protectedReady, setProtectedReady] = useState(false);
  useEffect(() => {
    void loadProtectedSettings(oldProtected, detectLanguage(navigator.language)).then((stored) => {
      savedProtected.current = JSON.stringify(stored);
      setSettings((current) => ({ ...current, ...stored }));
      setProtectedReady(true);
    });
  }, [oldProtected]);
  /** What an older version stored of the start and notice preferences, read before any save. */
  const [oldPrefs] = useState(() => {
    let storage: Storage | null = null;
    try {
      storage = localStorage;
    } catch {
      // Blocked storage: nothing to migrate.
    }
    return legacyMainPrefs(storage, readStoredSettingsText());
  });
  /**
   * The start and notice preferences as the backend keeps them (mainStore.ts), as JSON for the
   * comparison; `null` until they are read. Sessions are restored only afterwards, so the restore
   * setting and the start folder are the user's, not the defaults.
   */
  const savedPrefs = useRef<string | null>(null);
  const [prefsReady, setPrefsReady] = useState(false);
  const grouping = settings.sessionGrouping;
  // The session list follows the sidebar's grouping, so cycling and the rail match it.
  useEffect(() => {
    dispatch({ type: "regrouped", by: grouping });
  }, [grouping]);
  const [version, setVersion] = useState<string | null>(null);
  const [profilePath, setProfilePath] = useState<string | null>(null);
  const [dialog, setDialog] = useState<OpenDialog>(null);
  /** The section Settings opens on; `null`: the one chosen last. */
  const [settingsSection, setSettingsSection] = useState<SettingsSection | null>(null);
  /** The Team window floats over the terminals and is not modal, so it is not a `dialog`. */
  const [teamOpen, setTeamOpen] = useState(false);
  /** What's New starting at this version; `null` shows all entries (opened from the menu). */
  const [whatsNewSince, setWhatsNewSince] = useState<string | null>(null);
  /** The changelog, once What's New needed it (changelogData.ts). */
  const [changelog, setChangelog] = useState<readonly ChangelogEntry[] | null>(null);
  // Read before the settings are saved for the first time: has Hangar run here before?
  const [knownUser] = useState(hasStoredSettings);
  const [renamingId, setRenamingId] = useState<number | null>(null);
  /** The running agent arena (arena.ts); one at a time, kept in memory only. */
  const [arena, setArena] = useState<ArenaRun | null>(null);
  const arenaRef = useRef(arena);
  useEffect(() => {
    arenaRef.current = arena;
  }, [arena]);
  /** When each contestant's session started (performance.now), for promptReady. */
  const arenaStarts = useRef(new Map<number, number>());
  /** Contestants whose task went out, so the poll never sends it twice. */
  const arenaSent = useRef(new Set<number>());
  /** Attention changes of every session reach the arena here (set once the arena code exists). */
  const arenaAttention = useRef<(id: number, next: AttentionState) => void>(() => undefined);
  /** What the terminals' trackers report; `attention` below adds the gate's waiting sessions. */
  const [trackedAttention, setAttention] = useState<ReadonlyMap<number, AttentionState>>(
    () => new Map(),
  );
  /** Why a waiting session waits (attention.ts); only sessions in "attention" have an entry. */
  const [trackedReasons, setReasons] = useState<ReadonlyMap<number, AttentionReason>>(
    () => new Map(),
  );
  /** Profile list loaded once (or failed): only then does the restore run. */
  const [profilesReady, setProfilesReady] = useState(false);
  /** Restore finished; nothing is saved before that, otherwise an empty start would overwrite the saved state. */
  const [restored, setRestored] = useState(false);
  /** Startup finished (restored or failed): only then do the program queries start. */
  const [startupDone, setStartupDone] = useState(false);
  /** Sessions in the backend that the UI does not know about after a reload. */
  const [orphans, setOrphans] = useState<SessionInfo[]>([]);
  /** What the restore brought back after a run that did not quit cleanly (crashRestore.ts). */
  const [crashRestore, setCrashRestore] = useState<RestoredSession[] | null>(null);
  const closeCrashNotice = useCallback(() => {
    setCrashRestore(null);
  }, []);
  /** Session whose closing is waiting for confirmation. */
  const [closingId, setClosingId] = useState<number | null>(null);
  /** Context menu of the terminal, open at this position in the window. */
  const [terminalMenu, setTerminalMenu] = useState<{ x: number; y: number } | null>(null);
  /** "Save as snippet": the cleaned selection the dialog starts with; `null` while closed. */
  const [snippetDraft, setSnippetDraft] = useState<string | null>(null);
  /** Most recently started program (Ctrl+Shift+T, empty state). */
  const [lastProgramId, setLastProgramId] = useState<string | null>(null);
  /** Clicked terminal link, for as long as its hint is shown. */
  /**
   * The link notice: an address to copy when the browser could not open it, or, with `ask`, an
   * OSC 8 link whose visible text claims another address, shown with an Open button.
   */
  const [link, setLink] = useState<{ url: string; copied: boolean; ask?: boolean } | null>(null);
  /** Newest local dev server address per session, from its current process (devServer.ts). */
  const [devServers, setDevServers] = useState<ReadonlyMap<number, string>>(() => new Map());
  /** An agent session just finished with changed files in its folder (doneNotice.ts). */
  const [doneNotice, setDoneNotice] = useState<DoneNotice | null>(null);
  /**
   * The done notice under the pointer or keyboard focus: it stays until let go. Held by identity,
   * so a notice that vanished while held does not keep the next one.
   */
  const [heldDoneNotice, setHeldDoneNotice] = useState<DoneNotice | null>(null);
  /** Whether the Changes tab shows in the right panel; read by the attention listener. */
  const changesShownRef = useRef(false);
  /** Text from the app waiting for "Send anyway" (appPaste.ts); the answer goes to the ref. */
  const [pasteCheck, setPasteCheck] = useState<{ warning: PasteWarning; name: string } | null>(
    null,
  );
  const pasteAnswer = useRef<((send: boolean) => void) | null>(null);
  const pasteCancelButton = useRef<HTMLButtonElement | null>(null);
  const viewportWidth = useSyncExternalStore(subscribeWindowWidth, windowWidth);
  const [noteFolders, setNoteFolders] = useState<NoteFolderInfo[]>([]);
  /** Favorite and recent working folders ("Starts in" menu). */
  const [folders, setFolders] = useState<FolderPrefs>(EMPTY_FOLDERS);
  /** The day and level the budget notice last showed, kept by the backend with the preferences. */
  const [budgetWarned, setBudgetWarned] = useState<BudgetWarned | null>(null);
  /** The day the morning summary was last offered for, kept by the backend too. */
  const [morningOfferedDay, setMorningOfferedDay] = useState<string | null>(null);
  /** The day the daily review opens on when the morning summary asked for it. */
  const [reviewDay, setReviewDay] = useState<Date | undefined>(undefined);
  useEffect(() => {
    void loadMainPrefs(oldPrefs).then((stored) => {
      const {
        startMode: mode,
        lastProgram,
        favorites,
        recent,
        budgetWarned,
        morningOfferedDay,
        ...notices
      } = stored;
      savedPrefs.current = JSON.stringify(stored);
      setStartMode(mode);
      setLastProgramId(lastProgram);
      setFolders({ favorites, recent });
      setBudgetWarned(budgetWarned);
      setMorningOfferedDay(morningOfferedDay);
      setSettings((current) => ({ ...current, ...notices }));
      setPrefsReady(true);
    });
  }, [oldPrefs]);
  /** Workspace templates (templates.ts), kept by the backend for this window (mainStore.ts). */
  const [templates, setTemplates] = useState<WorkspaceTemplate[]>([]);
  /** The list the backend holds; the templates are saved only once it was read, and changed. */
  const savedTemplates = useRef<WorkspaceTemplate[] | null>(null);
  useEffect(() => {
    void loadStoredTemplates().then((stored) => {
      savedTemplates.current = stored;
      setTemplates(stored);
    });
  }, []);
  /** Recently opened sessions (recentlyOpened.ts), kept by the backend for this window. */
  const [recentSessions, setRecentSessions] = useState<RecentSession[]>([]);
  /** The list the backend holds; saved only once it was read, and changed. */
  const savedRecent = useRef<RecentSession[] | null>(null);
  /** The list is read once, after the preferences and programs (see `seedProgram`). */
  const recentRequested = useRef(false);
  useEffect(() => {
    if (savedRecent.current === null || savedRecent.current === recentSessions) return;
    savedRecent.current = recentSessions;
    saveRecentSessions(recentSessions).catch(() => undefined);
  }, [recentSessions]);
  /** A recently opened session is starting again; a second click waits until it runs. */
  const reopening = useRef(false);
  /** A template is starting its sessions; another start waits until it is done. */
  const templateStarting = useRef(false);
  /** Save or rename dialog of the templates. */
  const [templateDialog, setTemplateDialog] = useState<
    { kind: "save" } | { kind: "rename"; id: string } | null
  >(null);
  /** Search bar of a session; `token` changes on every Ctrl+Shift+F to refocus the field. */
  const [search, setSearch] = useState<{ sessionId: number; token: number } | null>(null);
  /** Pane under files dragged from the file manager. */
  const [dropTarget, setDropTarget] = useState<number | null>(null);
  /** Session whose marker is being edited. */
  const [markerId, setMarkerId] = useState<number | null>(null);
  /** The session whose "Hand over to" dialog is open. */
  const [handoverId, setHandoverId] = useState<number | null>(null);
  /** The session whose "Send to partner" dialog is open. */
  const [partnerId, setPartnerId] = useState<number | null>(null);
  /** A partner message that did not arrive: the text stays copyable until dismissed. */
  const [unsentPartner, setUnsentPartner] = useState<{ text: string; message: string } | null>(
    null,
  );
  /** The partner projects (ADR 0012), as the backend keeps them. */
  const [partnerBook, setPartnerBook] = useState<PartnerBook>(EMPTY_BOOK);
  /** Messages agents of partner projects sent that wait for the person (partnerInbox.ts). */
  const [waitingMessages, setWaitingMessages] = useState<WaitingMessage[]>([]);
  /** Waiting messages whose send from their card runs right now (messageCard.ts). */
  const [inboxSends, setInboxSends] = useState<ReadonlyMap<number, CardSend>>(() => new Map());
  const [inboxRuns] = useState(() => new CardSends(setInboxSends));
  /** Waiting messages the person discarded; hidden until the backend no longer lists them. */
  const [hiddenWaiting, setHiddenWaiting] = useState<ReadonlySet<number>>(() => new Set());
  /** The waiting messages for the delivery loops, which outlive a render. */
  const waitingRef = useRef(waitingMessages);
  useEffect(() => {
    waitingRef.current = waitingMessages;
  });
  /** The waiting messages already seen, so an arrival is announced once. */
  const seenWaiting = useRef<Set<number> | null>(null);
  /** Link groups, linkable sessions and their messages (ADR 0014), as the backend keeps them. */
  const [linkState, setLinkState] = useState<LinkState>(EMPTY_LINKS);
  /** The open link dialog: link a session with others, edit a group, or read its history. */
  const [linkDialog, setLinkDialog] = useState<
    | { kind: "link"; id: number }
    | { kind: "edit"; group: number }
    | { kind: "history"; group: number }
    | null
  >(null);
  /** Held link messages whose send from their card runs right now (messageCard.ts). */
  const [linkSends, setLinkSends] = useState<ReadonlyMap<number, CardSend>>(() => new Map());
  const [linkRuns] = useState(() => new CardSends(setLinkSends));
  /** Held link messages the person discarded; hidden until the backend no longer lists them. */
  const [hiddenLinks, setHiddenLinks] = useState<ReadonlySet<number>>(() => new Set());
  /** Targets whose automatic delivery runs right now: one message at a time per target. */
  const linkDelivering = useRef<Set<number>>(new Set());
  /**
   * Messages whose automatic delivery runs right now. A slow paste can outlast the backend's claim
   * (links.rs `CLAIM_TIMEOUT_MS`); such a message gets no card and no click, or it went in twice.
   */
  const linkDeliveringIds = useRef<Set<number>>(new Set());
  /** The same ids for the render, which must not read a ref. */
  const [deliveringIds, setDeliveringIds] = useState<ReadonlySet<number>>(() => new Set());
  /** The link groups for listeners outside React (the terminals' key handlers). */
  const linkStateRef = useRef(linkState);
  useEffect(() => {
    linkStateRef.current = linkState;
  });
  /** A handover text that was not pasted (the new agent never turned bracketed paste on). */
  const [unpastedHandover, setUnpastedHandover] = useState<{
    text: string;
    program: string;
  } | null>(null);
  /** Session menu (right click on a session), open at this position. */
  const [sessionMenu, setSessionMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  /** The commands submenu of a session (session menu, "Commands …"), at the menu's place. */
  const [commandMenu, setCommandMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  /** A command with `confirm` that waits for the answer of its dialog. */
  const [commandAsk, setCommandAsk] = useState<{ id: number; command: AgentCommand } | null>(null);
  /** When each session last got a compact from its row button (the click's `timeStamp`). */
  const compactSentAt = useRef(new Map<number, number>());
  /**
   * Queued messages per session key (messageQueue.ts). The ref is the truth the attention
   * listener reads and writes outside React; the state renders it.
   */
  const [queue, setQueue] = useState<MessageQueue>(EMPTY_QUEUE);
  const queueRef = useRef<MessageQueue>(queue);
  const changeQueue = useCallback((next: MessageQueue) => {
    if (next === queueRef.current) return;
    queueRef.current = next;
    setQueue(next);
  }, []);
  /** Takes the messages the saved workspace holds (on restore, or remembered before a reload). */
  const restoreQueue = useCallback(
    (workspace: Workspace) => {
      // A message queued while the backend answered wins over the stored one.
      changeQueue(new Map([...restoredQueue(queuedMessagesOf(workspace)), ...queueRef.current]));
    },
    [changeQueue],
  );
  /** The queued message editor: for which session, and where the menu was (`null`: centered). */
  const [queueEditor, setQueueEditor] = useState<{
    id: number;
    anchor: { x: number; y: number } | null;
  } | null>(null);
  /** Sends a queued message once its session is done; set below, read by the listener. */
  const sendQueuedRef = useRef<(id: number, text: string) => void>(() => undefined);
  /** "Show last lines" from the session menu, for the sidebar's peek. */
  const [peekRequest, setPeekRequest] = useState<{ id: number; token: number } | null>(null);
  /** Working agent sessions that have shown nothing new for a while, and since when (stall.ts). */
  const [stalls, setStalls] = useState<ReadonlyMap<number, number>>(() => new Map());
  /** Sessions that have waited for the person past the threshold, and since when (waitHint.ts). */
  const [longWaits, setLongWaits] = useState<ReadonlyMap<number, number>>(() => new Map());
  /** Quick access chose a project: its card in the overview scrolls into view; a new token again. */
  const [projectFocus, setProjectFocus] = useState<{ key: string; token: number } | null>(null);
  /**
   * Session whose sub-agents the body shows (sub-agent view), or `null`. Only while that session
   * is the active one; switching sessions or arranging the panes ends the view.
   */
  const [subAgentsOf, setSubAgentsOf] = useState<number | null>(null);
  /** Sub-agent whose activity the view shows; `null`: the list of all of them. */
  const [subAgentPick, setSubAgentPick] = useState<string | null>(null);
  /**
   * Finished sub-agents removed from the tree and the list, per session and conversation. Only
   * in memory: a view preference for this run of Hangar, while the transcripts stay untouched.
   */
  const [dismissedSubAgents, setDismissedSubAgents] = useState<Dismissed>(() => new Map());
  /** Layout picker (View, quick access) for the active session; `preview` is its chosen zone. */
  const [snapPicker, setSnapPicker] = useState<{ preview: SnapTarget | null } | null>(null);
  /**
   * The open sessions are put aside: the body shows the home view (calendar) while every session
   * keeps running. Picking a session in the sidebar brings them back.
   */
  const [putAside, setPutAside] = useState(false);
  const homeShown = state.sessions.length === 0 || putAside;
  /**
   * How often each session finished a stretch of work the user asked for: the project overview
   * asks git again for a card when one of its sessions finished, and only then.
   */
  const [finishedRuns, setFinishedRuns] = useState<ReadonlyMap<number, number>>(() => new Map());
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
      if (kind === "launcher") {
        // Rejecting lets the badge send the count again on its next change.
        const sent = await api.setLauncherCount(text === null ? 0 : count);
        if (sent !== true) throw new Error("launcher count not sent");
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
  /** Goes up when the panel window changed notes: the docked panel reloads. */
  const [notesRefresh, setNotesRefresh] = useState(0);
  const notesHandle = useRef<NotesHandle | null>(null);
  /** The right panel in its own window, kept on top; remembered per machine (ADR 0007). */
  const [panelWindow, setPanelWindow] = useState<PanelWindowPrefs>(loadWindowPrefs);
  /** The answer about the anonymous usage statistics; `undefined` until the backend told it. */
  const [usageConsent, setUsageConsent] = useState<UsageConsent | undefined>(undefined);
  /** Changes tab of the right panel (Ctrl+Shift+D): focus on opening. */
  const [changesFocusToken, setChangesFocusToken] = useState(0);
  /** Uncommitted changes or commits; kept for this window while the tab is closed. */
  const [changesView, setChangesView] = useState<ChangesView>("open");
  /** A commit chosen in quick access, for the history to open. */
  const [commitFocus, setCommitFocus] = useState<CommitFocus | null>(null);
  /** Start term of the search across sessions: the selection when it opened. */
  const [searchAllSeed, setSearchAllSeed] = useState("");
  const [filesFocusToken, setFilesFocusToken] = useState(0);
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
  /** The newest notice about the person's reservations ("Anna handed it over to you"). */
  const [taskNotice, setTaskNotice] = useState<{ text: string; key: number } | null>(null);
  /** What stayed after closing a worktree session, with its path to copy. */
  const [worktreeNotice, setWorktreeNotice] = useState<
    (WorktreeNotice & { copied: boolean }) | null
  >(null);
  /** A new collision of two sessions on one file (claims.ts), until dismissed or replaced. */
  const [claimNotice, setClaimNotice] = useState<{ overlap: Overlap; key: number } | null>(null);
  /** A new collision with a teammate's session, until dismissed or replaced. */
  const [teamNotice, setTeamNotice] = useState<{ overlap: TeammateOverlap; key: number } | null>(
    null,
  );
  /** Gate decisions answered here whose snapshot has not caught up yet: hidden at once. */
  const [answeredGate, setAnsweredGate] = useState<ReadonlySet<number>>(() => new Set());
  /** An answer that did not reach the backend, shown on its card. */
  const [gateError, setGateError] = useState<{ id: number; text: string } | null>(null);
  /** Above 0 while Ctrl+Shift+A wants the focus on the decision card; the card resets it. */
  const [gateFocusToken, setGateFocusToken] = useState(0);
  const updater = useUpdater(version, settings.autoUpdate);
  const installed = installedKey(profiles);
  const programs = usePrograms(installed, settings.showVersions, settings.showIcons, startupDone);
  const programUpdates = useProgramUpdates(
    installed,
    settings.checkProgramUpdates,
    programs.versionsSettled && startupDone,
    programs.refreshVersions,
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
  /** A push to the phone once a session has stayed waiting, done or failed (phonePush.ts). */
  const [pushes] = useState(() => new PushScheduler());
  useEffect(() => {
    pushes.setSender((id, event) => {
      const current = settingsRef.current;
      // Quiet hours hold pushes back like every other notice; nothing is sent later.
      if (!current.attentionEnabled || isQuietTime(current.quietHours, new Date())) return;
      if (!stateRef.current.sessions.some((s) => s.id === id)) return;
      // Without parameters the "{name}" stays in the sentence: the backend names the session.
      const template = createTranslator(current.language)(PUSH_TEXTS[event]);
      api.pushSession(id, event, template).catch(() => undefined);
    });
    return () => {
      pushes.setSender(null);
    };
  }, [pushes]);
  // The sessions' CPU and memory and the machine's system bar: both stores ask the backend only
  // while this is on.
  useEffect(() => {
    setResourcesEnabled(settings.showResources);
    setSystemUsageEnabled(settings.showResources);
  }, [settings.showResources]);
  // Power saving on battery: slower polls and still row pulses (power.ts); on AC nothing changes.
  useEffect(() => {
    setPowerSavingEnabled(settings.powerSaving);
    startPowerWatch();
  }, [settings.powerSaving]);
  // Whether the bundled figures play their idle loop (useFigureMotion.ts).
  useEffect(() => {
    setFigureMotion(settings.figureMotion);
  }, [settings.figureMotion]);
  const profilesRef = useRef(profiles);
  /**
   * The stall hint (stall.ts): samples the screens of working agent sessions outside React and
   * reports only when a hint comes or goes. Plain shells and sessions without attention never get
   * one.
   */
  const [stallMonitor] = useState(() => new StallMonitor(wallClock, setStalls));
  useEffect(() => {
    stallMonitor.setSources(() =>
      stateRef.current.sessions.map((session) => {
        const terminal = terminals.get(session.id);
        const command = profilesRef.current.find((p) => p.id === session.profileId)?.command;
        return {
          id: session.id,
          working: terminal?.attention.state === "working",
          detected:
            settingsRef.current.attentionEnabled &&
            session.status === "running" &&
            !isPlaceholder(session.id) &&
            runsAgent(command, session.kind),
          lines: () => terminal?.screenLines() ?? [],
        };
      }),
    );
  }, [stallMonitor]);
  useEffect(() => {
    stallMonitor.setThreshold(
      settings.attentionEnabled ? stallThresholdMs(settings.stallMinutes) : null,
    );
  }, [stallMonitor, settings.attentionEnabled, settings.stallMinutes]);
  // On battery the screens are compared less often (power.ts `pollInterval`).
  useEffect(() => {
    const follow = () => {
      stallMonitor.setSampleInterval(pollInterval("stall", isPowerSaving()));
    };
    follow();
    return onPowerSavingChange(follow);
  }, [stallMonitor]);
  // CPU use of a session's process tree counts as busy; without readings only the screen counts.
  useEffect(() => {
    const stop = observeResources((readings) => {
      for (const [id, reading] of readings) stallMonitor.cpu(id, reading.cpu);
    });
    return () => {
      stop();
      stallMonitor.dispose();
    };
  }, [stallMonitor]);
  /**
   * The wait hint (waitHint.ts): learns each change of state from the attention listener and
   * reports when a session has waited past the threshold. Crossing it in the background flashes
   * the taskbar once, under the same settings as a session that starts waiting.
   */
  const [waitMonitor] = useState(() => new WaitMonitor(wallClock, setLongWaits));
  useEffect(() => {
    waitMonitor.setOnCross(() => {
      const notices = noticesAt(settingsRef.current, new Date());
      flasher.attentionChanged("attention", {
        enabled: notices.taskbarFlash && notices.attentionEnabled,
        windowFocused: document.hasFocus(),
      });
    });
  }, [waitMonitor, flasher]);
  useEffect(() => {
    waitMonitor.setThreshold(
      settings.attentionEnabled ? waitThresholdMs(settings.waitMinutes) : null,
    );
  }, [waitMonitor, settings.attentionEnabled, settings.waitMinutes]);
  useEffect(
    () => () => {
      waitMonitor.dispose();
    },
    [waitMonitor],
  );
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

  useEffect(() => {
    api
      .telemetryStatus()
      .then((value) => {
        const consent = parseUsageConsent(value);
        if (consent !== undefined) setUsageConsent(consent);
      })
      // Without an answer the question stays away; the switch waits as well.
      .catch(() => undefined);
  }, []);

  /** The person's answer, from the notice or the switch; off deletes what was counted. */
  const answerUsage = useCallback(
    (enabled: boolean) => {
      setUsageConsent(enabled);
      api
        .telemetrySetConsent(enabled)
        .then((value) => {
          const consent = parseUsageConsent(value);
          if (consent !== undefined) setUsageConsent(consent);
        })
        .catch((error: unknown) => {
          showError(error);
          api
            .telemetryStatus()
            .then((value) => {
              setUsageConsent(parseUsageConsent(value));
            })
            .catch(() => undefined);
        });
    },
    [showError],
  );

  const openUsageInfo = useCallback(() => {
    api.telemetryOpenInfo().catch(showError);
  }, [showError]);

  /**
   * Reads the waiting messages (on start, after each `partner-messages-changed` and after every
   * action on a card). Reads can overlap; an older answer never overwrites a newer one. A
   * discarded card stays hidden only until the list no longer has its message.
   */
  const loadWaitingRef = useRef(
    latestOnly(
      () => api.waitingMessages(),
      (value) => {
        const next = parseWaitingMessages(value);
        setWaitingMessages(next);
        setHiddenWaiting((hidden) =>
          pruneHidden(
            hidden,
            next.map((message) => message.id),
          ),
        );
      },
      (e: unknown) => {
        setError(errorText(e));
      },
    ),
  );

  /** Reads the link groups (on start, after each `link-groups-changed` and after card actions). */
  const loadLinksRef = useRef(
    latestOnly(
      () => api.linkState(),
      (value) => {
        const next = parseLinkState(value);
        setLinkState(next);
        setHiddenLinks((hidden) =>
          pruneHidden(
            hidden,
            next.messages.map((message) => message.id),
          ),
        );
      },
      (e: unknown) => {
        setError(errorText(e));
      },
    ),
  );

  // A claim the window never closed runs out in the backend (links.rs `CLAIM_TIMEOUT_MS`) without
  // an event: read again then, so its card comes back.
  const anyClaimed = linkState.messages.some((message) => message.claimed);
  useEffect(() => {
    if (!anyClaimed) return;
    const timer = window.setTimeout(() => {
      loadLinksRef.current();
    }, CLAIM_TIMEOUT_MS + 1000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [anyClaimed, linkState.messages]);

  // A new message no row can show (no agent session runs in its project) is named once; one a
  // row shows speaks for itself. After a restart that includes the messages that still wait.
  useEffect(() => {
    const seen = seenWaiting.current;
    seenWaiting.current = new Set(waitingMessages.map((message) => message.id));
    if (seen === null) return;
    const fresh = waitingMessages.filter((message) => !seen.has(message.id));
    if (fresh.length === 0) return;
    const rows = inboxBySession(
      fresh,
      partnerBook,
      state.sessions,
      (profileId) => profiles.find((profile) => profile.id === profileId)?.command,
    );
    const [first] = unshownMessages(fresh, rows);
    if (first !== undefined) setStartHint(t("inbox.arrived", { from: first.from, to: first.to }));
  }, [waitingMessages, partnerBook, state.sessions, profiles, t]);

  // A group whose loop brake turns on says so once; its messages then wait below the rows.
  const previousGroups = useRef(linkState.groups);
  useEffect(() => {
    if (newlyBraked(previousGroups.current, linkState.groups).length > 0) {
      setStartHint(t("links.braked"));
    }
    previousGroups.current = linkState.groups;
  }, [linkState.groups, t]);
  // The backend tells when the brake turns on, not when a quiet minute released it: ask again
  // then, so the hint does not outlast it.
  const anyBraked = linkState.groups.some((entry) => entry.braked);
  useEffect(() => {
    if (!anyBraked) return;
    const timer = window.setTimeout(() => {
      loadLinksRef.current();
    }, BRAKE_WINDOW_MS + 1000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [anyBraked, linkState.groups]);

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

  const openHangarMd = useCallback(async () => {
    await api.openHangarMd();
  }, []);

  useEffect(() => {
    loadProfiles(showError);
    api.defaultCwd().then(setHome).catch(showError);
    api
      .partnerProjects()
      .then((value) => {
        setPartnerBook(parsePartnerBook(value));
      })
      .catch(showError);
    loadWaitingRef.current();
    loadLinksRef.current();
    getVersion()
      .then(async (v) => {
        setVersion(withBuild(v, HANGAR_BUILD));
        // After an update, show once what's new. The version is stored right away so the
        // dialog does not come back even after a crash or reload. On an ordinary start the
        // decision needs no entries, and the changelog is not loaded.
        const lastSeen = loadLastSeen();
        const entries = whatsNewNeedsEntries(v, lastSeen, knownUser) ? await loadChangelog() : [];
        const decision = decideWhatsNew(entries, v, lastSeen, knownUser);
        if (decision.remember !== null) saveLastSeen(decision.remember);
        if (!decision.show) return;
        setChangelog(entries);
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

  // The switches go to the backend, the one place other windows cannot write to.
  useEffect(() => {
    if (!switchesReady) return;
    const next = switchesOf(settings);
    const saved = savedSwitches.current;
    if (
      saved !== null &&
      saved.claimsEnabled === next.claimsEnabled &&
      saved.gateEnabled === next.gateEnabled &&
      saved.commandGuardEnabled === next.commandGuardEnabled
    ) {
      return;
    }
    savedSwitches.current = next;
    api.saveSecuritySwitches(next).catch(() => undefined);
  }, [settings, switchesReady]);

  // The protected settings likewise.
  useEffect(() => {
    if (!protectedReady) return;
    const next = protectedOf(settings);
    const text = JSON.stringify(next);
    if (text === savedProtected.current) return;
    savedProtected.current = text;
    saveProtectedSettings(next).catch(() => undefined);
  }, [settings, protectedReady]);

  useEffect(() => {
    setCurrentLanguage(settings.language);
    document.documentElement.lang = settings.language;
  }, [settings.language]);

  useEffect(() => {
    applyDocumentTheme(theme);
    // The area around the terminals takes the scheme's background, so no frame shows; the
    // sub-agent log reads like a terminal with the scheme's text color and font.
    const colors = schemeColors(terminalLook.scheme, theme);
    const rootStyle = document.documentElement.style;
    rootStyle.setProperty("--terminal-bg", colors.background);
    rootStyle.setProperty("--terminal-fg", colors.foreground);
    rootStyle.setProperty("--terminal-font", fontStack(terminalLook.font));
    // Change options only: xterm repaints, content and scrollback stay.
    for (const terminal of terminals.values()) terminal.setLook(terminalLook, theme);
  }, [theme, terminalLook]);

  // The start and notice preferences go to the backend too, once they were read, and only when
  // they changed.
  useEffect(() => {
    if (!prefsReady) return;
    const next: MainPrefs = {
      startMode,
      lastProgram: lastProgramId,
      favorites: folders.favorites,
      recent: folders.recent,
      ...noticesOf(settings),
      budgetWarned,
      morningOfferedDay,
      dictationEnabled: settings.dictationEnabled,
    };
    const text = JSON.stringify(next);
    if (text === savedPrefs.current) return;
    savedPrefs.current = text;
    saveMainPrefs(next).catch(() => undefined);
  }, [prefsReady, startMode, lastProgramId, folders, settings, budgetWarned, morningOfferedDay]);

  // The settings a daily report shows, only while the statistics are on; a run of changes (a
  // slider, a dragged splitter) sends its last state once.
  const usageSnapshot = useMemo(
    () =>
      usageConsent === true && prefsReady
        ? usageSettings(settings, layout, panelWindow.detached)
        : null,
    [usageConsent, prefsReady, settings, layout, panelWindow.detached],
  );
  useEffect(() => {
    if (usageSnapshot === null) return;
    const timer = window.setTimeout(() => {
      api.telemetrySettings(usageSnapshot).catch(() => undefined);
    }, USAGE_SETTINGS_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [usageSnapshot]);

  // Only after the backend answered, and only a list that differs from the one it holds.
  useEffect(() => {
    if (savedTemplates.current === null || savedTemplates.current === templates) return;
    savedTemplates.current = templates;
    saveStoredTemplates(templates).catch(() => undefined);
  }, [templates]);

  useEffect(() => {
    modalRef.current =
      dialog !== null ||
      closingId !== null ||
      markerId !== null ||
      queueEditor !== null ||
      templateDialog !== null ||
      snippetDraft !== null ||
      commandAsk !== null;
  }, [dialog, closingId, markerId, queueEditor, templateDialog, snippetDraft, commandAsk]);

  useEffect(() => {
    orphansShown.current = orphans.length > 0;
  }, [orphans]);

  useEffect(() => {
    for (const terminal of terminals.values()) terminal.setScrollback(settings.scrollbackLines);
  }, [settings.scrollbackLines]);

  useEffect(() => {
    // Color and pulse per session state; edges and chips keep 3 : 1 in both themes.
    const style = document.documentElement.style;
    for (const [name, value] of Object.entries(lookVariables(settings.looks, theme))) {
      style.setProperty(name, value);
    }
  }, [settings.looks, theme]);

  useEffect(() => {
    // The flow on the tree guides: off, nothing is counted and nothing moves (treeFlowStore.ts).
    setTreeFlow(settings.treeFlow);
  }, [settings.treeFlow]);

  useEffect(() => {
    // How the working dots move (styles.css, `:root[data-dots]`).
    document.documentElement.dataset.dots = settings.dotAnimation;
  }, [settings.dotAnimation]);

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
    const current = noticesAt(settingsRef.current, new Date());
    if (!current.notifyDesktop || !current.attentionEnabled || document.hasFocus()) return;
    const name = (id: number) =>
      stateRef.current.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
    const text = overlapNotice(first, name, createTranslator(current.language));
    api.notifySession(first.sessionId, text.title, text.body).catch(() => undefined);
  });
  // The gate: a pending decision plays the chime, flashes the taskbar and sends a desktop
  // notification once, under the same settings as a session that starts waiting.
  // Not configured before the switches are read from the backend (`null`).
  const gateSetting = switchesReady ? gateActive(settings) : null;
  const guardSetting = switchesReady ? settings.commandGuardEnabled : null;
  const gate = useGate(gateSetting, guardSetting, (fresh) => {
    const current = noticesAt(settingsRef.current, new Date());
    if (!current.attentionEnabled) return;
    const watched = windowWatched();
    if (current.chime && fresh.some((d) => !(watched && shownRef.current.has(d.sessionId)))) {
      chimes.request("waiting", current.chimeVolume);
    }
    flasher.attentionChanged("attention", {
      enabled: current.taskbarFlash,
      windowFocused: document.hasFocus(),
    });
    const first = fresh[0];
    if (first === undefined || !current.notifyDesktop || document.hasFocus()) return;
    const name = (id: number) =>
      stateRef.current.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
    const text = decisionText(first, name, createTranslator(current.language));
    api.notifySession(first.sessionId, text.title, text.detail).catch(() => undefined);
  });
  /** A remembered answer that replied for a session: a short note on its pane. */
  const ruleNotes = useRuleNotes(gate.log);
  /** Pending decisions not answered here yet, oldest first. */
  const pendingGate = useMemo(
    () => gate.pending.filter((decision) => !answeredGate.has(decision.id)),
    [gate.pending, answeredGate],
  );
  /**
   * Sessions with a pending gate decision or a dialog their agent's hooks reported (ADR 0016), for
   * the attention listener (a queued message waits) and the link deliveries (no Enter goes in).
   */
  const gateWaitingRef = useRef<ReadonlySet<number>>(new Set());
  const permissionWaits = usePermissionWaits();
  const permissionWaitingRef = useRef<ReadonlySet<number>>(new Set());
  useEffect(() => {
    gateWaitingRef.current = waitingSessions({ enabled: gate.enabled, pending: pendingGate });
  }, [gate.enabled, pendingGate]);
  useEffect(() => {
    permissionWaitingRef.current = permissionWaits;
  }, [permissionWaits]);
  // A session waiting for a decision looks like any waiting session, with the reason "gate"; one
  // whose agent shows a dialog, with the reason "permission".
  const { attention, reasons } = useMemo(
    () =>
      withPermission(
        withGate(
          { attention: trackedAttention, reasons: trackedReasons },
          waitingSessions({ enabled: gate.enabled, pending: pendingGate }),
        ),
        permissionWaits,
      ),
    [trackedAttention, trackedReasons, gate.enabled, pendingGate, permissionWaits],
  );
  /** Overlaps per session, grouped once per snapshot. */
  const overlapsById = useMemo(() => overlapsBySession(claims), [claims]);
  // Shared projects (team service step F part 2b): the teammates' sessions that collide with
  // ours join the same marker, Changes block and one-time notice as the user's own sessions.
  const teamStore = useTeam();
  /** This device's sessions for the Files tab: names, programs and repositories. */
  const localSessions = useMemo(
    () => localSessionsOf(state.sessions, profiles),
    [state.sessions, profiles],
  );
  // The Files tab's activity map hears every claims and team update here, in the main window.
  useActivityRecorder(claims, teamStore.activities, localSessions);
  const teammatesById = useMemo(
    () => teammateOverlapsBySession(teamStore.activities),
    [teamStore.activities],
  );
  /** Sidebar marker text per session that collides with another one (or a teammate) on a file. */
  const overlapMarkers = useMemo(() => {
    const name = (id: number) => state.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;
    const markers = new Map<number, string>();
    for (const session of state.sessions) {
      const text =
        markerText(overlapsById.get(session.id) ?? NO_OVERLAPS, name, t) ??
        teammateMarkerText(teammatesById.get(session.id) ?? NO_TEAMMATES, t);
      if (text !== null) markers.set(session.id, text);
    }
    return markers;
  }, [overlapsById, teammatesById, state.sessions, t]);
  /** Teammate collisions already announced in this app run. */
  const seenTeammates = useRef(new Set<string>());
  /** When the last desktop notification about a teammate went out. */
  const teammateNotifiedAt = useRef<number | null>(null);
  useEffect(() => {
    const fresh = freshTeammateOverlaps(seenTeammates.current, teamStore.activities);
    rememberSeen(seenTeammates.current, fresh.map(teammateOverlapKey));
    const first = fresh[0];
    if (first === undefined) return;
    const now = Date.now();
    setTeamNotice({ overlap: first, key: now });
    const current = noticesAt(settingsRef.current, new Date());
    if (!current.notifyDesktop || !current.attentionEnabled || document.hasFocus()) return;
    // Teammates' devices decide how many collisions arrive: the desktop hears of a few at most.
    if (!teammateNoticeDue(teammateNotifiedAt.current, now)) return;
    teammateNotifiedAt.current = now;
    const session =
      stateRef.current.sessions.find((s) => s.id === first.sessionId)?.title ??
      `#${String(first.sessionId)}`;
    const text = teammateNotice(first, session, createTranslator(current.language));
    api.notifySession(first.sessionId, text.title, text.body).catch(() => undefined);
  }, [teamStore.activities]);
  // Reservations: each notice once in the window, and a release request for a task this device
  // holds or a warning as a desktop notification while Hangar is in the background, at most once
  // per TEAMMATE_NOTICE_GAP_MS like the teammates' overlaps.
  const seenTaskNotices = useRef(new Set<string>());
  const seenRequests = useRef(new Set<string>());
  const taskNotifiedAt = useRef<number | null>(null);
  useEffect(() => {
    const translate = createTranslator(settingsRef.current.language);
    const notices = freshTaskNotices(seenTaskNotices.current, teamStore.activities);
    rememberSeen(
      seenTaskNotices.current,
      notices.map((entry) => entry.key),
    );
    const requests = releaseRequests(teamStore.activities).filter(
      (entry) => !seenRequests.current.has(releaseRequestKey(entry.projectId, entry.task)),
    );
    rememberSeen(
      seenRequests.current,
      requests.map((entry) => releaseRequestKey(entry.projectId, entry.task)),
    );
    const notice = notices[notices.length - 1]?.notice;
    const now = Date.now();
    if (notice !== undefined) {
      setTaskNotice({ text: taskNoticeText(notice, translate, clockTime), key: now });
    }
    const request = requests[0]?.task;
    let desktop: { title: string; body: string; session: number | null } | null = null;
    if (request?.request) {
      desktop = {
        title: translate("team.request.title"),
        body: translate("team.request.text", {
          person: personTag(request.request, translate),
          title: taskTitle(request.title, translate),
        }),
        session: null,
      };
    } else if (notice !== undefined) {
      desktop = {
        title: translate("team.tasks.title"),
        body: taskNoticeText(notice, translate, clockTime),
        session: notice.sessionId,
      };
    }
    const current = noticesAt(settingsRef.current, new Date());
    if (desktop === null || !current.notifyDesktop || !current.attentionEnabled) return;
    if (document.hasFocus() || !teammateNoticeDue(taskNotifiedAt.current, now)) return;
    taskNotifiedAt.current = now;
    const session = desktop.session ?? stateRef.current.activeId ?? 0;
    api.notifySession(session, desktop.title, desktop.body).catch(() => undefined);
  }, [teamStore.activities]);
  // What each session does goes into the team's presence (only for sessions in repositories
  // linked to a shared project; the backend picks them). At most once per second, always the
  // newest states: a steady stream of changes must not hold them back.
  const sessionIds = state.sessions.map((session) => session.id).join(",");
  const statesTimer = useRef<number | null>(null);
  const sendStates = useRef<() => void>(() => undefined);
  useEffect(() => {
    const ids = sessionIds === "" ? [] : sessionIds.split(",").map(Number);
    sendStates.current = () => {
      statesTimer.current = null;
      api.teamSessionStates(sessionStates(ids, attention)).catch(() => undefined);
    };
    statesTimer.current ??= window.setTimeout(() => {
      sendStates.current();
    }, TEAM_STATES_DELAY_MS);
  }, [sessionIds, attention]);
  useEffect(
    () => () => {
      if (statesTimer.current !== null) window.clearTimeout(statesTimer.current);
    },
    [],
  );
  /** Where the next session starts: the active project, a chosen folder, the last one, home. */
  const target = resolveStartTarget({
    chosen: chosenCwd,
    active: activeSession,
    lastUsed: folders.recent[0] ?? null,
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
  // Detached: the buttons and menu bring the panel window to the front, they hide nothing.
  const sideShown = layout.sideOpen && !panelWindow.detached;
  /** The panel slides in and out (sidePanelMotion.ts); detaching takes it away at once. */
  const sideMotion = useSidePanelMotion(sideShown, !panelWindow.detached);
  const notesShown = sideShown && layout.sideTab === "notes";
  const changesShown = sideShown && layout.sideTab === "changes";
  const filesShown = sideShown && layout.sideTab === "files";
  useEffect(() => {
    changesShownRef.current = changesShown;
  });

  if (search !== null && search.sessionId !== state.activeId) setSearch(null);

  // The marker dialog or session menu of a session that is gone (closed, or replaced by a restart
  // with a new ID) closes, or the modal flag would keep blocking the shortcuts.
  if (markerId !== null && !state.sessions.some((s) => s.id === markerId)) setMarkerId(null);
  if (queueEditor !== null && !state.sessions.some((s) => s.id === queueEditor.id)) {
    setQueueEditor(null);
  }
  if (sessionMenu !== null && !state.sessions.some((s) => s.id === sessionMenu.id)) {
    setSessionMenu(null);
  }
  // The same for the commands submenu and its question, also once the session's process ended
  // or it may show a question (agentCommands.ts `awaitsAnswer`).
  const commandsGone = (id: number) =>
    !state.sessions.some((s) => s.id === id && s.status === "running") ||
    awaitsAnswer(reasons.get(id) ?? null);
  if (commandMenu !== null && commandsGone(commandMenu.id)) setCommandMenu(null);
  if (commandAsk !== null && commandsGone(commandAsk.id)) setCommandAsk(null);
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

  const closeTeam = useCallback(() => {
    setTeamOpen(false);
    focusActive();
  }, [focusActive]);

  /** Settings, on `section` or the section chosen last; the Team window makes way. */
  const openSettings = useCallback((section: SettingsSection | null = null) => {
    setTeamOpen(false);
    setSettingsSection(section);
    setDialog("settings");
  }, []);

  // Dictation: hold the dictation key in a session, speak, let go; the text goes into the session
  // that was active on key down, through the same paste path as dropped files (no Enter).
  const dictationStatus = useDictationStatus(settings.dictation.model);
  const [dictationProblem, setDictationProblem] = useState<{
    session: number;
    problem: DictationProblem;
  } | null>(null);
  useEffect(() => {
    setDictationKey(settings.dictationEnabled ? settings.dictation.key : null);
  }, [settings.dictationEnabled, settings.dictation.key]);
  useEffect(() => {
    if (dictationProblem === null) return;
    const timer = window.setTimeout(() => {
      setDictationProblem(null);
    }, 4000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [dictationProblem]);
  useDictationKey({
    enabled: settings.dictationEnabled,
    prefs: settings.dictation,
    status: dictationStatus,
    target: () => activeIdRef.current,
    blocked: () => modalRef.current,
    onSetup: () => {
      openSettings("dictation");
    },
    onText: ({ session, text }) => {
      terminals.get(session)?.pasteText(pasteForm(text));
    },
    onProblem: (problem, session) => {
      setDictationProblem({ problem, session });
    },
  });
  const dictationSession = busySession(dictationStatus.state);
  const dictationMode = (id: number): IndicatorMode | null => {
    if (dictationSession === id) {
      return dictationStatus.state.kind === "recording" ? "recording" : "transcribing";
    }
    return dictationProblem?.session === id ? dictationProblem.problem : null;
  };

  /** After a mouse click on a button: focus back to the terminal, even if the button has it. */
  const focusTerminal = useCallback(() => {
    requestAnimationFrame(() => {
      const id = activeIdRef.current;
      if (id !== null) terminals.get(id)?.focus();
    });
  }, []);

  /** The home view and back: the sessions step aside and keep running (sidebar's home button). */
  const toggleHome = () => {
    if (state.sessions.length === 0) return;
    setPutAside((current) => !current);
    focusTerminal();
  };

  /**
   * The project overview of the home view, from the menu or quick access; sessions keep running.
   * Its heading takes the focus once it shows, after the menu or quick access gave the focus back.
   */
  const showProjects = () => {
    if (state.sessions.length > 0) setPutAside(true);
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(".projects h2")?.focus();
    });
  };

  /** A project from quick access: the home view shows and its card scrolls into view. */
  const showProject = (key: string) => {
    if (state.sessions.length > 0) setPutAside(true);
    setProjectFocus((current) => ({ key, token: (current?.token ?? 0) + 1 }));
  };

  /** "One line" (oneLineRows.ts): every row on one close line, and back to the layout before. */
  const toggleRowsOneLine = () => {
    setSettings((current) => ({ ...current, ...toggleOneLine(current) }));
  };

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
    setPutAside(false);
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
    ratios: layout.paneRatios,
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

  // Sub-agents: one watch, for the open view's session or else the active session while the
  // sidebar shows its tree; a restart or resume fallback with a new conversation id starts a new
  // watch. Texts only flow while the view is open; the tree needs none. The log follows the
  // sub-agent the view shows (focus), without restarting the watch.
  const subAgentSession =
    subAgentsOf === null ? undefined : state.sessions.find((s) => s.id === subAgentsOf);
  const subAgentsAvailable =
    subAgentSession?.subAgents === true && !isPlaceholder(subAgentsOf ?? 0);
  // The tree hangs below the active row; a folded group hides that row, and the tree too.
  const subAgentTreeShown =
    !layout.collapsed &&
    activeSession !== undefined &&
    !layout.closedAgentGroups.includes(sessionGroupKey(activeSession, grouping));
  // The crew column beside the active terminal needs the same watch, also without the tree.
  const crewWatching = crewWatchWanted(
    activeSession?.subAgents === true && !isPlaceholder(activeSession.id),
    settings.crewColumn,
  );
  const watchedId = watchedSession(subAgentsOf, state.activeId, subAgentTreeShown || crewWatching);
  const watched = watchedId === null ? undefined : state.sessions.find((s) => s.id === watchedId);
  const subAgentKey = useSubAgentWatch(
    watched !== undefined && watched.subAgents === true && !isPlaceholder(watched.id)
      ? {
          sessionId: watched.id,
          cliSessionId: watched.cliSessionId ?? null,
          excerpts: settings.subAgentExcerpts && (subAgentsOf !== null || crewWatching),
          focus: subAgentsOf === null ? null : subAgentPick,
        }
      : null,
  );
  const subAgentHidden = dismissedIn(dismissedSubAgents, subAgentKey);
  // Only counts and states: a snapshot that changes times, tools or excerpts renders the tree and
  // the pane (they read the store themselves), never the app.
  const subAgentWatch = useSubAgentSummary(subAgentKey, subAgentHidden);
  if (subAgentsOf === null && subAgentPick !== null) setSubAgentPick(null);
  const subAgentNotice =
    subAgentWatch.error !== null
      ? t("subAgents.error", { error: subAgentWatch.error })
      : !subAgentsAvailable
        ? t("subAgents.unavailable")
        : subAgentWatch.located === null
          ? t("subAgents.loading")
          : !subAgentWatch.located
            ? t("subAgents.notLocated")
            : t("subAgents.none");
  const subAgentShowsNotice =
    subAgentWatch.error !== null || !subAgentsAvailable || subAgentWatch.located !== true;
  /**
   * Layout of the open view, `null` while closed: always the session's terminal with one pane
   * beside it, so the main agent stays in sight (and can be answered) while a sub-agent's
   * activity shows, and switching sub-agents never resizes the terminal.
   */
  const subAgentPlan =
    subAgentSession === undefined
      ? null
      : { layout: SUB_AGENT_LAYOUT, ratios: subAgentRatios(layout.subAgentRatios) };
  /** The splitter beside the sub-agent view; its position is remembered apart from the split view. */
  const setSubAgentSplit = (next: Ratios) => {
    setLayout((current) =>
      sameRatios(subAgentRatios(current.subAgentRatios), next)
        ? current
        : { ...current, subAgentRatios: next },
    );
  };
  const resetSubAgentSplit = () => {
    setLayout((current) =>
      current.subAgentRatios === null ? current : { ...current, subAgentRatios: null },
    );
  };
  // Sessions shown in a pane right now; the chime stays silent for them while the window has focus.
  const shown = new Set<number>(state.activeId === null ? [] : [state.activeId]);
  if (subAgentPlan === null) {
    for (const id of splitView.split.slots) if (id !== null) shown.add(id);
  }
  const shownKey = [...shown].sort((a, b) => a - b).join(",");
  /** Where each visible session sits in a split layout, for the miniature on its sidebar row. */
  const placements = new Map<number, PanePlacement>();
  if (subAgentPlan === null && splitView.layout !== "single") {
    const { layout: splitLayout, ratios } = splitView;
    splitView.split.slots.forEach((id, zone) => {
      if (id !== null) placements.set(id, { layout: splitLayout, zone, ratios });
    });
  }
  useEffect(() => {
    shownRef.current = new Set(
      shownKey === "" ? [] : shownKey.split(",").map((part) => Number(part)),
    );
  }, [shownKey]);
  // The wait hint hears what the rows show, so waiting at the gate counts as well; a session on
  // screen never waits unseen (its gate keeps the "decision" chip). The monitor keeps the start of
  // a session that goes on waiting and ignores calls that change nothing.
  useEffect(() => {
    const ids = state.sessions.map((session) => session.id).filter((id) => !isPlaceholder(id));
    const onScreen = new Set(shownKey === "" ? [] : shownKey.split(",").map(Number));
    const waiting = waitingIds(ids, attention, reasons, onScreen);
    for (const id of ids) waitMonitor.changed(id, waiting.has(id));
  }, [waitMonitor, state.sessions, attention, reasons, shownKey]);

  /** The one decision on a card; the others are counted on it. */
  const shownDecision = decisionToShow(pendingGate, state.activeId, shown);

  const answerDecision = (decision: PendingDecision, answer: GateAnswer) => {
    setGateError(null);
    setAnsweredGate((current) => new Set(current).add(decision.id));
    focusTerminal();
    // `false` means the decision no longer waited: the next snapshot drops it anyway.
    api.answerGate(decision.id, answer).catch((e: unknown) => {
      setAnsweredGate((current) => {
        const next = new Set(current);
        next.delete(decision.id);
        return next;
      });
      setGateError({ id: decision.id, text: t("gate.failed", { error: errorText(e) }) });
    });
  };

  const clearGateFocus = useCallback(() => {
    setGateFocusToken(0);
  }, []);

  const gateCardFor = (decision: PendingDecision, floating: boolean) => (
    <GateCard
      key={decision.id}
      focusToken={gateFocusToken}
      onFocused={clearGateFocus}
      decision={decision}
      floating={floating}
      more={pendingGate.length - 1}
      error={gateError?.id === decision.id ? gateError.text : null}
      nameOf={sessionName}
      onAnswer={(answer) => {
        answerDecision(decision, answer);
      }}
      onShowSession={showSession}
      onEscape={focusTerminal}
    />
  );

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
  /**
   * A row of the sidebar's tree: shows this sub-agent's activity beside the terminal (the row
   * that is already shown closes it again); `null` ("+N more") shows the list of all of them.
   * A mouse click returns to the terminal, the keyboard stays in the tree.
   */
  const showSubAgent = (id: number, agentId: string | null, byKeyboard: boolean) => {
    if (subAgentsOf === id && agentId !== null && subAgentPick === agentId) {
      setSubAgentsOf(null);
    } else {
      if (subAgentsOf !== id) {
        dispatch({ type: "activated", id });
        setSubAgentsOf(id);
      }
      setSubAgentPick(agentId);
    }
    if (!byKeyboard) focusTerminal();
  };
  /**
   * Removes finished sub-agents from the tree and the list; a shown one closes the view. A mouse
   * click returns to the terminal (the clicked button is gone), the keyboard stays in the tree.
   */
  const dismissSubAgents = (ids: readonly string[], byKeyboard: boolean) => {
    if (subAgentKey === null) return;
    const agents = subAgentsNow(subAgentKey);
    const next = dismissFinished(dismissedSubAgents, subAgentKey, agents, ids);
    if (next === dismissedSubAgents) return;
    setDismissedSubAgents(next);
    const hidden = dismissedIn(next, subAgentKey);
    if (subAgentPick !== null && hidden.has(subAgentPick)) setSubAgentsOf(null);
    // With the last row gone the tree disappears; the keyboard goes back to the terminal.
    if (!byKeyboard || visibleAgents(agents, hidden).length === 0) focusTerminal();
  };

  /** New place of a session in the sidebar (drag or Alt+arrow); the workspace saves the order. */
  const moveSession = (id: number, index: number) => {
    const session = state.sessions.find((s) => s.id === id);
    dispatch({ type: "moved", id, index });
    // The place within the session's sidebar group, as the list shows it.
    const place = groupPosition(
      groupedOrder(moveItem(state.sessions, id, index) ?? [], grouping),
      id,
      grouping,
    );
    if (session && place) {
      setMoveAnnouncement(t("session.moved", { title: session.title, ...place }));
    }
  };

  const {
    drag: sessionDragView,
    begin: beginSessionDrag,
    attachGhost: attachDragGhost,
  } = useSessionDrag({
    layout: layout.snapLayout,
    ratios: layout.paneRatios,
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

  /**
   * An agent session stopped working: counts the changed files in its folder and offers them,
   * unless they are on screen already or the session works again by the time git answers.
   */
  const offerDone = useCallback((id: number) => {
    setFinishedRuns((current) => new Map(current).set(id, (current.get(id) ?? 0) + 1));
    const session = stateRef.current.sessions.find((s) => s.id === id);
    if (session === undefined || isPlaceholder(id) || session.kind === "update") return;
    const profile = profilesRef.current.find((p) => p.id === session.profileId);
    if (profile === undefined || shellKind(profile.command) !== "agent") return;
    doneFileCount(id, api.gitChangedCount)
      .then((count) => {
        const terminal = terminals.get(id);
        const current = stateRef.current.sessions.find((s) => s.id === id);
        if (terminal === undefined || current === undefined) return;
        const offer = offersDoneNotice({
          count,
          sessionId: id,
          state: terminal.attention.state,
          activeId: activeIdRef.current,
          changesShown: changesShownRef.current,
        });
        if (!offer) return;
        // The name the sidebar shows: the folder for a session still named after its program.
        const label = sessionLabels(
          [current],
          (profileId) => profilesRef.current.find((p) => p.id === profileId)?.name,
        ).get(id);
        setDoneNotice({ sessionId: id, title: label?.primary ?? current.title, count });
      })
      .catch(() => undefined);
  }, []);

  /**
   * Saves the work tree of an agent session that just finished a step, silently, for its
   * timeline (timeline.ts). The backend skips it when nothing changed or outside a repository.
   */
  const takeSnapshot = useCallback((id: number) => {
    const session = stateRef.current.sessions.find((s) => s.id === id);
    const terminal = terminals.get(id);
    if (session === undefined || terminal === undefined) return;
    const profile = profilesRef.current.find((p) => p.id === session.profileId);
    if (!takesSnapshots(id, runsAgent(profile?.command, session.kind))) return;
    // The backend remembers a skipped or failed step and shows it in the timeline.
    api.snapshotTake(id, terminal.takeStepPrompt()).catch(() => undefined);
  }, []);

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
      // An OSC 8 link opens like a printed address; the notice shows it to copy if that fails.
      // When its visible text claims another host or a path, the notice shows the real address
      // and opens it only on request. Its own text and the word around it are judged both
      // (termSafety.ts `osc8DecisionOf`).
      terminal.onLink = (url, shown) => {
        if (osc8DecisionOf([shown.own, shown.word], url) === "confirm") {
          setLink({ url, copied: false, ask: true });
          return;
        }
        api.openWebUrl(url).catch(() => {
          setLink({ url, copied: false });
        });
      };
      // Addresses and existing paths in the output (terminalLinkOpen.ts decides what opens).
      terminal.cwd = () => stateRef.current.sessions.find((s) => s.id === id)?.cwd ?? null;
      terminal.onOpenLink = (request) => {
        void openLink(request, {
          openUrl: (url) => api.openWebUrl(url),
          openPath: (path) => api.openPath(path),
          openViewer: (relative) =>
            api.openFileViewer(id, relative).then(() => {
              rememberFile(id, relative);
            }),
        }).then((outcome) => {
          if (outcome.kind === "showUrl") setLink({ url: outcome.url, copied: false });
          if (outcome.kind === "failed") {
            const translate = createTranslator(settingsRef.current.language);
            setError(translate("link.openFailed", { error: errorText(outcome.error) }));
          }
        });
      };
      // A pasted screenshot's path is quoted like a dropped file, for the session's program.
      terminal.shellKind = () => {
        const session = stateRef.current.sessions.find((s) => s.id === id);
        const profile = profilesRef.current.find((p) => p.id === session?.profileId);
        return shellKind(profile?.command ?? "");
      };
      // Ctrl+V and Shift+Insert ask like "Send to" when the text would run lines or hides some.
      terminal.warnsBeforePaste = () => settingsRef.current.warnMultilinePaste;
      terminal.confirmPaste = (warning) =>
        confirmPaste(stateRef.current.sessions.find((s) => s.id === id)?.title ?? "")(warning);
      terminal.onImagePasteFailed = () => {
        setStartHint(createTranslator(settingsRef.current.language)("paste.imageFailed"));
      };
      // The person answered the dialog the agent's hooks reported (ADR 0016): Claude Code sends no
      // hook after a "No", so the signal goes with the person's key.
      terminal.onPersonAnswerKey = () => {
        if (permissionWaitingRef.current.has(id)) {
          api.permissionAnswered(id).catch(() => undefined);
        }
      };
      // A line the person typed and sent in a link group member: the group's budget starts again.
      terminal.onPersonSubmit = () => {
        if (groupOf(linkStateRef.current, id) !== undefined) {
          api.linkPersonTyped(id).catch(() => undefined);
        }
      };
      // An agent session is named after its first task, unless the user named it (promptTitle.ts).
      terminal.onPromptLine = (line) => {
        const session = stateRef.current.sessions.find((s) => s.id === id);
        const profile = profilesRef.current.find((p) => p.id === session?.profileId);
        // Placeholders and unknown programs wait: the shell kind decides, and it needs the profile.
        if (session === undefined || profile === undefined || isPlaceholder(id)) return;
        const verdict = promptVerdict(
          {
            title: session.title,
            cwd: session.cwd,
            kind: session.kind,
            programNames: [profile.name, profile.id],
            shell: shellKind(profile.command),
          },
          line,
        );
        if (verdict.kind === "wait") return;
        terminal.onPromptLine = null;
        if (verdict.kind === "stop") return;
        api
          .rename(id, verdict.title)
          .then((info) => {
            dispatch({ type: "updated", session: info });
          })
          // Not worth an error: the session keeps its name, the user can still rename it.
          .catch(() => undefined);
      };
      terminal.onDevServer = (url) => {
        setDevServers((current) => {
          if ((current.get(id) ?? null) === url) return current;
          const map = new Map(current);
          if (url === null) map.delete(id);
          else map.set(id, url);
          return map;
        });
      };
      // Only state changes arrive here, never individual output chunks. A change of the reason
      // alone (a bell or an exit while already waiting) does not flash or notify again.
      let previous: AttentionState = "idle";
      let previousReason: AttentionReason | null = null;
      terminal.setAttentionListener((next, reason) => {
        const entered = previous !== "attention" || next !== "attention";
        const notices = noticesAt(settingsRef.current, new Date());
        const session = stateRef.current.sessions.find((s) => s.id === id);
        // A queued message (messageQueue.ts): armed by work that follows the user's input, sent
        // when the work is done, kept when the program asks.
        if (next === "working" && previous !== "working") {
          changeQueue(workStarted(queueRef.current, terminal.key, terminal.hasTyped()));
        }
        const ended = phaseEnd(
          previous,
          next,
          reason,
          terminal.attention.endedBy,
          gateWaitingRef.current.has(id) || permissionWaitingRef.current.has(id),
        );
        // A permission dialog rings no bell: a question on screen keeps the message, because
        // Enter after it would confirm the preselected answer (promptGuard.ts).
        const end =
          ended === "done" && queueRef.current.has(terminal.key)
            ? guardedEnd(ended, looksLikeQuestion(terminal.screenLines()))
            : ended;
        const queued = end === null ? null : phaseEnded(queueRef.current, terminal.key, end);
        if (queued !== null) changeQueue(queued.queue);
        // The session goes on with the queued message right away: no notice that it waits.
        const send = queued?.send ?? null;
        const continues = send !== null;
        if (send !== null) sendQueuedRef.current(id, send);
        // The stall hint belongs to one working phase.
        if (next !== "working") stallMonitor.progressed(id);
        const chime =
          continues || session === undefined || isPlaceholder(id)
            ? null
            : chimeFor({ previous, previousReason, next, reason }, sessionExit(session, terminal), {
                enabled: notices.chime && notices.attentionEnabled,
                windowFocused: windowWatched(),
                shown: shownRef.current.has(id),
              });
        // Only work the user asked for: a program's start-up output (also on restore) is no task.
        // A step that goes on with a queued message is still a step worth a snapshot.
        if (finishedWork(previous, next, reason) && terminal.takeTyped()) {
          takeSnapshot(id);
          if (!continues) offerDone(id);
        }
        // New work makes the session's done notice stale.
        if (next === "working") {
          setDoneNotice((current) => (current?.sessionId === id ? null : current));
        }
        arenaAttention.current(id, next);
        if (!isPlaceholder(id) && session?.kind !== "update") {
          void reportPhase(
            phases.change(id, next === "working", Date.now(), terminal.activity().lastOutput),
          );
        }
        previous = next;
        previousReason = reason;
        if (chime !== null) chimes.request(chime, settingsRef.current.chimeVolume);
        // Every call, not only on entering: waiting turns into done or failed when the program
        // exits while it waits. Update sessions are no agent's work.
        pushes.change(
          id,
          continues || session === undefined || isPlaceholder(id) || session.kind === "update"
            ? null
            : pushEventFor(next, terminal.exitCode),
        );
        setReasons((current) => {
          if ((current.get(id) ?? null) === reason) return current;
          const map = new Map(current);
          if (reason === null) map.delete(id);
          else map.set(id, reason);
          return map;
        });
        if (!entered) return;
        flasher.attentionChanged(next, {
          enabled: notices.taskbarFlash && !continues,
          windowFocused: document.hasFocus(),
        });
        const notify = shouldNotify(next, {
          enabled: notices.notifyDesktop && !continues,
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
      // The backend names the session's snapshots by this key from now on; windows name the
      // session by ID only.
      if (!isPlaceholder(id)) api.snapshotBind(id, terminal.key).catch(() => undefined);
    },
    [flasher, chimes, pushes, offerDone, takeSnapshot, changeQueue, stallMonitor, confirmPaste],
  );

  /**
   * Events that arrived before `create_session` answered and the session existed: a very short
   * process has already exited, or resuming has already fallen back to a normal start.
   */
  const catchUp = useCallback((terminal: SessionTerminal, id: number) => {
    if (terminal.fallback !== undefined) dispatch({ type: "fellBack", id, ...terminal.fallback });
    if (terminal.exitCode !== undefined) dispatch({ type: "exited", id, code: terminal.exitCode });
  }, []);

  const forgetAttention = useCallback(
    (id: number) => {
      pushes.forget(id);
      waitMonitor.remove(id);
      void reportPhase(
        phases.end(id, Date.now(), terminals.get(id)?.activity().lastOutput ?? null),
      );
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
    },
    [pushes, waitMonitor],
  );

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
   * `folder`: start there instead of the start target (quick access). `mode` "team": first a git
   * worktree with a branch of its own is created from that folder, and the session runs in it with
   * the agent gate (ADR 0008). A start in the target uses up a chosen folder: the next one follows
   * the active session again. `title`: a name chosen beforehand (workspace templates). `onError`
   * takes a failure's text instead of the error notice (a template reports once at the end).
   * `prepared`: a worktree made before the start; the session runs there privately unless `mode`
   * is "team" and gets `onStarted` with its id. A `fresh` one (the arena's) is taken away again
   * when the session does not run, a reused one (a handover) never. `subAgent`: Claude Code
   * starts as that sub-agent definition (subAgentLibrary.ts) and the session is named after it.
   * `resume`: start the program resumed, as a restore does (a recently closed session): the
   * conversation with `cliSessionId`, or without one the program's last conversation in the
   * folder (`resumeArgs`); a program without either starts normally, and if resuming fails the
   * backend starts it fresh (`resumeFallback`).
   * Resolves to the new session's id, `null` when it does not run.
   */
  const start = useCallback(
    async (
      profile: Profile,
      folder?: string,
      mode: StartMode = "private",
      title?: string,
      onError?: (text: string) => void,
      prepared?: PreparedWorktree,
      subAgent: string | null = null,
      resume: { cliSessionId: string | null } | null = null,
    ): Promise<number | null> => {
      const fail = onError ?? setError;
      const base = folder ?? targetRef.current;
      let worktree: Worktree | undefined = prepared?.worktree;
      // A team session needs its worktree; without a folder there is none to make.
      if (mode === "team" && base === null) return null;
      if (mode === "team" && base !== null && worktree === undefined) {
        setCreatingWorktree(true);
        try {
          const made = parseWorktree(await api.createWorktree(base, worktreeLabel(profile.name)));
          if (made === null) throw new Error(t("error.unknown"));
          worktree = made;
        } catch (e) {
          fail(createErrorText(e, t));
          return null;
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
          terminal.startSize(INITIAL_SIZE),
          terminal.createChannel(),
          resume !== null,
          resume?.cliSessionId ?? null,
          mode === "team" && worktree !== undefined ? "team" : "private",
          // A prepared worktree (a handover, the arena) is not fresh from the project: no
          // upstream check as the first prompt.
          startOptions(
            settingsRef.current,
            worktree !== undefined && prepared === undefined,
            subAgent,
          ),
        );
        terminal.setId(info.id);
        wire(terminal, info.id);
        prepared?.onStarted?.(info.id);
        // The same program twice: "PowerShell 7 (2)", so the sidebar can tell them apart.
        // A real name, not a display trick: rename and restore keep it.
        const others = [
          ...stateRef.current.sessions.map((session) => session.title),
          ...claimedTitles.current.values(),
        ];
        // In an own worktree the branch tells sessions apart: "Claude Code · 1a2b3c".
        // A sub-agent session is named after its agent: "backend".
        const programTitle = subAgent ?? info.title;
        const named = numberedTitle(
          title ?? (worktree ? worktreeTitle(programTitle, worktree.branch) : programTitle),
          others,
        );
        claimedTitles.current.set(info.id, named);
        if (folder === undefined) setChosenCwd(null);
        dispatch({
          type: "added",
          session: await withTitle(info, named),
          ...(worktree ? { worktree } : {}),
        });
        setLastProgramId(profile.id);
        // A start the list keeps (not in an own worktree, not as a sub-agent definition, not a
        // team session): it is offered again once it is no longer open.
        const opened =
          worktree === undefined && subAgent === null && mode !== "team"
            ? recentEntry(
                {
                  profileId: profile.id,
                  cwd: info.cwd,
                  title: named,
                  // The id Hangar gave the agent CLI: a list kept from a crash still resumes it.
                  cliSessionId: info.cliSessionId ?? resume?.cliSessionId,
                },
                Date.now(),
              )
            : null;
        if (opened !== null) setRecentSessions((current) => pushRecent(current, opened));
        // The folder the session really runs in (the backend may fall back to the home folder);
        // for a worktree the repository's folder, so recent folders do not fill with worktrees.
        const used = worktree === undefined ? info.cwd : (prepared?.worktree.repoRoot ?? base);
        if (used !== null) setFolders((current) => recordRecent(current, used));
        // Very short processes may already have exited before the response arrives.
        if (terminal.exitCode !== undefined) {
          dispatch({ type: "exited", id: info.id, code: terminal.exitCode });
        }
        return info.id;
      } catch (e) {
        terminal.dispose();
        fail(errorText(e));
        // Nothing runs in the new worktree yet: take it away again (never forced).
        if (worktree && (prepared === undefined || prepared.fresh))
          api.removeWorktree(worktree).catch(() => undefined);
        return null;
      }
    },
    [
      layout.fontSize,
      attentionConfig,
      theme,
      terminalLook,
      settings.scrollbackLines,
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
          terminal.startSize(INITIAL_SIZE),
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
          subAgent: saved.subAgent,
          mode: sessionMode(
            saved,
            profilesRef.current.find((p) => p.id === saved.profileId),
          ),
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
    async (saved: SavedSession, banner: string): Promise<RestoredSession> => {
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
        return { id: addPlaceholder(terminal, saved), start: "placeholder" };
      }
      try {
        const info = await api.createSession(
          saved.profileId,
          saved.cwd,
          terminal.startSize(INITIAL_SIZE),
          terminal.createChannel(),
          true,
          saved.cliSessionId,
          // Worktree sessions of older workspaces come back as team sessions where the program
          // can be one.
          sessionMode(saved, profile),
          startOptions(settingsRef.current, false, saved.subAgent),
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
        return { id: info.id, start: info.resumed === true ? "resumed" : "started" };
      } catch (e) {
        terminal.writeNotice(tr("terminal.startFailed", { error: errorText(e) }));
        return { id: addPlaceholder(terminal, saved), start: "placeholder" };
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
            terminal.startSize(),
            terminal.createChannel(),
            true,
            session.cliSessionId ?? null,
            sessionMode(
              session,
              profilesRef.current.find((p) => p.id === session.profileId),
            ),
            startOptions(settingsRef.current, false, session.subAgent ?? null),
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
        const info = await api.restart(id, terminal.startSize(), terminal.createChannel());
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

  /**
   * `discard`: the user confirmed in the arena that the worktree goes with all its changes
   * (arena_discard); otherwise it is removed only when clean.
   */
  const close = useCallback(
    async (id: number, discard = false) => {
      const session = stateRef.current.sessions.find((s) => s.id === id);
      const worktree = session?.worktree;
      let closed = true;
      if (!isPlaceholder(id)) {
        try {
          await api.close(id);
        } catch (e) {
          closed = false;
          showError(e);
        }
        // Closed means: its timeline goes too. Only then: a session that still runs keeps it.
        // Before the worktree goes, while its repository is still there.
        if (closed) await api.snapshotForget(id).catch(() => undefined);
      }
      // Remove locally even on error: the backend no longer knows the session anyway.
      const terminal = terminals.get(id);
      if (terminal) {
        // Closed means: it does not come back after a restart either.
        api.deleteScrollback(terminal.key).catch(() => undefined);
        // Its queued message goes with it.
        changeQueue(discardMessage(queueRef.current, terminal.key));
        terminal.dispose();
      }
      terminals.delete(id);
      forgetFile(id);
      // Only once the process is gone: a still running one holds the conversation. Replaces the
      // entry of its start with the final title, marker and conversation.
      const entry = closed && session !== undefined ? recentEntry(session, Date.now()) : null;
      if (entry !== null) setRecentSessions((current) => pushRecent(current, entry));
      dispatch({ type: "removed", id });
      forgetAttention(id);
      setDevServers((current) => {
        if (!current.has(id)) return current;
        const map = new Map(current);
        map.delete(id);
        return map;
      });
      setDoneNotice((current) => (current?.sessionId === id ? null : current));
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
        } else if (discard) {
          api.arenaDiscard(worktree).catch((e: unknown) => {
            setWorktreeNotice({
              message: tr("arena.discardFailed", { error: arenaErrorText(e, tr) }),
              path: worktree.path,
              copied: false,
            });
          });
        } else {
          removeSessionWorktree(worktree);
        }
      }
    },
    [showError, forgetAttention, removeSessionWorktree, changeQueue],
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
              subAgent: session.subAgent ?? null,
              queuedMessage: queueRef.current.get(terminal.key)?.text ?? null,
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
    // The panel window saves its pending input too and says so (or the timeout ends the wait).
    // A window that is gone (crash) cannot answer, so the quit does not wait for it.
    const panelWindowSaved = loadWindowPrefs().detached
      ? Window.getByLabel(PANEL_WINDOW_LABEL)
          .then((found) =>
            found === null
              ? undefined
              : new Promise<void>((resolve) => {
                  notesSavedRef.current = resolve;
                  emitTo(PANEL_WINDOW_LABEL, PANEL_COMMAND, { kind: "flush" }).catch(() => {
                    resolve();
                  });
                }),
          )
          .catch(() => undefined)
      : Promise.resolve();
    const work = Promise.all([
      panelWindowSaved,
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
        const real = ids.filter((id) => !isPlaceholder(id));
        await Promise.all(real.map((id) => api.close(id)));
        // Nothing brings these sessions back, so neither does their timeline.
        await Promise.allSettled(real.map((id) => api.snapshotForget(id)));
        await api.removeWorktree(worktree);
      }),
    );
  }, []);

  const quit = useCallback(async () => {
    if (quitting.current) return;
    quitting.current = true;
    try {
      // Open working phases end with the app; bounded like the rest of the quit.
      await Promise.race([
        Promise.all(phases.flush(Date.now()).map(reportPhase)),
        delay(QUIT_SAVE_TIMEOUT_MS),
      ]);
      await persistAll();
      await Promise.race([removeOrphanedWorktrees(), delay(QUIT_SAVE_TIMEOUT_MS)]);
      if (!settingsRef.current.restoreSessions) {
        // No session comes back, so no timeline does; the next start removes what is left.
        const ids = stateRef.current.sessions.map((s) => s.id).filter((id) => !isPlaceholder(id));
        await Promise.race([
          Promise.allSettled(ids.map((id) => api.snapshotForget(id))),
          delay(QUIT_SAVE_TIMEOUT_MS),
        ]);
      }
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
    if (!profilesReady || !protectedReady || !prefsReady || restoreStarted) return;
    restoreStarted = true;
    /**
     * The saved workspace with the queued messages an older Hangar kept elsewhere moved in
     * (once); its messages go into the queue. Without "Remember sessions" nothing comes back,
     * so the old places are only emptied.
     */
    const loadSaved = async (): Promise<Workspace | null> => {
      if (!settingsRef.current.restoreSessions) {
        await migrateQueuedMessages(EMPTY_WORKSPACE, api.saveWorkspace);
        return null;
      }
      const loaded = parseWorkspace(await api.loadWorkspace());
      const workspace = await migrateQueuedMessages(loaded ?? EMPTY_WORKSPACE, api.saveWorkspace);
      restoreQueue(workspace);
      return loaded === null ? null : workspace;
    };
    /**
     * `false`: after a reload the saved state could not be read. Saving then stays off
     * (with a visible error message), because it would otherwise overwrite that state.
     */
    const run = async (): Promise<boolean> => {
      const running = await api.listSessions();
      if (running.length > 0) {
        setOrphans(backgroundSessions(running, new Set()));
        try {
          rememberedWorkspace.current = await loadSaved();
        } catch (e) {
          showError(e);
          return false;
        }
        return true;
      }
      if (!settingsRef.current.restoreSessions) {
        await loadSaved();
        return true;
      }
      // How the previous run ended, as the backend read it at this start; an error means no notice.
      const cleanBefore = await api
        .lastExitWasClean()
        .then((clean) => clean !== false)
        .catch(() => true);
      const workspace = await loadSaved();
      if (!workspace || workspace.sessions.length === 0) return true;
      // Claude Code sessions get the gate only if the backend knows the setting before they start.
      await gateReady;
      const language = settingsRef.current.language;
      const banner = createTranslator(language)("terminal.restored", {
        time: formatShortTime(new Date(), language),
      });
      // The active session first so it is usable right away; the others afterwards in parallel and
      // slightly staggered, instead of each waiting for the previous one to start.
      const active = workspace.sessions.find((saved) => saved.key === workspace.activeKey);
      const starts: RestoredSession[] = [];
      if (active) {
        const start = await restoreSession(active, banner);
        starts.push(start);
        dispatch({ type: "activated", id: start.id });
      }
      const rest = workspace.sessions.filter((saved) => saved !== active);
      await Promise.allSettled(
        rest.map(async (saved, index) => {
          await delay(index * RESTORE_STAGGER_MS);
          starts.push(await restoreSession(saved, banner));
        }),
      );
      if (!cleanBefore) setCrashRestore(starts);
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
  }, [profilesReady, protectedReady, prefsReady, restoreSession, restoreQueue, showError]);

  // Queued messages are saved with their sessions in the workspace, so a changed queue saves it.
  useEffect(() => {
    if (!restored || !settings.restoreSessions) return;
    const timer = window.setTimeout(() => {
      saveWorkspaceNow().catch(() => undefined);
    }, WORKSPACE_SAVE_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [state.sessions, state.activeId, queue, restored, settings.restoreSessions, saveWorkspaceNow]);

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
  // The done notice is a passing hint: it fades on its own (and on the session's next work), but
  // not while the pointer or the keyboard is on it, so its button stays reachable.
  const doneNoticeHeld = doneNotice !== null && heldDoneNotice === doneNotice;
  useEffect(() => {
    if (doneNotice === null || doneNoticeHeld) return;
    const timer = window.setTimeout(() => {
      setDoneNotice(null);
    }, DONE_NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [doneNotice, doneNoticeHeld]);
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
  /** Panel window events: the current handlers, for the listener attached once below. */
  const notesChangedRef = useRef<() => void>(() => undefined);
  /** The panel window loaded: it gets the context now. */
  const panelReadyRef = useRef<() => void>(() => undefined);
  const sendFromNotesRef = useRef<(text: string, sessionId: number | null) => void>(
    () => undefined,
  );
  /** Resolves the wait on quit once the panel window reports that it saved. */
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
      listen(PANEL_MESSAGE, (event) => {
        // Checked by the backend (panel_to_main) and here again before anything happens.
        const message = parsePanelMessage(event.payload);
        if (message?.kind === "sendToTerminal") {
          sendFromNotesRef.current(message.text, message.sessionId);
        } else if (message?.kind === "notesChanged") {
          notesSavedRef.current?.();
          notesSavedRef.current = null;
          notesChangedRef.current();
        } else if (message?.kind === "showSession") {
          // The backend has already brought this window to the front.
          activateRef.current(message.sessionId);
        } else if (message?.kind === "ready") {
          panelReadyRef.current();
        }
      })
        .then(keep)
        .catch(() => undefined);
      // A bare signal: the messages carry an agent's text, so they come from a main-only command.
      listen(PARTNER_MESSAGES_CHANGED, () => {
        loadWaitingRef.current();
      })
        .then(keep)
        .catch(() => undefined);
      // A bare signal as well: link messages carry an agent's text.
      listen(LINK_GROUPS_CHANGED, () => {
        loadLinksRef.current();
      })
        .then(keep)
        .catch(() => undefined);
      // A bare signal: the results name program paths, so they come from a main-only command.
      listen(PROGRAM_UPDATE_RESULT, () => {
        api
          .takeUpdateResults()
          .then((results) => {
            if (!Array.isArray(results)) return;
            for (const result of results) {
              const outcome = parseUpdateOutcome(result);
              if (outcome !== null) updateResultRef.current(outcome);
            }
          })
          .catch(() => undefined);
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

  /** The running session whose pane lies at this point (CSS pixels); none while a dialog is open. */
  const liveSessionAt = (x: number, y: number) => {
    const id = modalRef.current ? null : paneAt(x, y);
    const session = id === null ? undefined : stateRef.current.sessions.find((s) => s.id === id);
    return session && session.status !== "exited" ? session : undefined;
  };

  /**
   * A path from the Files tab into a session (the active one for `null`), typed for its program:
   * an @ mention for agents, the shell's own form for shells, relative to the session's folder.
   */
  const insertPath = (item: TreePath, sessionId: number | null) => {
    const id = sessionId ?? activeIdRef.current;
    const session = stateRef.current.sessions.find((s) => s.id === id);
    if (session === undefined) return;
    const profile = profilesRef.current.find((p) => p.id === session.profileId);
    const text = insertPathText(item, session.cwd, shellKind(profile?.command ?? ""));
    if (text === "") return;
    sendFromNotes(text, session.id);
    countFeature("pathMentioned");
  };

  /** A row of the file tree dragged onto a pane: its path goes into that session. */
  const dragPath = (drag: PathDrag) => {
    const target = drag.phase === "cancel" ? undefined : liveSessionAt(drag.x, drag.y);
    if (drag.phase === "move") {
      setDropTarget(target?.id ?? null);
      return;
    }
    setDropTarget(null);
    if (drag.phase !== "drop" || target === undefined) return;
    insertPath(drag.item, target.id);
    focusTerminal();
  };

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
      const target = liveSessionAt(event.position.x / ratio, event.position.y / ratio);
      if (event.type !== "drop") {
        setDropTarget(target?.id ?? null);
        return;
      }
      setDropTarget(null);
      const terminal = target ? terminals.get(target.id) : undefined;
      if (!target || !terminal) return;
      const profile = profilesRef.current.find((p) => p.id === target.profileId);
      const text = dropText(event.paths, shellKind(profile?.command ?? ""));
      if (event.paths.some(unsafeDropPath)) {
        setStartHint(createTranslator(settingsRef.current.language)("paste.dropSkipped"));
      }
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

  /** Notes changed here: the panel window reloads too. */
  const notesChangedHere = useCallback(() => {
    refreshNoteFolders();
    if (loadWindowPrefs().detached) {
      emitTo(PANEL_WINDOW_LABEL, PANEL_COMMAND, { kind: "refresh" }).catch(() => undefined);
    }
  }, [refreshNoteFolders]);

  // What the detached panel shows, computed here: this window stays the single source of truth.
  const activeCwd = activeSession?.cwd ?? null;
  const panelTrigger = activeSession ? (attention.get(activeSession.id) ?? "idle") : "none";
  const panelSessions = useMemo(
    () =>
      state.sessions.map((session, index) => ({
        id: session.id,
        title: session.title,
        running: session.status !== "exited",
        program: localSessions[index]?.program ?? "",
        repo: localSessions[index]?.repo ?? null,
        agent: runsAgent(
          profiles.find((profile) => profile.id === session.profileId)?.command,
          session.kind,
        ),
        shell: shellKind(
          profiles.find((profile) => profile.id === session.profileId)?.command ?? "",
        ),
      })),
    [state.sessions, localSessions, profiles],
  );
  /** The last payload sent, as JSON: an unchanged context is not sent again. */
  const panelSent = useRef("");
  const pushPanelContext = useCallback((context: PanelContext, force: boolean) => {
    const payload = panelContextPayload(context);
    const text = JSON.stringify(payload);
    if (!force && text === panelSent.current) return;
    panelSent.current = text;
    // The panel may address this session only; the backend learns it before the panel does.
    api
      .setPanelSession(context.activeId)
      .catch(() => undefined)
      .then(() => emitTo(PANEL_WINDOW_LABEL, PANEL_CONTEXT, payload))
      .catch(() => undefined);
  }, []);
  const teamActivities = teamStore.activities;
  useEffect(() => {
    if (!panelWindow.detached) return;
    pushPanelContext(
      {
        activeId: state.activeId,
        cwd: activeCwd,
        trigger: panelTrigger,
        sessions: panelSessions,
        claims,
        activities: teamActivities,
      },
      false,
    );
  }, [
    panelWindow.detached,
    state.activeId,
    activeCwd,
    panelTrigger,
    panelSessions,
    claims,
    teamActivities,
    pushPanelContext,
  ]);
  useEffect(() => {
    panelReadyRef.current = () => {
      pushPanelContext(
        {
          activeId: state.activeId,
          cwd: activeCwd,
          trigger: panelTrigger,
          sessions: panelSessions,
          claims,
          activities: teamActivities,
        },
        true,
      );
    };
  });

  /** New prefs from the panel window; it closed: the panel is docked here again, same tab. */
  const panelWindowRef = useRef(panelWindow);
  useEffect(() => {
    panelWindowRef.current = panelWindow;
  });
  const adoptPanelPrefs = useCallback((next: PanelWindowPrefs) => {
    if (panelWindowRef.current.detached && !next.detached) {
      setLayout((current) => ({ ...current, sideOpen: true, sideTab: next.tab }));
    }
    panelWindowRef.current = next;
    setPanelWindow(next);
  }, []);

  // Closing the panel window (its own close or dock button) brings the panel back here.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === WINDOW_PREFS_KEY) adoptPanelPrefs(loadWindowPrefs());
    };
    // A window that closed without telling (crash): the panel is docked again.
    const onFocus = () => {
      if (!loadWindowPrefs().detached) return;
      Window.getByLabel(PANEL_WINDOW_LABEL)
        .then((found) => {
          if (found === null) adoptPanelPrefs(updateWindowPrefs({ detached: false }));
        })
        .catch(() => undefined);
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("focus", onFocus);
    };
  }, [adoptPanelPrefs]);

  // Detached last time: the panel window comes back with the app.
  useEffect(() => {
    const prefs = loadWindowPrefs();
    if (prefs.detached) api.openPanelWindow(prefs.onTop, prefs.bounds).catch(() => undefined);
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

  /** Command for the detached panel window; the window opens (or comes to the front) first. */
  const toPanelWindow = (command: PanelCommand | null) => {
    const prefs = loadWindowPrefs();
    api
      .openPanelWindow(prefs.onTop, prefs.bounds)
      .then(() => (command ? emitTo(PANEL_WINDOW_LABEL, PANEL_COMMAND, command) : undefined))
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
    if (panelWindow.detached) {
      toPanelWindow({ kind: "quickNote" });
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
    if (panelWindow.detached) {
      toPanelWindow({ kind: "show", tab: "notes" });
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
    if (panelWindow.detached) {
      toPanelWindow({ kind: "show", tab: "changes" });
      return;
    }
    if (layout.sideOpen && layout.sideTab === "changes") {
      closeSide();
      return;
    }
    openSide("changes");
    setChangesFocusToken((token) => token + 1);
  };

  /** The folder button on the active session row: its file tree in the right panel, or away. */
  const toggleFiles = () => {
    if (panelWindow.detached) {
      toPanelWindow({ kind: "show", tab: "files" });
      return;
    }
    if (filesShown) {
      closeSide();
      return;
    }
    openSide("files");
    setFilesFocusToken((token) => token + 1);
  };

  /** Quick access chose a commit: the history of the changes panel opens on it. */
  const openCommit = (sha: string) => {
    if (panelWindow.detached) {
      // The panel window keeps its own view; it shows the changes, the commit stays a click away.
      toPanelWindow({ kind: "show", tab: "changes" });
      return;
    }
    setChangesView("commits");
    setCommitFocus((current) => ({ sha, token: (current?.token ?? 0) + 1 }));
    openSide("changes");
  };

  /** Quick access chose a file of the active session: the read-only viewer shows it. */
  const openFile = (sessionId: number, path: string) => {
    rememberFile(sessionId, path);
    api.openFileViewer(sessionId, path).catch((e: unknown) => {
      setError(t("files.openFailed", { error: errorText(e) }));
    });
  };

  /** Search across sessions, seeded with the visible terminal's selection. */
  const openSearchAll = () => {
    const id = state.activeId;
    setSearchAllSeed(id === null ? "" : (terminals.get(id)?.searchSeed() ?? ""));
    setDialog("searchAll");
  };

  /** A hit of the search across sessions: show its session and select the spot. */
  const revealMatch = (match: LineMatch) => {
    setDialog(null);
    dispatch({ type: "activated", id: match.sessionId });
    // After the session is shown, so the terminal has its size and can scroll.
    requestAnimationFrame(() => {
      const terminal = terminals.get(match.sessionId);
      if (!terminal) return;
      if (!terminal.revealMatch(match)) setStartHint(t("searchAll.gone"));
      terminal.focus();
    });
  };

  /**
   * "Add to notes" of the daily review: a new note in General, because a day spans several
   * projects; then the notes show that area.
   */
  const saveReviewNote = async (title: string, markdown: string) => {
    const folder: NoteFolder = { path: null };
    const id = await api.createNote(folder, title);
    if (typeof id !== "string") throw new Error(t("error.unknown"));
    await api.writeNote(folder, id, markdown);
    notesChangedHere();
    setNotesRefresh((value) => value + 1);
  };

  /** Quick access "Notes in …": that project's notes. */
  const openNotes = (folder: NoteFolder) => {
    if (panelWindow.detached) {
      toPanelWindow({ kind: "show", tab: "notes" });
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "openFolder", folder }));
  };

  /** A note quick access found by its text: it opens in the notes, here or in their window. */
  const openNoteAt = (hit: SearchHit) => {
    if (panelWindow.detached) {
      toPanelWindow({ kind: "openNote", folder: hit.folder, id: hit.id });
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "openNote", folder: hit.folder, id: hit.id }));
  };

  /**
   * "Open in own window": the whole right panel moves into a window of its own, on the same tab;
   * the terminal gets the space. Pending note input is saved first, the window reads it again.
   */
  const detachPanel = () => {
    const tab = layout.sideTab;
    void flushNotes().then(() => {
      setPanelWindow(updateWindowPrefs({ detached: true, tab }));
      setLayout((current) => ({ ...current, sideOpen: false }));
      panelSent.current = "";
      toPanelWindow({ kind: "show", tab });
      countFeature("sidePanelDetached");
      focusTerminal();
    });
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
    if (panelWindow.detached) {
      toPanelWindow({ kind: "capture", text, source });
      return;
    }
    openSide("notes");
    askNotes((token) => ({ token, kind: "capture", text, source }));
  };

  /** "Save as snippet": the selection, cleaned of terminal artifacts, into a dialog to confirm. */
  const saveSelectionAsSnippet = () => {
    const text = snippetTextFromSelection(activeTerminal()?.selectionText() ?? "");
    if (text !== "") setSnippetDraft(text);
  };

  /**
   * F6 and Shift+F6 while the notes are open: terminal, list, editor. Returns whether it moved
   * the focus (otherwise F6 stays with the program in the terminal).
   */
  const cycleFocus = (step: 1 | -1, fromTerminal: boolean): boolean => {
    const panel = notesHandle.current;
    if (!layout.sideOpen || layout.sideTab !== "notes" || panelWindow.detached || !panel) {
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
      // Without "Remember sessions" they do not come back, nor do their timelines; otherwise
      // the saved state keeps them.
      if (settingsRef.current.restoreSessions) return;
      results.forEach((result, index) => {
        const id = ids[index];
        if (result.status === "fulfilled" && id !== undefined) {
          api.snapshotForget(id).catch(() => undefined);
        }
      });
    });
    focusActive();
  };

  /**
   * The globe on a session row: its dev server in the default browser. Without the backend
   * command (older backend) the address shows in the link notice to copy, as for terminal links.
   */
  const openDevServer = (id: number) => {
    const url = devServers.get(id);
    if (url === undefined) return;
    api.openLocalUrl(url).catch(() => {
      setLink({ url, copied: false });
    });
  };

  /** "Show changes" in the done notice: that session, with its changes in the right panel. */
  const showDoneChanges = (id: number) => {
    setDoneNotice(null);
    if (!stateRef.current.sessions.some((s) => s.id === id)) return;
    dispatch({ type: "activated", id });
    if (panelWindow.detached) {
      toPanelWindow({ kind: "show", tab: "changes" });
      return;
    }
    openSide("changes");
    setChangesFocusToken((token) => token + 1);
  };

  /** "Open" on a link whose visible text differed: the user saw the real address and agreed. */
  const openCheckedLink = () => {
    if (link === null) return;
    const url = link.url;
    setLink(null);
    focusTerminal();
    api.openWebUrl(url).catch(() => {
      setLink({ url, copied: false });
    });
  };

  const copyLink = () => {
    if (link === null) return;
    const url = link.url;
    writeClipboard(url)
      .then(() => {
        setLink((current) => (current?.url === url ? { ...current, copied: true } : current));
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

  /** Light and dark switch (sidebar footer, menu); if the setting is System, it becomes a fixed choice. */
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
  /**
   * Saves a session's conversation as a Markdown report. The backend reads the agent's transcript
   * and asks for the place in its save dialog; a cancelled dialog is no error.
   */
  const exportReport = (id: number) => {
    const session = stateRef.current.sessions.find((s) => s.id === id);
    if (session === undefined) return;
    const now = new Date();
    api
      .exportReport(
        id,
        session.title,
        exportFileName(session.title, now, "report"),
        utcOffsetMinutes(now),
        reportLabels(t),
      )
      .then(parseExportResult)
      .catch(showError)
      .finally(focusActive);
  };
  /**
   * The values of snippet variables for one session, read right now (snippetVariables.ts).
   * Anything unavailable stays out; git errors count as unavailable.
   */
  const variableValues = async (
    id: number,
    names: readonly SnippetVariable[],
  ): Promise<VariableValues> => {
    const session = stateRef.current.sessions.find((s) => s.id === id);
    if (session === undefined) return {};
    const cwd = session.cwd;
    const values: VariableValues = {};
    const reads = names.map(async (name) => {
      switch (name) {
        case "folder":
          values.folder = cwd;
          break;
        case "selection":
          values.selection = terminals.get(id)?.selectionText() ?? "";
          break;
        case "file": {
          const path = fileOf(id);
          if (path !== null) values.file = mentionText(path, false).trimEnd();
          break;
        }
        case "branch": {
          const branches = parseBranches(await api.gitBranches([cwd]).catch(() => null));
          const found = [...branches].find(([path]) => samePath(path, cwd));
          const branch = found?.[1] ?? null;
          if (branch !== null) values.branch = branch;
          break;
        }
        case "diff": {
          const diff = parseGitDiff(await api.gitDiff(id).catch(() => null));
          if (diff?.isRepo === true && diff.diff.trim() !== "") {
            values.diff = capDiff(diff.diff, (count) => t("snippets.diffCut", { count }));
          }
          break;
        }
      }
    });
    await Promise.all(reads);
    return values;
  };

  /** Fills a text's variables for one session; text without variables passes unchanged. */
  const fillVariables = async (id: number, text: string): Promise<Filled> => {
    const used = usedVariables(text);
    // Unknown names stay as written (Jinja or Mustache examples in a prompt).
    if (used.known.length === 0) return { ok: true, text };
    const result = expandVariables(text, await variableValues(id, used.known));
    if (result.ok) return { ok: true, text: result.text };
    return {
      ok: false,
      details: missingSummary(result.missing, (name) => t(MISSING_REASONS[name])),
    };
  };

  /**
   * Snippet into the visible session, like pasting: bracketed paste, no Enter. Its variables are
   * filled first; when one has no value, nothing is pasted and a notice names what is missing.
   */
  const insertSnippet = (snippet: Snippet) => {
    const session = activeSession;
    const terminal = activeTerminal();
    if (!session || session.status === "exited" || !terminal) return;
    const id = session.id;
    void fillVariables(id, snippet.text).then((filled) => {
      if (filled.ok) pasteIntoSession(id, filled.text);
      else setError(t("snippets.notSent", { details: filled.details }));
    });
  };

  /**
   * Pastes filled text into one session and sends Enter after it (broadcast.ts). Resolves to
   * `false` only when the text was held back: several lines or hidden characters for a program
   * without bracketed paste (`blockedFor`). A session closed meanwhile gets nothing, as before. A
   * paste the program did not take gets no Enter and counts as sent: the terminal itself says so
   * (`InputStalled`), and sending it again could repeat the part that did arrive.
   */
  const submitText = async (id: number, text: string): Promise<boolean> => {
    const terminal = terminals.get(id);
    if (!terminal) return true;
    try {
      const blocked = await sendToTargets([terminal], text, () => delay(BROADCAST_ENTER_DELAY_MS));
      return blocked.length === 0;
    } catch {
      return true;
    }
  };

  /** Says which sessions got nothing because of `blockedFor`. */
  const reportBlocked = (ids: readonly number[]) => {
    if (ids.length === 0) return;
    setError(t("send.blocked", { sessions: ids.map(sessionName).join(", ") }));
  };

  /**
   * Text to several sessions: each gets it with its own variable values as a bracketed paste,
   * then Enter as a separate write. Afterwards the split view shows up to four of them. When a
   * session lacks a value nothing is sent at all, and the answer names the session and the gap.
   */
  const sendToSeveral = async (text: string, ids: number[]): Promise<string | null> => {
    const filled = await Promise.all(
      ids.map(async (id) => ({ id, result: await fillVariables(id, text) })),
    );
    for (const { id, result } of filled) {
      if (!result.ok) {
        return t("snippets.notSentIn", { session: sessionName(id), details: result.details });
      }
    }
    setDialog(null);
    const sent = filled.map(async ({ id, result }) =>
      result.ok && !(await submitText(id, result.text)) ? id : null,
    );
    void Promise.all(sent).then((blocked) => {
      reportBlocked(blocked.filter((id) => id !== null));
    });
    splitView.showOnly(ids);
    countFeature("broadcastSent");
    return null;
  };

  /**
   * A queued message whose session is done: the same path as "Send to several" (variables filled,
   * bracketed paste, then Enter). A missing variable sends nothing; the message stays queued and
   * a notice names the gap.
   */
  const sendQueued = (id: number, text: string) => {
    const key = terminals.get(id)?.key;
    void fillVariables(id, text).then((filled) => {
      if (filled.ok) {
        // A message that cannot go in safely stays queued, like one with a missing variable.
        void submitText(id, filled.text).then((sent) => {
          if (sent) return;
          if (key !== undefined) changeQueue(queueMessage(queueRef.current, key, text));
          reportBlocked([id]);
        });
        return;
      }
      if (key !== undefined) changeQueue(queueMessage(queueRef.current, key, text));
      setError(t("snippets.notSentIn", { session: sessionName(id), details: filled.details }));
    });
  };
  // The attention listener lives outside React; it always reaches the newest values.
  useEffect(() => {
    sendQueuedRef.current = sendQueued;
  });

  /** The session may show a question, so a command's Enter would answer it (`asksQuestion`). */
  const sessionAsks = (id: number) =>
    asksQuestion(reasons.get(id) ?? null, () => terminals.get(id)?.screenLines() ?? []);
  // A delivery that waits for a long time asks again with the newest values, not its render's.
  const sessionAsksRef = useRef(sessionAsks);
  useEffect(() => {
    sessionAsksRef.current = sessionAsks;
  });

  /**
   * An agent command into one running session, sent like "Send to several" (agentCommands.ts);
   * that session shows afterwards, so its answer is in view.
   */
  const sendAgentCommand = (id: number, command: AgentCommand) => {
    const session = state.sessions.find((s) => s.id === id);
    const terminal = terminals.get(id);
    if (session?.status !== "running" || !terminal || withholdCommand(id, command)) return;
    // A stalled paste is reported in the terminal itself.
    sendCommand(terminal, command, () => delay(BROADCAST_ENTER_DELAY_MS)).then(
      (sent) => {
        if (!sent) reportBlocked([id]);
      },
      () => undefined,
    );
    dispatch({ type: "activated", id });
    focusTerminal();
  };

  /**
   * The session may show a question (agentCommands.ts `asksQuestion`): the command stays unsent,
   * since its Enter would answer that question, and a notice says why nothing happened. Menu and
   * quick access hide the commands then; this catches the row's compact button, which cannot
   * watch the screen, and a question that came up after the menu opened.
   */
  const withholdCommand = (id: number, command: AgentCommand) => {
    if (!sessionAsks(id)) return false;
    setError(t("agentCommand.asksFirst", { session: sessionName(id), command: command.text }));
    return true;
  };

  /** From the menu or quick access: commands that throw the conversation away ask first. */
  const runAgentCommand = (id: number, command: AgentCommand) => {
    if (withholdCommand(id, command)) return;
    if (command.confirm) setCommandAsk({ id, command });
    else sendAgentCommand(id, command);
  };

  /**
   * The row's compact button. It is hidden while the agent is busy; should the agent get busy
   * between render and click, nothing is sent (agentCommands.ts `commandBusy`).
   */
  const compactFromRow = (id: number, command: AgentCommand, at: number) => {
    if (commandBusy(attention.get(id) ?? "idle", reasons.get(id) ?? null)) return;
    // The button stays until the agent's answer marks it as working; a double click in between
    // would otherwise queue a second compact.
    if (compactRepeated(compactSentAt.current.get(id), at)) return;
    compactSentAt.current.set(id, at);
    runAgentCommand(id, command);
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

  // Taskbar badge with the number of waiting sessions, in the color of "done".
  const badgeColor = resolvedColor(settings.looks, "done", theme);
  useEffect(() => {
    badge.update(waitingCount, badgeColor);
  }, [badge, waitingCount, badgeColor]);

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
  /** The mode of the next start in the target: team only where a worktree is possible. */
  const nextMode = teamOffered ? effectiveMode(startMode, worktree) : "private";
  /**
   * Start in the target as the switch says. The sidebar greys out programs that cannot be team
   * sessions; a shortcut or menu start of one of them runs private instead of doing nothing.
   */
  const startHere = (profile: Profile) => {
    // Until the backend answered, the mode and folder are the defaults, not the user's: a start
    // then could run private (without the gate) in the home folder.
    if (!prefsReady) return;
    void start(profile, undefined, startModeFor(nextMode, profile) ?? "private");
  };

  // Sub-agent definitions (subAgentLibrary.ts): those of the start places and the sessions'
  // folders, read again whenever that set of folders changes.
  const agentLibrary = useSubAgentLibrary();
  const libraryKey = [...places, ...state.sessions.map((session) => session.cwd)].join("\n");
  useEffect(() => {
    setLibraryFolders(libraryKey === "" ? [] : libraryKey.split("\n"));
  }, [libraryKey]);
  const subAgentIcon = (def: AgentDef) => (
    <AgentFigure
      figure={defFigure(def, agentLibrary.library, agentLibrary.figures)}
      size={26}
      crop="full"
    />
  );
  /** What the sidebar and Ctrl+Shift+O offer to reopen: nothing that is open already. */
  const recentShown = shownRecent(recentSessions, state.sessions);
  /** The program the next start would use, once the preferences and programs are read. */
  const seedProgram = profilesReady && prefsReady ? nextProgram?.id : undefined;
  /**
   * Reads the recently opened sessions once the preferences and programs are known. When the
   * backend holds none yet, the first list (once per machine) is the recent folders with
   * `seedProgram`, so "Recently opened" offers something from the start; a list from before
   * marks that as done, so a list emptied by hand stays empty.
   */
  useEffect(() => {
    if (recentRequested.current || !prefsReady || !profilesReady) return;
    recentRequested.current = true;
    const recentFolders = folders.recent;
    void loadRecentSessions().then((stored) => {
      savedRecent.current = stored;
      const storage = browserStorage();
      const first = stored.length === 0 && needsSeed(storage);
      markSeeded(storage);
      const base = first ? seedRecent(recentFolders, seedProgram) : stored;
      // Sessions started or closed before the answer came go in front.
      setRecentSessions((current) =>
        current.reduceRight((list, entry) => pushRecent(list, entry), base),
      );
    });
  }, [prefsReady, profilesReady, folders.recent, seedProgram]);
  /**
   * Starts a recently opened session again: the same program in the same folder, privately,
   * with its title and marker, resuming its conversation (without a known one, the start
   * resumes the program's last conversation in the folder, or starts fresh). A program that is
   * gone says so and its entry leaves the list; a folder that is gone gives the start's own
   * error.
   */
  const reopenRecent = (entry: RecentSession) => {
    // Until the profiles are read, every program would look gone and the entry would be lost.
    if (reopening.current || profiles.length === 0) return;
    const profile = profiles.find((p) => p.id === entry.profileId);
    if (profile?.installed !== true) {
      setRecentSessions((current) => withoutRecent(current, entry));
      setError(t("recentlyOpened.programMissing", { name: profile?.name ?? entry.profileId }));
      return;
    }
    reopening.current = true;
    // An entry taken over from the recent folders never ran here: a fresh start, named after
    // its program. Any other one resumes as a restore does.
    const seeded = entry.closedAt === UNKNOWN_TIME;
    void start(
      profile,
      entry.cwd,
      "private",
      seeded ? undefined : entry.title,
      undefined,
      undefined,
      null,
      seeded ? null : { cliSessionId: entry.cliSessionId },
    )
      .then((id) => {
        // It did not start (a folder that is gone): not offered again.
        if (id === null) setRecentSessions((current) => withoutRecent(current, entry));
        if (id !== null && (entry.color !== null || entry.emoji !== null)) {
          dispatch({ type: "marked", id, marker: { color: entry.color, emoji: entry.emoji } });
        }
      })
      .finally(() => {
        reopening.current = false;
      });
  };

  // Agent arena (arena.ts, ArenaDialog.tsx, backend arena.rs).

  /** Installed agents that can take part: programs that are not a known shell. */
  const arenaAgents = startable.filter((profile) => shellKind(profile.command) === "agent");
  /** Folders an arena can start in: the start places that lie in a git repository. */
  const arenaFolders = places.filter((path) => worktreeChance(branches, path) === "yes");
  const arenaOpenable = arena !== null || arenaAgents.length >= 2;
  const openArena = () => {
    setDialog("arena");
  };

  /** Changes one contestant; the state stays the same object when nothing changed. */
  const updateContestant = useCallback((id: number, change: (c: Contestant) => Contestant) => {
    setArena((current) => {
      if (current === null) return current;
      const contestants = current.contestants.map((c) => (c.sessionId === id ? change(c) : c));
      const changed = contestants.some((c, index) => c !== current.contestants[index]);
      return changed ? { ...current, contestants } : current;
    });
  }, []);

  // Working and finish times follow the attention changes of the contestants' sessions.
  useEffect(() => {
    arenaAttention.current = (id, next) => {
      updateContestant(id, (c) => trackAttention(c, next, Date.now()));
    };
  }, [updateContestant]);

  /** Pastes the task into a contestant's session, then Enter (as "Send to several" does). */
  const sendArenaTask = useCallback(
    (id: number) => {
      const terminal = terminals.get(id);
      const prompt = arenaRef.current?.prompt;
      if (!terminal || prompt === undefined || terminal.exitCode !== undefined) return;
      arenaSent.current.add(id);
      // An agent that is still busy when its task arrives counts as working on it.
      const state = terminal.attention.state;
      updateContestant(id, (c) =>
        c.promptSent ? c : trackAttention({ ...c, promptSent: true }, state, Date.now()),
      );
      void sendToTargets([terminal], prompt, () => delay(BROADCAST_ENTER_DELAY_MS));
    },
    [updateContestant],
  );

  // The task goes in once a contestant's start screen is quiet (promptReady).
  const waitingForTask =
    arena?.contestants
      .filter((c) => !c.promptSent)
      .map((c) => c.sessionId)
      .join(",") ?? "";
  useEffect(() => {
    if (waitingForTask === "") return;
    const ids = waitingForTask.split(",").map(Number);
    const timer = window.setInterval(() => {
      const now = performance.now();
      for (const id of ids) {
        const terminal = terminals.get(id);
        const startedAt = arenaStarts.current.get(id);
        if (!terminal || startedAt === undefined || arenaSent.current.has(id)) continue;
        const lastOutput = terminal.lastOutputAt > 0 ? terminal.lastOutputAt : null;
        if (promptReady(startedAt, lastOutput, now)) sendArenaTask(id);
      }
    }, ARENA_POLL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [waitingForTask, sendArenaTask]);

  /** Runs the test command in a contestant's worktree (backend, argv without a shell). */
  const runArenaTests = useCallback(
    (id: number) => {
      const current = arenaRef.current;
      const contestant = current?.contestants.find((c) => c.sessionId === id);
      if (!current || !contestant || current.testCommand === "") return;
      if (contestant.tests.kind === "running") return;
      updateContestant(id, (c) => ({ ...c, tests: { kind: "running" }, testedFor: c.doneAt }));
      api
        .arenaRunTests(contestant.worktree, current.testCommand)
        .then((value) => {
          const run = parseTestRun(value);
          updateContestant(id, (c) => ({
            ...c,
            tests:
              run === null ? { kind: "error", message: t("error.unknown") } : { kind: "done", run },
          }));
        })
        .catch((e: unknown) => {
          updateContestant(id, (c) => ({
            ...c,
            tests: { kind: "error", message: arenaErrorText(e, t) },
          }));
        });
    },
    [updateContestant, t],
  );

  // Tests run by themselves once per finish.
  useEffect(() => {
    if (arena === null || arena.testCommand === "") return;
    for (const c of arena.contestants) {
      const session = state.sessions.find((s) => s.id === c.sessionId);
      if (session === undefined) continue;
      const now = contestantState(c, attention.get(c.sessionId), session.status === "exited");
      if (testsDue(c, now, arena.testCommand)) runArenaTests(c.sessionId);
    }
  }, [arena, attention, state.sessions, runArenaTests]);

  /**
   * One worktree per agent from the current commit (backend), then one private session in each,
   * shown side by side. The task follows per session once its agent is ready.
   */
  const startArena = async (input: ArenaInput): Promise<string | null> => {
    const picked = input.profileIds.flatMap((id) => {
      const profile = arenaAgents.find((agent) => agent.id === id);
      return profile ? [profile] : [];
    });
    let setup;
    try {
      setup = parseArena(
        await api.arenaCreate(
          input.folder,
          picked.map((profile) => profile.name),
        ),
      );
    } catch (e) {
      return arenaErrorText(e, t);
    }
    if (setup === null) return t("error.unknown");
    const failures: string[] = [];
    const contestants: Contestant[] = [];
    for (const [index, profile] of picked.entries()) {
      const worktree = setup.worktrees[index];
      if (worktree === undefined) continue;
      const started: number[] = [];
      const ok = await start(
        profile,
        input.folder,
        "private",
        t("arena.sessionTitle", { name: profile.name }),
        (text) => {
          failures.push(text);
        },
        {
          worktree,
          fresh: true,
          onStarted: (id) => {
            started.push(id);
            arenaStarts.current.set(id, performance.now());
          },
        },
      );
      const id = started[0];
      if (ok !== null && id !== undefined) {
        contestants.push(newContestant(id, profile.id, profile.name, worktree));
      }
    }
    // Worktrees whose session did not start were taken away again by `start`.
    const lastError = failures.at(-1);
    if (contestants.length === 0) return lastError ?? t("error.unknown");
    setArena(arenaRun(setup, input, contestants));
    setDialog(null);
    splitView.showOnly(contestants.map((c) => c.sessionId));
    if (lastError !== undefined) setError(lastError);
    else setStartHint(t("arena.started"));
    return null;
  };

  const applyArenaWinner = async (id: number, files: string[]): Promise<ApplyOutcome | string> => {
    const current = arenaRef.current;
    const contestant = current?.contestants.find((c) => c.sessionId === id);
    if (!current || !contestant) return t("error.unknown");
    try {
      const outcome = parseApplyOutcome(
        await api.arenaApply(contestant.worktree, current.baseCommit, files),
      );
      return outcome ?? t("error.unknown");
    } catch (e) {
      return arenaErrorText(e, t);
    }
  };

  /** After taking a winner over: the others close, their worktrees go with all changes. */
  const closeArenaOthers = (winnerId: number) => {
    const current = arenaRef.current;
    if (!current) return;
    const existing = new Set(stateRef.current.sessions.map((s) => s.id));
    for (const c of others(current, winnerId, existing)) void close(c.sessionId, true);
  };
  /**
   * Links the sessions a template started (ADR 0014): the agent sessions among them that got the
   * send client, with the member names and the lead the template keeps. Answers `null` once they
   * are linked, else why not.
   */
  const linkTemplateSessions = async (
    template: WorkspaceTemplate,
    started: readonly StartedMember[],
  ): Promise<string | null> => {
    const current = parseLinkState(await api.linkState().catch(() => null));
    const request = templateLinkRequest(started, current.sendable);
    if (request === null) return t("links.templateTooFew", { name: template.name });
    try {
      await api.linkSessions(request.ids, {
        names: request.names,
        lead: request.lead,
        askFirst: template.askFirst === true,
      });
      loadLinksRef.current();
      return null;
    } catch (e) {
      return errorText(e);
    }
  };
  /**
   * Starts every session of a workspace template, one after another so titles number in order.
   * Team sessions stay team sessions only where the folder is still a git repository and the
   * program can be one; programs that are gone are skipped. One message once all were tried:
   * a short hint when everything runs, otherwise one error with the count, the last failure and
   * the skipped programs. A template that links its sessions links them once all were tried.
   * A second click while a template starts does nothing.
   */
  const startTemplate = async (id: string) => {
    const template = templates.find((entry) => entry.id === id);
    if (template === undefined || templateStarting.current) return;
    templateStarting.current = true;
    try {
      // Without a team to be in, a template's team sessions start private (teamPlan.ts).
      const repos =
        teamOffered && template.sessions.some((entry) => entry.mode === "team")
          ? parseBranches(await api.gitBranches(templateFolders(template)).catch(() => null))
          : new Map<string, string | null>();
      const inRepo = (folder: string) =>
        [...repos].some(([path, branch]) => branch !== null && samePath(path, folder));
      const plan = planTemplate(template, startable, inRepo);
      let started = 0;
      let lastError: string | null = null;
      const members: StartedMember[] = [];
      for (const entry of plan.starts) {
        const ok = await start(entry.profile, entry.folder, entry.mode, entry.title, (text) => {
          lastError = text;
        });
        if (ok !== null) {
          started += 1;
          members.push({ id: ok, memberName: entry.memberName, role: entry.role });
        }
      }
      const linkProblem =
        template.link === true ? await linkTemplateSessions(template, members) : null;
      const report = templateReport(template.sessions.length, started, plan.skipped, lastError);
      if (report.kind === "started") {
        if (linkProblem !== null) setError(linkProblem);
        else if (template.link === true) {
          setStartHint(t("links.templateLinked", { name: template.name }));
        } else setStartHint(t("templates.started", { name: template.name }));
        return;
      }
      setError(
        [
          t("templates.partial", {
            name: template.name,
            started: report.started,
            total: report.total,
          }),
          report.lastError,
          report.skipped.length > 0
            ? t("templates.skipped", { names: report.skipped.join(", ") })
            : null,
          linkProblem,
        ]
          .filter((part): part is string => part !== null)
          .join(" "),
      );
    } finally {
      templateStarting.current = false;
    }
  };
  const saveTemplate = (name: string, sessions: TemplateSession[], link: TemplateLink) => {
    setTemplates((current) => storeTemplate(current, name, sessions, link));
    setTemplateDialog(null);
    setStartHint(t("templates.saved", { name: name.trim() }));
    focusTerminal();
  };
  const renamingTemplate =
    templateDialog?.kind === "rename"
      ? templates.find((entry) => entry.id === templateDialog.id)
      : undefined;
  const closeTemplateDialog = () => {
    setTemplateDialog(null);
    focusTerminal();
  };
  const openSaveTemplate =
    state.sessions.length === 0
      ? null
      : () => {
          setTemplateDialog({ kind: "save" });
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
          current.collapsed || !isGroupOpen(current, "newSession")
            ? { ...openGroup(current, "newSession"), collapsed: false }
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
        openSettings();
        break;
      case "nextWaiting": {
        // A pending decision comes first: its card takes the focus. Pressed again on the card,
        // the shortcut goes on to the next waiting session as usual.
        const onCard = (document.activeElement?.closest(".gate-card") ?? null) !== null;
        if (shownDecision !== null && !onCard) {
          if (shownDecision.sessionId !== state.activeId && shown.has(shownDecision.sessionId)) {
            dispatch({ type: "activated", id: shownDecision.sessionId });
          }
          setGateFocusToken((token) => token + 1);
          break;
        }
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
      case "reopenRecent": {
        const entry = recentShown[0];
        if (entry !== undefined) reopenRecent(entry);
        break;
      }
      case "find":
        openSearch();
        break;
      case "launcher":
        setDialog("launcher");
        break;
      case "searchAll":
        if (state.sessions.length > 0) openSearchAll();
        break;
      case "overview":
        if (state.sessions.length > 0) setDialog("overview");
        break;
      case "home":
        toggleHome();
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
  const notesDocked = notesShown;
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
  /** Link messages that wait for a click, as cards below their target's row. */
  const linkInboxRows = new Map(
    [
      ...heldBySession(
        linkState.messages.filter((message) => !hiddenLinks.has(message.id)),
        deliveringIds,
      ),
    ].map(([to, list]) => [
      to,
      list.map((message) => {
        // Hangar's own introduction is written for the agent; people get a summary of it.
        const intro =
          message.fromName === null
            ? introSummary(
                linkState,
                message,
                state.sessions,
                (profileId) => profileById.get(profileId)?.name,
              )
            : null;
        return {
          id: message.id,
          from: message.fromName ?? "hangar",
          to: String(message.to),
          text: message.text,
          at: message.at,
          ...(message.secrets === undefined ? {} : { secrets: message.secrets }),
          ...(intro === null ? {} : { intro }),
          title:
            message.fromName === null
              ? t(INTRO_TITLES[introKind(message.text)])
              : t("links.from", { name: message.fromName }),
        };
      }),
    ]),
  );
  /** Where messages from partner projects wait: on each running agent session of their project. */
  const inboxRows = inboxBySession(
    waitingMessages.filter((message) => !hiddenWaiting.has(message.id)),
    partnerBook,
    state.sessions,
    (profileId) => profileById.get(profileId)?.command,
  );
  /** Every card on screen, row by row: when one appears or goes, all arm anew. */
  const cardsKey = [inboxRows, linkInboxRows]
    .map((rows) =>
      [...rows]
        .map(([to, list]) => `${String(to)}:${list.map((m) => String(m.id)).join(".")}`)
        .join(","),
    )
    .join("|");
  const badgeFor = (profileId: string) =>
    resolveBadge(
      profileId,
      profileById.get(profileId),
      // "Show program icons" off means letters for every program, bundled icons included.
      settings.showIcons
        ? { agent: agentIcon(profileId), program: programs.icons.get(profileId) }
        : {},
      settings.programs,
    );
  const openWhatsNew = () => {
    loadChangelog()
      .then((entries) => {
        setChangelog(entries);
        setWhatsNewSince(null);
        setDialog("whatsNew");
      })
      .catch(showError);
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
        {
          type: "action",
          label: t("menu.reopenRecent"),
          shortcut: keys("reopenRecent"),
          disabled: recentShown.length === 0,
          run: () => {
            perform("reopenRecent");
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
        // Hidden, not greyed out, while fewer than two agents are installed.
        ...(arenaOpenable
          ? [{ type: "action" as const, label: t("arena.menu"), run: openArena }]
          : []),
        {
          type: "action",
          label: t("templates.save"),
          disabled: openSaveTemplate === null,
          run: () => {
            openSaveTemplate?.();
          },
        },
        {
          type: "action",
          label: t("menu.worktreeCleanup"),
          run: () => {
            setDialog("worktreeCleanup");
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
            // Only for programs whose transcript Hangar reads; hidden for all others.
            ...(activeSession !== undefined && canExportReport(activeSession)
              ? [
                  {
                    type: "action" as const,
                    label: t("menu.exportReport"),
                    run: () => {
                      exportReport(activeSession.id);
                    },
                  },
                ]
              : []),
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
            openSettings();
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
          label: t("menu.searchAll"),
          shortcut: keys("searchAll"),
          disabled: state.sessions.length === 0,
          run: openSearchAll,
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
        {
          type: "action",
          label: putAside ? t("home.showSessions") : t("home.hideSessions"),
          disabled: state.sessions.length === 0,
          shortcut: keys("home"),
          run: toggleHome,
        },
        {
          type: "action",
          label: t("projects.menu"),
          run: showProjects,
        },
        {
          type: "action",
          label: isOneLine(settings.rowLayout) ? t("sidebar.oneLineOff") : t("sidebar.oneLineOn"),
          run: toggleRowsOneLine,
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
          label: t("costs.menu"),
          run: () => {
            setDialog("costs");
          },
        },
        {
          type: "action",
          label: t("menu.overview"),
          shortcut: keys("overview"),
          disabled: state.sessions.length === 0,
          run: () => {
            perform("overview");
          },
        },
        {
          type: "action",
          label: t("review.menu"),
          run: () => {
            setDialog("dailyReview");
          },
        },
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
  const crashNotice = crashRestore === null ? null : crashSummary(crashRestore, state.sessions);
  const closingSession =
    closingId === null ? undefined : state.sessions.find((s) => s.id === closingId);
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
    {
      type: "action",
      key: "toSnippet",
      label: t("snippets.saveSelectionMenu"),
      disabled: activeTerminal()?.hasSelection() !== true,
      run: saveSelectionAsSnippet,
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
  /** Queued message per session id, for the rows, the menu and quick access. */
  const queuedById = new Map<number, QueuedMessage>();
  for (const session of state.sessions) {
    const key = terminals.get(session.id)?.key;
    const entry = key === undefined ? undefined : queue.get(key);
    if (entry !== undefined) queuedById.set(session.id, entry);
  }
  /** What the queued-message entries offer for a session (sessionMenu.ts queueOffer). */
  const queueOfferFor = (session: SessionView) =>
    queueOffer({
      status: session.status,
      agent: runsAgent(profileById.get(session.profileId)?.command, session.kind),
      attentionEnabled: settings.attentionEnabled,
      state: attention.get(session.id) ?? "idle",
      queued: queuedById.has(session.id),
    });
  /** Discards the session's queued message. */
  const discardQueued = (id: number) => {
    const key = terminals.get(id)?.key;
    if (key !== undefined) changeQueue(discardMessage(queueRef.current, key));
  };
  const closeQueueEditor = () => {
    setQueueEditor(null);
    focusActive();
  };
  /**
   * Saves the editor's text. A session that finished while the editor was open (done, no
   * question) gets it right away instead of waiting for work that will not come.
   */
  const saveQueued = (id: number, text: string) => {
    const terminal = terminals.get(id);
    if (terminal === undefined) return;
    const session = state.sessions.find((s) => s.id === id);
    const saved = saveMessage(queueRef.current, terminal.key, text, {
      running: session?.status === "running",
      working: terminal.attention.state === "working",
      endedBy: terminal.attention.endedBy,
      gateWaiting: gateWaitingRef.current.has(id) || permissionWaitingRef.current.has(id),
      showsQuestion: looksLikeQuestion(terminal.screenLines()),
    });
    changeQueue(saved.queue);
    if (saved.send !== null) sendQueued(id, saved.send);
    closeQueueEditor();
  };
  /** The session is on screen (active or in a pane): its own terminal shows more than a peek. */
  const onScreen = (id: number) => id === state.activeId || placements.has(id);
  const runSessionAction = (
    action: SessionAction,
    id: number,
    anchor: { x: number; y: number } | null = null,
  ) => {
    switch (action) {
      case "queue":
        setQueueEditor({ id, anchor });
        break;
      case "discardQueue":
        discardQueued(id);
        break;
      case "peek":
        setPeekRequest((current) => ({ id, token: (current?.token ?? 0) + 1 }));
        break;
      case "interrupt":
        // ETX, as Ctrl+C typed in the terminal would send it; the hint starts over.
        stallMonitor.progressed(id);
        terminals.get(id)?.attention.interrupted();
        api.write(id, "\x03").catch(showError);
        break;
      case "rename":
        startRename(id);
        break;
      case "marker":
        setMarkerId(id);
        break;
      case "handover":
        setHandoverId(id);
        break;
      case "partner":
        setPartnerId(id);
        break;
      case "link":
        setLinkDialog({ kind: "link", id });
        break;
      case "linkEdit": {
        const group = groupOf(linkState, id);
        if (group !== undefined) setLinkDialog({ kind: "edit", group: group.id });
        break;
      }
      case "linkHistory": {
        const group = groupOf(linkState, id);
        if (group !== undefined) setLinkDialog({ kind: "history", group: group.id });
        break;
      }
      case "unlink": {
        const group = groupOf(linkState, id);
        if (group !== undefined) api.unlinkGroup(group.id).catch(showError);
        break;
      }
      case "report":
        exportReport(id);
        break;
      case "commands":
        if (sessionMenu !== null) setCommandMenu({ ...sessionMenu, id });
        break;
      case "subAgents":
        toggleSubAgents(id);
        break;
      case "crew":
        updateSettings({ crewColumn: !settings.crewColumn });
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
  /** The partner projects of a session's project (partners.ts); the backend checks again. */
  const partnersOfSession = (session: { cwd: string; worktree?: Worktree | undefined }) => {
    const own = projectOfSession(partnerBook, session);
    return own === undefined ? [] : partnersOf(partnerBook, own.name);
  };
  const sessionItems: PopupItem[] =
    menuSession === undefined
      ? []
      : sessionMenuEntries(menuSession, t, subAgentsOf === menuSession.id, {
          queue: queueOfferFor(menuSession),
          stall: stallOffer(
            stalls.has(menuSession.id) && attention.get(menuSession.id) === "working",
            onScreen(menuSession.id),
          ),
          // Only where another installed agent can take over (only offer what works).
          handover:
            canHandOver(menuSession, profileById.get(menuSession.profileId)?.command) &&
            handoverTargets(startable, menuSession.profileId).length > 0,
          // Only an agent session whose project has partners (only offer what works): the
          // backend refuses shells and updates as senders.
          partner:
            menuSession.kind !== "update" &&
            shellKind(profileById.get(menuSession.profileId)?.command ?? "") === "agent" &&
            partnersOfSession(menuSession).length > 0,
          // Only where another session could join; members edit or unlink their group.
          link: linkOffer(linkState, menuSession, state.sessions),
          commands:
            sessionCommands(menuSession, profiles).length > 0 && !sessionAsks(menuSession.id),
          crewShown: settings.crewColumn,
        }).map((entry): PopupItem => {
          if (entry.type === "separator") return entry;
          const action = entry.key;
          const id = menuSession.id;
          const anchor = sessionMenu === null ? null : { x: sessionMenu.x, y: sessionMenu.y };
          return {
            ...entry,
            run: () => {
              runSessionAction(action, id, anchor);
            },
          };
        });

  const markerSession =
    markerId === null ? undefined : state.sessions.find((s) => s.id === markerId);
  const handoverSession =
    handoverId === null ? undefined : state.sessions.find((s) => s.id === handoverId);
  const closeHandover = () => {
    setHandoverId(null);
    focusActive();
  };

  /** Asks the old agent for a summary (pasted, then Enter); the handover follows later. */
  const requestSummary = (id: number) => {
    const terminal = terminals.get(id);
    setHandoverId(null);
    if (!terminal) return;
    const pause = () => delay(BROADCAST_ENTER_DELAY_MS);
    void sendToTargets([terminal], summaryRequest(t), pause);
    setStartHint(t("handover.summaryAsked"));
    focusActive();
  };

  /**
   * Starts `profile` in the session's folder (same worktree and mode where possible), waits until
   * its input line is up and pastes the handover text without Enter.
   */
  const handOver = async (id: number, profile: Profile) => {
    const session = stateRef.current.sessions.find((s) => s.id === id);
    setHandoverId(null);
    if (session === undefined) return;
    const old = terminals.get(id);
    const plan = handoverStart(session, profile);
    const [diff, notes] = await Promise.all([
      api.gitDiff(session.id).catch(() => null),
      api.listNotes({ path: session.cwd }).catch(() => null),
    ]);
    const changed = parseGitDiff(diff);
    const noteList = parseNotes(notes)
      .sort((a, b) => a.order - b.order)
      .slice(0, HANDOVER_NOTES_MAX);
    const noteTexts = await Promise.all(
      noteList.map(async (note) => {
        const value = await api.readNote({ path: session.cwd }, note.id).catch(() => null);
        const content = typeof value === "string" ? value : "";
        return { title: note.title, excerpt: noteExcerpt(content, note.title) };
      }),
    );
    const text = buildHandover(
      {
        from: profileById.get(session.profileId)?.name ?? session.title,
        folder: plan.folder,
        lastPrompt: old?.lastRequest() ?? null,
        files: changed?.isRepo === true ? changed.files.map((file) => file.path) : [],
        notes: noteTexts,
      },
      t,
    );
    const newId = await start(
      profile,
      plan.folder,
      // Without a team to be in, the new agent runs private in the same worktree (teamPlan.ts).
      teamOffered ? plan.mode : "private",
      undefined,
      undefined,
      plan.worktree === undefined ? undefined : { worktree: plan.worktree, fresh: false },
    );
    if (newId === null) return;
    const terminal = terminals.get(newId);
    if (!terminal) return;
    const ready = await waitUntilReady(
      {
        bracketedPaste: () => terminal.bracketedPaste(),
        quietFor: () => {
          const last = terminal.activity().lastOutput;
          return last === null ? null : Date.now() - last;
        },
        gone: () => terminals.get(newId) !== terminal,
      },
      delay,
    );
    if (ready === "ready") pasteIntoSession(newId, text);
    else if (ready === "notReady") setUnpastedHandover({ text, program: profile.name });
  };

  const partnerSession =
    partnerId === null ? undefined : state.sessions.find((s) => s.id === partnerId);
  const closePartner = () => {
    setPartnerId(null);
    focusActive();
  };
  const titleOf = (id: number) =>
    stateRef.current.sessions.find((s) => s.id === id)?.title ?? `#${String(id)}`;

  /** Where a message from session `from` to `partner` would go now (partnerMessage.ts). */
  const resolvePartner = async (from: number, partner: string): Promise<TargetView> => {
    const targets = parsePartnerTargets(await api.partnerTargets(from, partner));
    if (targets === null) return { kind: "none" };
    const plan = deliveryPlan(
      targets.sessions,
      (profileId) => profileById.get(profileId)?.command,
      new Set(startable.map((profile) => profile.id)),
      nextProgram?.id ?? null,
    );
    switch (plan.kind) {
      case "one":
        return { kind: "one", id: plan.id, title: titleOf(plan.id) };
      case "choose":
        return {
          kind: "choose",
          sessions: plan.ids.map((id) => ({ id, title: titleOf(id) })),
        };
      case "start": {
        const profile = plan.profileId === null ? undefined : profileById.get(plan.profileId);
        return profile === undefined
          ? { kind: "none" }
          : {
              kind: "start",
              profileId: profile.id,
              program: profile.name,
              folder: targets.partner.folder,
            };
      }
    }
  };

  /**
   * Delivers a cross-project message (ADR 0012): starts the partner's program first where none
   * runs, holds the message while the target works, waits until it can take a paste like the
   * handover, and only then has the backend check, clean, head and log it. Enter follows only for
   * "Paste and send"; otherwise the target shows, so the person reads the text before sending.
   */
  const sendToPartner = async (from: number, send: PartnerSend) => {
    setPartnerId(null);
    // The text is never lost silently: wherever delivery stops, a notice offers to copy it.
    const keep = (message: string) => {
      setUnsentPartner({ text: send.text, message });
    };
    const unsent = (session: string) => {
      keep(t("partner.unsent", { session }));
    };
    let to: number | null;
    if (send.target.kind === "session") {
      to = send.target.id;
    } else {
      const profile = profileById.get(send.target.profileId);
      const targets = await api
        .partnerTargets(from, send.partner)
        .then(parsePartnerTargets, (e: unknown) => {
          showError(e);
          return null;
        });
      if (profile === undefined || targets === null) {
        unsent(send.partner);
        return;
      }
      to = await start(profile, targets.partner.folder, "private");
    }
    if (to === null) {
      unsent(send.partner);
      return;
    }
    const target = to;
    const name = titleOf(target);
    const terminal = terminals.get(target);
    if (!terminal) {
      unsent(name);
      return;
    }
    const gone = () => terminals.get(target) !== terminal;
    const result = await deliverMessage(
      {
        working: () => terminal.attention.state === "working",
        gone,
        onHeld: () => {
          setStartHint(t("partner.held", { session: name }));
        },
        ready: () =>
          waitUntilReady(
            {
              bracketedPaste: () => terminal.bracketedPaste(),
              quietFor: () => {
                const last = terminal.activity().lastOutput;
                return last === null ? null : Date.now() - last;
              },
              gone,
            },
            delay,
          ),
        bracketedPaste: () => terminal.bracketedPaste(),
        prepare: () => api.deliverPartnerMessage(from, target, send.partner, send.text),
        paste: (text) => terminal.pasteAndWait(text),
        submit: () => {
          terminal.submit();
        },
        wait: delay,
      },
      send.submit,
      BROADCAST_ENTER_DELAY_MS,
    );
    switch (result.kind) {
      case "pasted":
        if (!send.submit) {
          dispatch({ type: "activated", id: target });
          focusTerminal();
        }
        break;
      case "notReady":
        keep(t("partner.notReady", { session: name }));
        break;
      case "refused":
        showError(result.error);
        unsent(name);
        break;
      // A message sent by hand has no "Cancel"; listed so every way the delivery ends is named.
      case "cancelled":
      case "gone":
      case "pasteFailed":
        // A failed paste is reported by the terminal itself; no Enter follows.
        unsent(name);
        break;
    }
  };

  /**
   * Delivers the message of a card below row `to` after the person's click (ADR 0012, decision
   * point 4; ADR 0014), the way a message sent by hand goes: held while the target works, pasted
   * once it can take a paste, Enter for "Send". Until the paste begins the card says that it
   * waits, with "Cancel" (messageCard.ts). The backend checks the target again and heads the
   * text; the message goes only once it was pasted, and the list is read again whatever
   * happened, so the card never outlives its message. A message that left the list meanwhile
   * (discarded, its group unlinked) ends the wait without a word.
   */
  const sendCardMessage = async (card: {
    runs: CardSends;
    to: number;
    id: number;
    submit: boolean;
    /** The backend still lists the message. */
    listed: () => boolean;
    prepare: () => Promise<string>;
    close: () => Promise<void>;
    release?: () => Promise<void>;
    reload: () => void;
  }) => {
    const terminal = terminals.get(card.to);
    const name = titleOf(card.to);
    if (!terminal) {
      setStartHint(t("partner.unsent", { session: name }));
      return;
    }
    if (!card.runs.start(card.id, card.submit, name, card.to)) return;
    const gone = () => terminals.get(card.to) !== terminal || !card.listed();
    const cancelled = () => card.runs.cancelled(card.id);
    try {
      const result = await sendFromCard({
        deliver: () =>
          deliverMessage(
            {
              working: () => terminal.attention.state === "working",
              gone,
              cancelled,
              // The card itself says that the message waits.
              onHeld: () => undefined,
              onSending: () => {
                card.runs.sending(card.id);
              },
              ready: () =>
                waitUntilReady(
                  {
                    bracketedPaste: () => terminal.bracketedPaste(),
                    quietFor: () => {
                      const last = terminal.activity().lastOutput;
                      return last === null ? null : Date.now() - last;
                    },
                    gone: () => gone() || cancelled(),
                  },
                  delay,
                ),
              bracketedPaste: () => terminal.bracketedPaste(),
              prepare: card.prepare,
              paste: (text) => terminal.pasteAndWait(text),
              submit: () => {
                terminal.submit();
              },
              wait: delay,
            },
            card.submit,
            BROADCAST_ENTER_DELAY_MS,
          ),
        close: card.close,
        ...(card.release === undefined ? {} : { release: card.release }),
        reload: card.reload,
        showError,
      });
      const notice = cardNotice(result, !card.listed());
      if (notice.kind === "hint") setStartHint(t(notice.key, { session: name }));
      else if (notice.kind === "error") showError(notice.error);
      else if (result.kind === "pasted" && !card.submit) {
        // Only pasted: the person goes on typing there.
        dispatch({ type: "activated", id: card.to });
        focusTerminal();
      }
    } finally {
      card.runs.finish(card.id);
    }
  };

  /** Sends waiting partner message `id` into session `to`; Enter only when `submit`. */
  const pasteWaiting = (to: number, id: number, submit: boolean) =>
    sendCardMessage({
      runs: inboxRuns,
      to,
      id,
      submit,
      listed: () => waitingRef.current.some((message) => message.id === id),
      prepare: () => api.waitingMessageText(id, to),
      close: () => api.closeWaitingMessage(id, to).then(() => undefined),
      reload: () => {
        loadWaitingRef.current();
      },
    });

  /** A card's discard: hidden at once, removed in the backend, the list read again. */
  const discardCard = (
    id: number,
    setHidden: (update: (hidden: ReadonlySet<number>) => ReadonlySet<number>) => void,
    close: () => Promise<unknown>,
    reload: () => void,
  ) => {
    void discardFromCard({
      hide: () => {
        setHidden((hidden) => new Set(hidden).add(id));
      },
      unhide: () => {
        setHidden((hidden) => new Set([...hidden].filter((entry) => entry !== id)));
      },
      close: () => close().then(() => undefined),
      reload,
      showError,
    });
  };

  /** Discards waiting message `id`; nothing of it stays. */
  const discardWaiting = (id: number) => {
    discardCard(
      id,
      setHiddenWaiting,
      () => api.closeWaitingMessage(id, null),
      () => {
        loadWaitingRef.current();
      },
    );
  };

  /**
   * Delivers a link message on its own, with Enter, once its target went quiet after working and
   * nobody types there (links.ts `autoReadiness`, `deliverAutomatically`). Whatever stops it turns
   * it into a message that waits for a click, so nothing is lost or retried without end. The
   * introduction waits only a few seconds: a session that never worked may show a trust dialog,
   * which an Enter would answer.
   */
  const deliverLinkMessage = async (message: LinkMessage) => {
    const terminal = terminals.get(message.to);
    if (!terminal) return;
    linkDelivering.current.add(message.to);
    linkDeliveringIds.current.add(message.id);
    setDeliveringIds(new Set(linkDeliveringIds.current));
    const gone = () => terminals.get(message.to) !== terminal;
    try {
      const intro = message.fromName === null;
      const result = await deliverAutomatically(
        {
          facts: () => ({
            state: terminal.attention.state,
            reason: terminal.attention.reason,
            endedBy: terminal.attention.endedBy,
            // On its own, the Enter would answer a question or permission dialog the target
            // shows (agentCommands.ts `asksQuestion`), so the message holds until it is answered.
            asks: sessionAsksRef.current(message.to),
            gatePending: gateWaitingRef.current.has(message.to),
            permissionPending: permissionWaitingRef.current.has(message.to),
            ...terminal.personInput(),
          }),
          gone,
          bracketedPaste: () => terminal.bracketedPaste(),
          quietFor: () => {
            const last = terminal.activity().lastOutput;
            return last === null ? null : Date.now() - last;
          },
          take: () => api.linkMessageText(message.id, message.to, true),
          paste: (text) => terminal.pasteAndWait(text),
          submit: () => {
            terminal.submit();
          },
          close: () => api.closeLinkMessage(message.id, message.to).then(() => undefined),
          wait: delay,
        },
        {
          waitWhileWorking: !intro,
          patienceMs: intro ? INTRO_PATIENCE_MS : MESSAGE_PATIENCE_MS,
          typingPatienceMs: TYPING_PATIENCE_MS,
        },
        BROADCAST_ENTER_DELAY_MS,
      );
      if (result !== "pasted") {
        await api.holdLinkMessage(message.id).catch(() => undefined);
        if (result === "timeout" || result === "typing" || result === "failed") {
          setStartHint(t("links.notReady", { session: titleOf(message.to) }));
        }
      }
    } finally {
      linkDelivering.current.delete(message.to);
      linkDeliveringIds.current.delete(message.id);
      setDeliveringIds(new Set(linkDeliveringIds.current));
      // Also brings back the card of a message whose claim ran out meanwhile.
      loadLinksRef.current();
    }
  };

  // Link messages that go on their own (ADR 0014): one at a time per target, each once the
  // target waits for input. The effect runs again whenever the backend's list changes; the ref
  // takes this render's function first, so it never delivers with a stale one.
  const deliverLinkRef = useRef<(message: LinkMessage) => void>(() => undefined);
  useEffect(() => {
    deliverLinkRef.current = (message) => {
      void deliverLinkMessage(message);
    };
  });
  useEffect(() => {
    for (const message of nextDeliveries(linkState.messages, linkDelivering.current)) {
      deliverLinkRef.current(message);
    }
  }, [linkState.messages]);

  /**
   * Sends held link message `id` into session `to` after the person's click. The backend claims
   * it for the paste; one that did not go in is freed for the next click. One whose automatic
   * delivery still runs here is refused: it is being pasted already.
   */
  const pasteHeldLink = async (to: number, id: number, submit: boolean) => {
    if (linkDeliveringIds.current.has(id)) return;
    await sendCardMessage({
      runs: linkRuns,
      to,
      id,
      submit,
      listed: () =>
        linkStateRef.current.messages.some((message) => message.id === id && message.to === to),
      prepare: () => api.linkMessageText(id, to, false),
      close: () => api.closeLinkMessage(id, to).then(() => undefined),
      release: () => api.holdLinkMessage(id).then(() => undefined),
      reload: () => {
        loadLinksRef.current();
      },
    });
  };

  /** Discards held link message `id`; nothing of it stays but its line in the group's history. */
  const discardLink = (id: number) => {
    discardCard(
      id,
      setHiddenLinks,
      () => api.closeLinkMessage(id, null),
      () => {
        loadLinksRef.current();
      },
    );
  };

  const closeLinkDialog = () => {
    setLinkDialog(null);
    focusActive();
  };
  /** The group the "Edit group" dialog shows; gone once the group ended. */
  const editedGroup =
    linkDialog?.kind === "edit"
      ? linkState.groups.find((entry) => entry.id === linkDialog.group)
      : undefined;
  const saveLinkGroup = async (
    names: { session: number; name: string }[],
    askFirst: boolean,
    lead: number | null,
    added: AddedMember[],
  ): Promise<string | null> => {
    if (editedGroup === undefined) return null;
    try {
      if (added.length === 0) {
        await api.editLinkGroup(editedGroup.id, names, askFirst, lead);
      } else {
        // Renames first, so a newcomer may take a name a member gives up; the lead goes with
        // the newcomers, so the members get one introduction naming both.
        await api.editLinkGroup(editedGroup.id, names, askFirst, leadOf(editedGroup));
        await api.addToLinkGroup(
          editedGroup.id,
          added.map((entry) => entry.session),
          added,
          lead,
        );
      }
      loadLinksRef.current();
      closeLinkDialog();
      return null;
    } catch (e) {
      // A rename that went through shows at once; the dialog says what did not.
      loadLinksRef.current();
      return errorText(e);
    }
  };
  /** Where session `id` works and what runs there, which its member name comes from. */
  const aliasSource = (id: number): AliasSource => {
    const session = stateRef.current.sessions.find((s) => s.id === id);
    if (session === undefined) return { path: "", program: "" };
    return {
      path: session.worktree?.repoRoot ?? session.cwd,
      program:
        profilesRef.current.find((p) => p.id === session.profileId)?.name ?? session.profileId,
    };
  };
  /** Default names for sessions `ids` joining a group whose members carry `members`' names. */
  const joinAliases = (ids: readonly number[], members: readonly AddedMember[]): string[] =>
    memberAliases(
      ids.map(aliasSource),
      members.map((member) => ({ ...aliasSource(member.session), name: member.name })),
    );
  /** Adds sessions `ids` to `group` with their default names; answers why not, or `null`. */
  const joinGroup = async (group: LinkGroup, ids: readonly number[]): Promise<string | null> => {
    const aliases = joinAliases(ids, group.members);
    const names = ids.map((session, index) => ({ session, name: aliases[index] ?? "" }));
    try {
      await api.addToLinkGroup(group.id, ids, names, leadOf(group));
      loadLinksRef.current();
      return null;
    } catch (e) {
      return errorText(e);
    }
  };
  const joinFromDialog = async (groupId: number): Promise<string | null> => {
    const group = linkState.groups.find((entry) => entry.id === groupId);
    if (linkDialog?.kind !== "link" || group === undefined) return null;
    const problem = await joinGroup(group, [linkDialog.id]);
    if (problem === null) closeLinkDialog();
    return problem;
  };
  const linkFromDialog = async (ids: number[], lead: number | null): Promise<string | null> => {
    const problem = await linkSessions(ids, lead);
    if (problem === null) closeLinkDialog();
    return problem;
  };
  /** The group the "Group history" dialog shows; gone once the group ended. */
  const historyGroup =
    linkDialog?.kind === "history"
      ? linkState.groups.find((entry) => entry.id === linkDialog.group)
      : undefined;
  const historyGroupId = historyGroup?.id;
  const loadLinkHistory = useCallback(async (): Promise<LinkHistoryEntry[] | string> => {
    if (historyGroupId === undefined) return [];
    try {
      return parseLinkHistory(await api.linkHistory(historyGroupId));
    } catch (e) {
      return errorText(e);
    }
  }, [historyGroupId]);

  /** Links sessions `ids`, with `lead` leading; answers why not, or `null` once linked. */
  const linkSessions = async (
    ids: readonly number[],
    lead: number | null = null,
  ): Promise<string | null> => {
    // Names from the project folder or the program: a title is often the first prompt, which
    // reads badly as a name the agents call each other by (linkAliases.ts).
    const aliases = memberAliases(ids.map(aliasSource));
    const names = ids.map((session, index) => ({ session, name: aliases[index] ?? "" }));
    try {
      await api.linkSessions(ids, { lead, names });
      loadLinksRef.current();
      return null;
    } catch (e) {
      return errorText(e);
    }
  };

  /** The project cards' partner list (ProjectPartners.tsx). */
  const partnerActions: PartnerActions = {
    onLink: async (link) => {
      try {
        setPartnerBook(parsePartnerBook(await api.linkPartner(link)));
        return null;
      } catch (e) {
        return errorText(e);
      }
    },
    onUnlink: (a, b) => {
      api
        .unlinkPartner(a, b)
        .then((value) => {
          setPartnerBook(parsePartnerBook(value));
          // Messages between the two go with the pair.
          loadWaitingRef.current();
        })
        .catch(showError);
    },
    onPickFolder: async () => {
      try {
        const picked = await open({ directory: true });
        return typeof picked === "string" ? picked : null;
      } catch (e) {
        showError(e);
        return null;
      }
    },
  };

  const editorSession =
    queueEditor === null ? undefined : state.sessions.find((s) => s.id === queueEditor.id);
  const editorKey = editorSession === undefined ? undefined : terminals.get(editorSession.id)?.key;
  // A session that ended (or went) meanwhile offers no commands: its menu and question go too.
  const commandMenuSession =
    commandMenu === null ? undefined : state.sessions.find((s) => s.id === commandMenu.id);
  const commandItems: PopupItem[] =
    commandMenuSession === undefined
      ? []
      : commandMenuEntries(
          sessionCommands(commandMenuSession, profiles),
          commandMenuSession.title,
          t,
        ).map((entry): PopupItem => {
          if (entry.type === "heading") return entry;
          const { command, ...item } = entry;
          const id = commandMenuSession.id;
          return {
            ...item,
            run: () => {
              runAgentCommand(id, command);
            },
          };
        });
  const commandAskSession =
    commandAsk === null ? undefined : state.sessions.find((s) => s.id === commandAsk.id);

  /** The active session's agent commands for quick access, `null` when it offers none. */
  const launcherCommands = () => {
    if (activeSession === undefined) return null;
    const commands = sessionCommands(activeSession, profiles);
    if (commands.length === 0 || sessionAsks(activeSession.id)) return null;
    const id = activeSession.id;
    return {
      commands,
      sessionTitle: activeSession.title,
      programName: profileById.get(activeSession.profileId)?.name ?? "",
      run: (command: AgentCommand) => {
        runAgentCommand(id, command);
      },
    };
  };

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
      sessionIcon: (session) => (
        <SessionLook look={badgeFor(session.profileId)} session={session} crop="full" />
      ),
      notesIcon: <NotesIcon size={14} />,
      activate: (id) => {
        dispatch({ type: "activated", id });
        focusTerminal();
      },
      start: (profile, folder) => void start(profile, folder),
      startAsTeam: teamOffered ? (profile, folder) => void start(profile, folder, "team") : null,
      subAgentStarts: {
        defsFor: (folder) => defsFor(agentLibrary.library, folder),
        icon: subAgentIcon,
        origin: (def) => agentOrigin(def, agentLibrary.library, t),
        start: (profile, folder, agent) =>
          void start(profile, folder, "private", undefined, undefined, undefined, agent),
      },
      templates,
      templateIcon: <TemplateIcon size={14} />,
      startTemplate: (id) => void startTemplate(id),
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
      crew:
        activeSession?.subAgents === true
          ? {
              shown: settings.crewColumn,
              toggle: () => {
                updateSettings({ crewColumn: !settings.crewColumn });
              },
            }
          : null,
      queue:
        activeSession === undefined
          ? null
          : {
              offer: queueOfferFor(activeSession),
              open: () => {
                setQueueEditor({ id: activeSession.id, anchor: null });
              },
            },
      agentCommands: launcherCommands(),
      openSettings,
      menu,
      skipLabels: [t("menu.launcher")],
    });

  /**
   * Entries of quick access that need the backend: the files of the active session's folder (the
   * same bounded index as the file tree's filter) and its recent commits.
   */
  const launcherLoaders = () => {
    const projects = () =>
      loadProjectItems(state.sessions, api.gitRepoPlaces, showProject, <FolderIcon size={14} />);
    const session = activeSession;
    if (session === undefined) return [projects];
    const fileIcon = <FileIcon size={14} />;
    const commitIcon = <SourceControlIcon size={14} />;
    return [
      projects,
      async () => {
        const index = parsePathIndex(await api.listSessionPaths(session.id));
        return index === null
          ? []
          : fileItems(
              index.entries,
              (path) => {
                openFile(session.id, path);
              },
              fileIcon,
            );
      },
      async () => {
        const log = parseGitLog(await api.gitLog(session.id, 0, COMMIT_PAGE));
        return log === null || !log.isRepo
          ? []
          : commitItems(log.commits, openCommit, t, commitIcon);
      },
    ];
  };

  /** Tiles of the overview: the state as edge color and in words, like the sidebar rows. */
  const overviewTiles = (): OverviewTile[] =>
    state.sessions.map((session) => {
      const tileState = attention.get(session.id) ?? "idle";
      const chip = sessionChip(
        session,
        tileState,
        reasons.get(session.id) ?? null,
        runsAgent(profileById.get(session.profileId)?.command, session.kind),
      );
      return {
        id: session.id,
        title: [session.marker?.emoji, session.title].filter(Boolean).join(" "),
        badge: (
          <SessionLook
            look={badgeFor(session.profileId)}
            session={session}
            programName={profileById.get(session.profileId)?.name ?? session.profileId}
          />
        ),
        edge: edgeClass(rowEdge(session, tileState, chip)),
        status: statusText(session, t, tileState, reasons.get(session.id) ?? null),
      };
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
      <div
        ref={appGrid}
        className="app"
        style={{ gridTemplateColumns: `${String(sidebarColumn)}px 1fr` }}
      >
        <Sidebar
          sessions={state.sessions}
          activeId={state.activeId}
          profiles={profiles}
          startable={startable}
          allHidden={allHidden}
          badgeFor={badgeFor}
          versions={programs.versions}
          updates={programUpdates.updates}
          filesOpen={filesShown}
          onToggleFiles={toggleFiles}
          devServers={devServers}
          onOpenDevServer={openDevServer}
          closedGroups={layout.closedGroups}
          onToggleGroup={(group: SidebarGroup) => {
            setLayout((current) => toggleGroup(current, group));
          }}
          closedAgentGroups={layout.closedAgentGroups}
          onToggleAgentGroup={(key: string) => {
            setLayout((current) => toggleAgentGroup(current, key));
          }}
          onFocusTerminal={focusTerminal}
          version={version}
          theme={theme}
          onToggleTheme={toggleTheme}
          target={target}
          runningProjects={running}
          worktree={worktree}
          startMode={startMode}
          onStartMode={setStartMode}
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
          placements={placements}
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
          homeShown={homeShown && state.sessions.length > 0}
          onHome={toggleHome}
          onMenuClose={focusActive}
          onActivate={(id) => {
            setPutAside(false);
            dispatch({ type: "activated", id });
          }}
          onStart={startHere}
          recentSessions={recentShown}
          onReopenRecent={reopenRecent}
          onRemoveRecent={(entry) => {
            setRecentSessions((current) => withoutRecent(current, entry));
          }}
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
          onForgetFolder={(path) => {
            setFolders((current) => forgetFolder(current, path));
          }}
          onMoveFavorite={(path, target, after) => {
            setFolders((current) => moveFavorite(current, path, target, after));
          }}
          templates={templates}
          onStartTemplate={(id) => void startTemplate(id)}
          onRenameTemplate={(id) => {
            setTemplateDialog({ kind: "rename", id });
          }}
          onDeleteTemplate={(id) => {
            setTemplates((current) => deleteTemplate(current, id));
          }}
          onSaveTemplate={openSaveTemplate}
          onOpenArena={arenaOpenable ? openArena : null}
          grouping={grouping}
          onGrouping={(sessionGrouping) => {
            updateSettings({ sessionGrouping });
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
          onAgentCommand={compactFromRow}
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
                          (id) => {
                            const session = state.sessions.find((s) => s.id === id);
                            return session ? sessionGroupKey(session, grouping) : "";
                          },
                        )
                      : null,
                }
          }
          onDragStart={beginSessionDrag}
          onMove={moveSession}
          subAgentTree={
            subAgentKey !== null &&
            watchedId !== null &&
            watchedId === state.activeId &&
            subAgentWatch.count > 0
              ? { sessionId: watchedId, watchKey: subAgentKey, hidden: subAgentHidden }
              : null
          }
          subAgentsOpen={subAgentsOf}
          subAgentPick={subAgentPick}
          onShowSubAgent={showSubAgent}
          onDismissSubAgents={dismissSubAgents}
          queued={queuedById}
          inbox={inboxRows}
          inboxSends={inboxSends}
          cardsKey={cardsKey}
          onInboxPaste={(sessionId, messageId, submit) => {
            void pasteWaiting(sessionId, messageId, submit);
          }}
          onInboxCancel={(messageId) => inboxRuns.cancel(messageId)}
          onInboxDiscard={discardWaiting}
          links={linkMarks(linkState)}
          onEditLinkGroup={(group) => {
            setLinkDialog({ kind: "edit", group });
          }}
          linkable={selectableForLink(linkState, state.sessions)}
          canLinkSelection={(ids) =>
            selectionPlan(linkState, state.sessions, new Set(ids)) !== null
          }
          onLinkSessions={(ids) => {
            // With members of one group among the selection, the others join that group.
            const plan = selectionPlan(linkState, state.sessions, new Set(ids));
            if (plan === null) return;
            const done =
              plan.kind === "new" ? linkSessions(plan.ids) : joinGroup(plan.group, plan.ids);
            void done.then((problem) => {
              if (problem !== null) setError(problem);
            });
          }}
          linkInbox={linkInboxRows}
          linkSends={linkSends}
          onLinkPaste={(sessionId, messageId, submit) => {
            void pasteHeldLink(sessionId, messageId, submit);
          }}
          onLinkCancel={(messageId) => linkRuns.cancel(messageId)}
          onLinkDiscard={discardLink}
          stalls={stalls}
          longWaits={longWaits}
          waitThresholdMs={waitThresholdMs(settings.waitMinutes)}
          peekRequest={peekRequest}
          rowLayout={settings.rowLayout}
          onToggleOneLine={toggleRowsOneLine}
          peekLines={(id) => {
            // A session on screen needs no peek; its own terminal shows more.
            if (shownRef.current.has(id)) return null;
            const terminal = terminals.get(id);
            return terminal ? terminal.peekContent(PEEK_LINES) : null;
          }}
          onInstallUpdate={() => {
            if (updater.state.kind !== "available") return;
            const update = updater.state.update;
            // The installer quits Hangar: save first so the sessions come back afterwards.
            void persistAll().then(() => updater.install(update));
          }}
          onDismissUpdate={updater.dismiss}
          restoresSessions={settings.restoreSessions}
          teamOpen={teamOpen}
          onToggleTeam={() => {
            if (teamOpen) closeTeam();
            else setTeamOpen(true);
          }}
          onOpenAccount={() => {
            openSettings("account");
          }}
          onOpenSettings={() => {
            openSettings();
          }}
        />
        <main className="body">
          <MainBar
            title={barTitle(homeShown ? undefined : activeSession)}
            sideOpen={sideShown}
            support={showsSupportLink(accountStore.view)}
            onToggleSide={() => {
              // The tab's own switch: closes it when open, opens it with the focus, or brings the
              // detached panel window up on it.
              if (layout.sideTab === "notes") toggleNotes();
              else if (layout.sideTab === "changes") toggleChanges();
              else toggleFiles();
            }}
          />
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
            className={
              sideShown && notesPlacement.mode === "overlay"
                ? "workspace notes-overlaid"
                : "workspace"
            }
          >
            <div
              className={[
                "panes",
                state.sessions.length > 0 ? "filled" : "",
                homeShown && state.sessions.length > 0 ? "put-aside" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              data-panes=""
            >
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
                      ? {
                          layout: subAgentPlan.layout,
                          slot: 0,
                          ratios: subAgentPlan.ratios,
                          session,
                          ...paneLook(session)(),
                        }
                      : null;
                const dictation = dictationMode(session.id);
                return (
                  <TerminalPane
                    key={session.id}
                    terminal={terminal}
                    sessionId={session.id}
                    active={active}
                    visible={active || paneInfo !== null}
                    split={paneInfo}
                    crew={
                      crewWatching && active && subAgentKey !== null && session.id === watchedId
                        ? {
                            sessionId: session.id,
                            sessionTitle: session.title,
                            cwd: session.cwd,
                            watchKey: subAgentKey,
                            hidden: subAgentHidden,
                            sessionWaits: waitsForDecision(
                              attention.get(session.id),
                              reasons.get(session.id),
                            ),
                            waiting: waitingSubAgents(pendingGate, session.id),
                            picked: subAgentsOf === session.id ? subAgentPick : null,
                            onShow: showSubAgent,
                            onHide: () => {
                              updateSettings({ crewColumn: false });
                            },
                          }
                        : null
                    }
                    dropping={dropTarget === session.id}
                    onFocusPane={(id) => {
                      dispatch({ type: "activated", id });
                    }}
                    onMaximize={splitView.maximize}
                    onHeaderMenu={(id, x, y) => {
                      setSessionMenu({ id, x, y });
                    }}
                  >
                    {shownDecision?.sessionId === session.id &&
                      shown.has(session.id) &&
                      gateCardFor(shownDecision, false)}
                    {shownDecision?.sessionId !== session.id && (
                      <GateRuleNote entry={ruleNotes.get(session.id)} />
                    )}
                    {dictation !== null && <DictationIndicator mode={dictation} />}
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
                <SubAgentPane
                  title={subAgentSession.title}
                  watchKey={subAgentKey}
                  showNotice={subAgentShowsNotice}
                  notice={subAgentNotice}
                  excerpts={settings.subAgentExcerpts}
                  picked={subAgentPick}
                  hidden={subAgentHidden}
                  onPick={setSubAgentPick}
                  onClose={closeSubAgents}
                  ratios={subAgentPlan.ratios}
                />
              )}
              {subAgentPlan !== null && subAgentSession !== undefined && (
                <PaneSplitters
                  layout={subAgentPlan.layout}
                  ratios={subAgentPlan.ratios}
                  onChange={setSubAgentSplit}
                  onReset={resetSubAgentSplit}
                  label={t("subAgents.splitter")}
                />
              )}
              {shownDecision !== null &&
                !shown.has(shownDecision.sessionId) &&
                gateCardFor(shownDecision, true)}
              {state.sessions.length > 0 &&
                subAgentPlan === null &&
                splitView.split.slots.map((id, slot) =>
                  id === null && splitView.layout !== "single" ? (
                    <EmptyPane
                      key={`empty-${String(slot)}`}
                      layout={splitView.layout}
                      slot={slot}
                      ratios={splitView.ratios}
                    />
                  ) : null,
                )}
              {state.sessions.length > 0 &&
                subAgentPlan === null &&
                splitView.layout !== "single" && (
                  <PaneSplitters
                    layout={splitView.layout}
                    ratios={splitView.ratios}
                    onChange={splitView.setRatios}
                    onReset={splitView.resetRatios}
                  />
                )}
              {sessionDragView?.overBody === true && (
                <>
                  {sessionDragView.target !== null && sessionDragView.target.kind !== "list" && (
                    <div
                      className="snap-preview"
                      style={
                        sessionDragView.target.kind === "snap"
                          ? paneStyle(
                              sessionDragView.target.layout,
                              sessionDragView.target.zone,
                              ratiosFor(layout.paneRatios, sessionDragView.target.layout),
                            )
                          : paneStyle(
                              splitView.layout,
                              sessionDragView.target.zone,
                              splitView.ratios,
                            )
                      }
                      aria-hidden="true"
                    >
                      <span className="snap-preview-label">{t("snap.showHere")}</span>
                    </div>
                  )}
                  <SnapBar
                    mode="drag"
                    current={splitView.layout}
                    ratios={layout.paneRatios}
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
                      style={paneStyle(
                        snapPicker.preview.layout,
                        snapPicker.preview.zone,
                        ratiosFor(layout.paneRatios, snapPicker.preview.layout),
                      )}
                      aria-hidden="true"
                    />
                  )}
                  <SnapBar
                    mode="pick"
                    current={splitView.layout}
                    ratios={layout.paneRatios}
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
              {homeShown && <GlyphBackground />}
              {homeShown && (
                <div className={state.sessions.length > 0 ? "empty home" : "empty"}>
                  {state.sessions.length > 0 ? (
                    <p className="home-back">
                      <span>{t("home.running", { count: String(state.sessions.length) })}</span>
                      <button
                        type="button"
                        className="link-button"
                        onClick={() => {
                          setPutAside(false);
                          focusTerminal();
                        }}
                      >
                        {t("home.showSessions")}
                      </button>
                    </p>
                  ) : (
                    allHidden && <p>{t("sidebar.allHidden")}</p>
                  )}
                  <ActivityCalendar
                    folders={activityFolders(
                      folders.favorites,
                      folders.recent,
                      state.sessions.map((session) => session.cwd),
                    )}
                    language={settings.language}
                  />
                  <ProjectOverview
                    sessions={state.sessions}
                    rows={new Map(overviewTiles().map((tile) => [tile.id, tile]))}
                    activeId={state.activeId}
                    devServers={devServers}
                    finished={finishedRuns}
                    language={settings.language}
                    onActivate={(id) => {
                      setPutAside(false);
                      dispatch({ type: "activated", id });
                      focusTerminal();
                    }}
                    onOpenDevServer={openDevServer}
                    onShowChanges={(id) => {
                      setPutAside(false);
                      showDoneChanges(id);
                    }}
                    partners={partnerBook}
                    partnerActions={partnerActions}
                    focus={projectFocus}
                    onNewSession={
                      nextProgram
                        ? () => {
                            perform("newSession");
                          }
                        : null
                    }
                  />
                </div>
              )}
            </div>
            {sideShown && notesPlacement.mode === "docked" && (
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
            {sideMotion.phase !== "closed" && (
              <SidePanel
                tab={layout.sideTab}
                onTab={(tab) => {
                  openSide(tab);
                  if (tab === "changes") setChangesFocusToken((token) => token + 1);
                  if (tab === "files") setFilesFocusToken((token) => token + 1);
                }}
                mode={notesPlacement.mode}
                width={notesPlacement.width}
                phase={sideMotion.phase}
                frame={sideMotion.panel}
                onDetach={detachPanel}
                onClose={closeSide}
                onEscape={focusTerminal}
                notes={
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
                }
                changes={
                  <ChangesPanel
                    claims={activeSession ? claimsOf(claims, activeSession.id) : null}
                    overlaps={
                      activeSession
                        ? (overlapsById.get(activeSession.id) ?? NO_OVERLAPS)
                        : NO_OVERLAPS
                    }
                    teammates={
                      activeSession
                        ? (teammatesById.get(activeSession.id) ?? NO_TEAMMATES)
                        : NO_TEAMMATES
                    }
                    nameOf={sessionName}
                    onShowSession={showSession}
                    cwd={activeSession?.cwd ?? null}
                    sessionId={activeSession?.id ?? null}
                    agent={
                      activeSession !== undefined &&
                      activeSession.status !== "exited" &&
                      panelSessionIsAgent(panelSessions, activeSession.id)
                    }
                    onSendToAgent={(text) => {
                      if (activeSession) sendFromNotes(text, activeSession.id);
                    }}
                    trigger={activeSession ? (attention.get(activeSession.id) ?? "idle") : "none"}
                    focusToken={changesFocusToken}
                    view={changesView}
                    onView={setChangesView}
                    timeline={timelineAvailability(
                      activeSession !== undefined &&
                        takesSnapshots(
                          activeSession.id,
                          panelSessionIsAgent(panelSessions, activeSession.id),
                        ),
                      settings.attentionEnabled,
                    )}
                    focusCommit={commitFocus}
                    language={settings.language}
                  />
                }
                files={
                  <FilesPanel
                    session={
                      activeSession ? { id: activeSession.id, cwd: activeSession.cwd } : null
                    }
                    claims={claims}
                    activities={teamStore.activities}
                    sessions={localSessions}
                    nameOf={sessionName}
                    focusToken={filesFocusToken}
                    onInsertPath={(item) => {
                      insertPath(item, null);
                      focusTerminal();
                    }}
                    onDragPath={dragPath}
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
                {pasteCheck.warning.secrets !== undefined &&
                  pasteCheck.warning.secrets.length > 0 && (
                    <span>
                      {t("paste.secret", {
                        kinds: secretLabels(pasteCheck.warning.secrets, t),
                      })}
                    </span>
                  )}
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
            {unpastedHandover !== null && (
              <div className="link-notice" role="status">
                <span>{t("handover.notReady", { program: unpastedHandover.program })}</span>
                <div className="link-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      const { text } = unpastedHandover;
                      setUnpastedHandover(null);
                      writeClipboard(text)
                        .then(() => {
                          setStartHint(t("handover.copied"));
                        })
                        .catch(showError);
                    }}
                  >
                    {t("menu.copy")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setUnpastedHandover(null);
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
            {unsentPartner !== null && (
              <div className="link-notice" role="status">
                <span>{unsentPartner.message}</span>
                <div className="link-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      const { text } = unsentPartner;
                      setUnsentPartner(null);
                      writeClipboard(text)
                        .then(() => {
                          setStartHint(t("partner.copied"));
                        })
                        .catch(showError);
                    }}
                  >
                    {t("menu.copy")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setUnsentPartner(null);
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
            )}
            {crashNotice !== null && (
              <CrashNotice
                summary={crashNotice}
                language={settings.language}
                onClose={closeCrashNotice}
              />
            )}
            <BudgetNotice
              language={settings.language}
              budget={prefsReady ? settings.dailyBudget : null}
              earlyPercent={settings.budgetEarlyPercent}
              warned={budgetWarned}
              onWarned={setBudgetWarned}
              quietHours={settings.quietHours}
              onShowCosts={() => {
                setDialog("costs");
              }}
            />
            <MorningNotice
              language={settings.language}
              enabled={prefsReady && settings.morningReview}
              offeredDay={morningOfferedDay}
              onOffered={setMorningOfferedDay}
              load={api.morningReview}
              onOpenReview={(day) => {
                setReviewDay(day);
                setDialog("dailyReview");
              }}
            />
            <UsageStatsNotice
              consent={prefsReady ? usageConsent : undefined}
              onAnswer={answerUsage}
              onOpenInfo={openUsageInfo}
            />
            {startHint !== null && (
              <div className="link-notice start-hint" role="status" key={startHint}>
                <span>{startHint}</span>
              </div>
            )}
            {doneNotice !== null && (
              <div
                className="link-notice start-hint done-notice"
                role="status"
                key={`${String(doneNotice.sessionId)}:${String(doneNotice.count)}`}
                onMouseEnter={() => {
                  setHeldDoneNotice(doneNotice);
                }}
                onFocus={() => {
                  setHeldDoneNotice(doneNotice);
                }}
                onMouseLeave={(event) => {
                  if (!event.currentTarget.contains(document.activeElement)) {
                    setHeldDoneNotice(null);
                  }
                }}
                onBlur={(event) => {
                  const inside =
                    event.relatedTarget instanceof Node &&
                    event.currentTarget.contains(event.relatedTarget);
                  if (!inside && !event.currentTarget.matches(":hover")) setHeldDoneNotice(null);
                }}
              >
                <span>
                  {t("changes.done", { title: doneNotice.title, count: doneNotice.count })}
                </span>
                <div className="link-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      showDoneChanges(doneNotice.sessionId);
                    }}
                  >
                    {t("changes.doneOpen")}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setDoneNotice(null);
                      focusTerminal();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
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
            {teamNotice !== null && (
              <TeammateNoticeView
                key={teamNotice.key}
                overlap={teamNotice.overlap}
                sessionName={sessionName(teamNotice.overlap.sessionId)}
                onShow={() => {
                  setTeamNotice(null);
                  showSession(teamNotice.overlap.sessionId);
                }}
                onDismiss={() => {
                  setTeamNotice(null);
                  focusActive();
                }}
              />
            )}
            {creatingWorktree && (
              <div className="link-notice" role="status">
                <span>{t("worktree.creating")}</span>
              </div>
            )}
            {taskNotice !== null && (
              <div key={taskNotice.key} className="link-notice" role="status">
                <span>{taskNotice.text}</span>
                <div className="link-actions">
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("error.dismiss")}
                    title={t("error.dismiss")}
                    onClick={() => {
                      setTaskNotice(null);
                      focusActive();
                    }}
                  >
                    <CloseIcon />
                  </button>
                </div>
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
              <div className={link.ask ? "link-notice link-check" : "link-notice"} role="status">
                <span>{t(link.ask ? "link.checkTarget" : "link.notice")}</span>
                <LinkAddress url={link.url} />
                <div className="link-actions">
                  {link.ask && (
                    <button type="button" className="primary-button" onClick={openCheckedLink}>
                      {t("link.open")}
                    </button>
                  )}
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
        {queueEditor !== null && editorSession !== undefined && editorKey !== undefined && (
          <QueueDialog
            key={queueEditor.id}
            title={editorSession.title}
            initial={queue.get(editorKey)?.text ?? ""}
            anchor={queueEditor.anchor}
            onSave={(text) => {
              saveQueued(editorSession.id, text);
            }}
            onDiscard={
              queue.has(editorKey)
                ? () => {
                    discardQueued(editorSession.id);
                    closeQueueEditor();
                  }
                : null
            }
            onClose={closeQueueEditor}
          />
        )}
        {commandMenu !== null && commandMenuSession !== undefined && (
          <PopupMenu
            items={commandItems}
            label={t("agentCommand.menuFor", { session: commandMenuSession.title })}
            position={commandMenu}
            onClose={() => {
              setCommandMenu(null);
              focusActive();
            }}
          />
        )}
        {commandAsk !== null && commandAskSession !== undefined && (
          <AgentCommandConfirm
            command={commandAsk.command}
            sessionTitle={commandAskSession.title}
            onCancel={() => {
              setCommandAsk(null);
              focusActive();
            }}
            onConfirm={() => {
              setCommandAsk(null);
              sendAgentCommand(commandAsk.id, commandAsk.command);
            }}
          />
        )}
        {snippetDraft !== null && (
          <SaveSnippetDialog
            text={snippetDraft}
            snippets={settings.snippets}
            onSave={(text) => {
              updateSettings({
                snippets: [...settings.snippets, { id: newSnippetId(settings.snippets), text }],
              });
              setSnippetDraft(null);
              focusActive();
            }}
            onShowSnippets={() => {
              setSnippetDraft(null);
              openSettings("snippets");
            }}
            onClose={() => {
              setSnippetDraft(null);
              focusActive();
            }}
          />
        )}
        {handoverSession !== undefined && (
          <HandoverDialog
            title={handoverSession.title}
            targets={handoverTargets(startable, handoverSession.profileId)}
            badgeFor={badgeFor}
            canSummarize={canRequestSummary(handoverSession, attention.get(handoverSession.id))}
            onSummarize={() => {
              requestSummary(handoverSession.id);
            }}
            onPick={(profile) => {
              void handOver(handoverSession.id, profile);
            }}
            onClose={closeHandover}
          />
        )}
        {linkDialog?.kind === "link" && (
          <LinkDialog
            candidates={linkCandidates(linkState, state.sessions)}
            groups={groupsWithRoom(linkState)}
            onJoin={joinFromDialog}
            initial={[linkDialog.id]}
            badgeFor={badgeFor}
            programOf={(profileId) => profileById.get(profileId)?.name ?? profileId}
            unattended={linkState.unattended}
            selectKey={currentPlatform() === "mac" ? "\u2318" : t("key.ctrl")}
            onLink={linkFromDialog}
            onClose={closeLinkDialog}
          />
        )}
        {editedGroup !== undefined && (
          <LinkGroupDialog
            group={editedGroup}
            language={settings.language}
            titleOf={titleOf}
            candidates={linkCandidates(linkState, state.sessions)}
            defaultName={(session, others) => joinAliases([session], others)[0] ?? ""}
            badgeFor={badgeFor}
            onSave={saveLinkGroup}
            onClose={closeLinkDialog}
          />
        )}
        {historyGroup !== undefined && (
          <LinkHistoryDialog
            color={historyGroup.color}
            load={loadLinkHistory}
            version={linkState.messages}
            language={settings.language}
            copy={writeClipboard}
            onClose={closeLinkDialog}
          />
        )}
        {partnerSession !== undefined && (
          <PartnerMessageDialog
            title={partnerSession.title}
            partners={partnersOfSession(partnerSession)}
            resolve={(partner) => resolvePartner(partnerSession.id, partner)}
            onSend={(send) => {
              void sendToPartner(partnerSession.id, send);
            }}
            onClose={closePartner}
          />
        )}
        {dialog === "costs" && (
          <CostDialog
            language={settings.language}
            budget={settings.dailyBudget}
            onClose={closeDialog}
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
            loaders={launcherLoaders()}
            searchers={[
              (query) =>
                searchNoteItems(query, api.searchNotes, openNoteAt, t, <NotesIcon size={14} />),
            ]}
            onClose={closeDialog}
            onRun={(item) => {
              closeDialog();
              item.run();
            }}
          />
        )}
        {dialog === "searchAll" && (
          <SessionSearchDialog
            sessions={state.sessions}
            targets={() => {
              // The visible session first: its hits are the likeliest to be meant.
              const order = [...state.sessions].sort(
                (a, b) => Number(b.id === state.activeId) - Number(a.id === state.activeId),
              );
              return order.flatMap((session) => {
                const terminal = terminals.get(session.id);
                return terminal ? [{ sessionId: session.id, source: terminal.rowSource() }] : [];
              });
            }}
            seed={searchAllSeed}
            badge={(sessionId) => {
              const session = state.sessions.find((s) => s.id === sessionId);
              return session ? (
                <SessionLook look={badgeFor(session.profileId)} session={session} />
              ) : null;
            }}
            onClose={closeDialog}
            onReveal={revealMatch}
          />
        )}
        {dialog === "worktreeCleanup" && (
          <WorktreeCleanupDialog
            folders={cleanupFolders({
              favorites: folders.favorites,
              recent: folders.recent,
              sessions: state.sessions,
            })}
            language={settings.language}
            onClose={closeDialog}
          />
        )}
        {dialog === "dailyReview" && (
          <DailyReviewDialog
            language={settings.language}
            load={api.dailyReview}
            onCopy={writeClipboard}
            onToNotes={saveReviewNote}
            initialDay={reviewDay}
            onClose={() => {
              setReviewDay(undefined);
              closeDialog();
            }}
          />
        )}
        {dialog === "overview" && (
          <OverviewDialog
            tiles={overviewTiles()}
            activeId={state.activeId}
            readLines={(id) => {
              const terminal = terminals.get(id);
              return terminal ? tailLines(terminal.rowSource()) : [];
            }}
            onActivate={(id) => {
              setDialog(null);
              dispatch({ type: "activated", id });
              focusTerminal();
            }}
            onClose={closeDialog}
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
            snippets={snippets}
            onSend={sendToSeveral}
            onClose={closeDialog}
          />
        )}
        {dialog === "arena" && (
          <ArenaDialog
            agents={arenaAgents}
            folders={arenaFolders}
            badgeFor={badgeFor}
            arena={arena}
            sessions={state.sessions}
            attention={attention}
            onStart={startArena}
            onShow={(id) => {
              setDialog(null);
              dispatch({ type: "activated", id });
              focusTerminal();
            }}
            onResend={sendArenaTask}
            onRunTests={runArenaTests}
            onApply={applyArenaWinner}
            onCloseOthers={closeArenaOthers}
            onClose={closeDialog}
          />
        )}
        {templateDialog?.kind === "save" && (
          <SaveTemplateDialog
            candidates={state.sessions.flatMap((session): TemplateCandidate[] => {
              const profile = profileById.get(session.profileId);
              const names = profile ? [profile.name, profile.id] : [session.profileId];
              const entry = templateSession(session, names);
              return entry === null
                ? []
                : [{ id: session.id, program: profile?.name ?? session.profileId, entry }];
            })}
            templates={templates}
            badgeFor={badgeFor}
            linkable={linkState.sendable}
            members={templateMembers(
              linkState,
              state.sessions.map((session) => session.id),
            )}
            askFirst={
              templateGroup(
                linkState,
                state.sessions.map((session) => session.id),
              )?.askFirst ?? false
            }
            onSave={saveTemplate}
            onClose={closeTemplateDialog}
          />
        )}
        {renamingTemplate !== undefined && (
          <RenameTemplateDialog
            template={renamingTemplate}
            templates={templates}
            onRename={(name) => {
              setTemplates((current) => renameTemplate(current, renamingTemplate.id, name));
              closeTemplateDialog();
            }}
            onClose={closeTemplateDialog}
          />
        )}
        <div className="sr-only" role="status">
          {moveAnnouncement}
        </div>
        {draggedSession && (
          <div ref={attachDragGhost} className="drag-ghost" aria-hidden="true">
            <SessionLook look={badgeFor(draggedSession.profileId)} session={draggedSession} />
            <span className="drag-ghost-title">{draggedSession.title}</span>
          </div>
        )}
        {/* For screen readers: waiting sessions are announced without moving focus. */}
        <div className="sr-only" role="status">
          {waiting
            .map((s) =>
              t(reasons.get(s.id) === "gate" ? "gate.announce" : "attention.announce", {
                title: s.title,
              }),
            )
            .join(". ")}
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
        {dialog === "whatsNew" && changelog !== null && (
          <WhatsNewDialog
            changelog={changelog}
            language={settings.language}
            since={whatsNewSince}
            current={version}
            onClose={closeDialog}
          />
        )}
        {teamOpen && (
          <TeamWindow
            sessions={state.sessions.map((session) => ({ id: session.id, title: session.title }))}
            onClose={closeTeam}
          />
        )}
        {dialog === "settings" && (
          <SettingsDialog
            initialSection={settingsSection}
            settings={settings}
            profiles={profiles}
            versions={programs.versions}
            badgeFor={badgeFor}
            fontSize={layout.fontSize}
            theme={theme}
            update={updater.state}
            updateCheckedAt={updater.checkedAt}
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
            usageConsent={usageConsent}
            onUsageConsent={answerUsage}
            onOpenUsageInfo={openUsageInfo}
            onOpenProfileFile={openProfileFile}
            onOpenHangarMd={openHangarMd}
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
