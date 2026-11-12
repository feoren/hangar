import { Fragment, useEffect, useId, useRef, useState } from "react";

import type { Profile } from "./api";
import { AppMenu, type MenuGroup } from "./AppMenu";
import type { AttentionReason, AttentionState } from "./attention";
import { Badge } from "./Badge";
import { FolderMenu } from "./FolderMenu";
import { elideMiddle, parentFolder } from "./folderSections";
import { folderName, type FolderPrefs } from "./folders";
import { exitDetail } from "./format";
import { gateHint } from "./gate";
import { useT } from "./i18n";
import {
  AlertIcon,
  BranchIcon,
  ChevronIcon,
  CloseIcon,
  DownloadIcon,
  EditIcon,
  FolderIcon,
  HangarLogo,
  OverlapIcon,
  PlusIcon,
  RestartIcon,
  StopIcon,
  WorktreeIcon,
} from "./icons";
import { ProgramUpdateOffer } from "./ProgramUpdateOffer";
import { targetTooltip, type StartTarget, type WorktreeChance } from "./startTarget";
import {
  clampFooterHeight,
  FOOTER_HEIGHT,
  SIDEBAR_WIDTH,
  type SideTab,
  type SidebarGroup,
} from "./layout";
import type { BadgeLook } from "./programPrefs";
import type { SessionView } from "./sessions";
import { labelWithVersion, type ProgramVersions } from "./programs";
import { offerShowsInstalled, runnableUpdate, type ProgramUpdates } from "./programUpdates";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import { markerColorText, SessionMarker } from "./SessionMarker";
import { shortBranch } from "./worktree";
import { revealInvisible } from "./invisible";
import type { EdgeHint } from "./waitingHint";
import { Revealed } from "./Revealed";
import type { DropLine } from "./sessionDrag";
import { formatShortcut, rowMoveStep, SHORTCUT_LABELS } from "./shortcuts";
import { LayoutGlyph } from "./LayoutGlyph";
import { placementText, type PanePlacement } from "./paneGlyph";
import { Splitter } from "./Splitter";
import { detailPlace, sessionLabels, statusSubject, type SessionLabel } from "./sessionLabel";
import { SidebarFooter } from "./SidebarFooter";
import {
  detailLine,
  statusLine,
  STATUS_TICK_MS,
  type Activity,
  type DetailLine,
  type StatusLine,
} from "./statusLine";
import type { ThemeSetting } from "./theme";
import { edgeClass, rowEdge, rowLook, runsAgent, sessionChip, type ChipKind } from "./sessionChip";
import { RepoTeammates } from "./RepoTeammates";
import { lookClass } from "./stateLook";
import {
  agentGroupKey,
  agentGroups,
  groupStepIndex,
  sessionGroup,
  shownSessions,
  type SessionGroup,
} from "./sessionGroups";
import { StatusChip } from "./StatusChip";
import { SubAgentTree } from "./SubAgentTree";
import { SessionActionGuard } from "./timing";
import { pillProgress, updatePill, updatePillHint } from "./updatePill";
import { useClock } from "./useClock";
import type { UpdateState } from "./useUpdater";
import { useWaitingEdges } from "./useWaitingEdges";
import { ContextMeter } from "./ContextMeter";
import { displayVersion } from "./version";
import { VersionText } from "./VersionText";

interface Props {
  sessions: SessionView[];
  activeId: number | null;
  profiles: Profile[];
  /** Version per profile ID; empty while the query runs or when it is turned off. */
  versions: ProgramVersions;
  /** New versions per profile ID; empty without a check. */
  updates: ProgramUpdates;
  /** The right panel's tab on screen in this window, `null` when closed or detached. */
  sideTab: SideTab | null;
  onToggleSideTab: (tab: SideTab) => void;
  /** The right panel shows the active session's folder as a file tree. */
  filesOpen: boolean;
  /** The folder button on the active row: shows or hides that file tree. */
  onToggleFiles: () => void;
  /** Launchable entries in the chosen order, hidden ones left out. */
  startable: Profile[];
  /** Installed entries exist, but all of them are hidden. */
  allHidden: boolean;
  /** Badge appearance per profile ID (user choice, program icon or letters). */
  badgeFor: (profileId: string) => BadgeLook;
  closedGroups: readonly SidebarGroup[];
  onToggleGroup: (group: SidebarGroup) => void;
  /** Folded agent groups (sessionGroups.ts agentGroupKey). */
  closedAgentGroups: readonly string[];
  onToggleAgentGroup: (key: string) => void;
  /** After a mouse click on a header button: focus goes back to the terminal. */
  onFocusTerminal: () => void;
  /** Hangar's own version (as getVersion returns it), `null` until known. */
  version: string | null;
  /** Chosen color mode ("System" follows the system); set from the account menu. */
  themeSetting: ThemeSetting;
  onSetTheme: (theme: ThemeSetting) => void;
  /** Where the next session starts (startTarget.ts). */
  target: StartTarget;
  /** Project folders of the running sessions, for the folder menu. */
  runningProjects: readonly string[];
  /** Whether the target can get an own worktree. */
  worktree: WorktreeChance;
  collapsed: boolean;
  width: number;
  footerHeight: number;
  menu: MenuGroup[];
  update: UpdateState;
  attention: ReadonlyMap<number, AttentionState>;
  /** Why a waiting session waits; only waiting sessions have an entry. */
  reasons: ReadonlyMap<number, AttentionReason>;
  /** Times of a session for its status line, sampled at `now` (outside React, cheap). */
  activityOf: (id: number, now: number) => Activity;
  /** Git branch per working folder; missing or `null`: none shown. */
  branches: ReadonlyMap<string, string | null>;
  /** Sessions that change a file another session changes too: the marker text (claims.ts). */
  overlaps: ReadonlyMap<number, string>;
  /** Sessions shown in a pane of a split layout: layout and zone, for the row's miniature. */
  placements: ReadonlyMap<number, PanePlacement>;
  /** UI language, for the compact durations. */
  language: string;
  /** Session whose name is being edited in the sidebar right now. */
  renamingId: number | null;
  onRenameStart: (id: number) => void;
  onRenameEnd: () => void;
  onWidth: (width: number) => void;
  onFooterHeight: (height: number) => void;
  onToggleCollapsed: () => void;
  onMenuClose: () => void;
  onActivate: (id: number) => void;
  /** Starts a program in the target; `ownWorktree` only when `worktree` is "yes". */
  onStart: (profile: Profile, ownWorktree: boolean) => void;
  /** Profile IDs whose update session runs right now. */
  updatingPrograms: ReadonlySet<string>;
  /** Program whose update waits for "Update anyway" (it is running), otherwise `null`. */
  confirmUpdate: string | null;
  /** "Update" clicked: the app runs it or asks first. */
  onRequestUpdate: (profileId: string) => void;
  onConfirmUpdate: (profileId: string) => void;
  onCancelUpdate: () => void;
  /** Opens the folder dialog; the folder applies to the next start. */
  onPickCwd: () => void;
  /** Favorite and recent folders for the folder menu. */
  folders: FolderPrefs;
  /** A folder from the menu: it applies to the next start. */
  onSelectCwd: (path: string) => void;
  /** Back from a chosen folder to the default target. */
  onResetTarget: () => void;
  onToggleFavorite: (path: string) => void;
  onRestart: (id: number) => void;
  onKill: (id: number) => void;
  onClose: (id: number) => void;
  onRename: (id: number, title: string) => void;
  /** Right click or context menu key on a session: the session menu at this point. */
  onSessionMenu: (id: number, x: number, y: number) => void;
  /** A session row is being dragged: which one, and where the drop line shows. */
  drag: { id: number; line: DropLine | null } | null;
  /** Mouse down on a session row: may become a drag (useSessionDrag). */
  onDragStart: (id: number, event: React.PointerEvent<HTMLElement>) => void;
  /** Alt+Up or Alt+Down on a row: the session moves to this place in the list. */
  onMove: (id: number, index: number) => void;
  onInstallUpdate: () => void;
  onDismissUpdate: () => void;
  /** Sessions come back after a restart (the update button's tooltip says so). */
  restoresSessions: boolean;
  /** The Team window is open (the footer's team pill shows it as expanded). */
  teamOpen: boolean;
  onToggleTeam: () => void;
  /** Settings on the Account section. */
  onOpenAccount: () => void;
  onOpenSettings: () => void;
  /** Sub-agents of the active session for the tree below its row; `null`: none watched. */
  subAgentTree: { sessionId: number; watchKey: string; hidden: ReadonlySet<string> } | null;
  /** Session whose sub-agent view is open, otherwise `null`. */
  subAgentsOpen: number | null;
  /** Sub-agent shown in the open view. */
  subAgentPick: string | null;
  /** A tree row: shows this sub-agent (`null`: the list of all of them). */
  onShowSubAgent: (sessionId: number, agentId: string | null, byKeyboard: boolean) => void;
  /** Removes finished sub-agents from the tree (by keyboard: the focus stays in the tree). */
  onDismissSubAgents: (ids: readonly string[], byKeyboard: boolean) => void;
}

export function Sidebar(props: Props) {
  const { sessions, activeId, profiles, versions, badgeFor, target, collapsed, update, attention } =
    props;
  const { startable } = props;
  const t = useT();
  const now = useClock(STATUS_TICK_MS);
  const root = useRef<HTMLElement>(null);
  const toggleButton = useRef<HTMLButtonElement>(null);
  /** Collapsed or expanded via keyboard: focus moves to the matching button of the new view. */
  const refocusToggle = useRef(false);
  const profileById = new Map(profiles.map((p) => [p.id, p]));
  const [guard] = useState(() => new SessionActionGuard());
  const newButton = useRef<HTMLButtonElement>(null);
  /** Program picker of the collapsed bar, open at this position. */
  const [programMenu, setProgramMenu] = useState<{ x: number; y: number } | null>(null);
  const cwdButton = useRef<HTMLButtonElement>(null);
  /** "Starts in" menu, open at this position (below the button, or above it if space is short). */
  const [folderMenu, setFolderMenu] = useState<{ x: number; y: number; flipAbove: number } | null>(
    null,
  );
  const onClose = (id: number) => {
    if (!guard.allows()) return;
    guard.closed();
    props.onClose(id);
  };
  const onKill = (id: number) => {
    if (!guard.allows()) return;
    guard.killed(id);
    props.onKill(id);
  };
  const onRestart = (id: number) => {
    if (guard.allowsRestart(id)) props.onRestart(id);
  };
  const onRename = (id: number) => {
    if (guard.allows()) props.onRenameStart(id);
  };
  /**
   * Alt+Up / Alt+Down on a row: one place up or down within its group; the focus stays on the
   * moved row.
   */
  const onRowKey = (event: React.KeyboardEvent<HTMLElement>, id: number) => {
    const step = rowMoveStep(event);
    if (step === null) return;
    event.preventDefault();
    const target = groupStepIndex(sessions, id, step);
    if (target === null) return;
    props.onMove(id, target);
    // React moves the row's element; moving a focused node in the DOM can drop its focus.
    requestAnimationFrame(() => {
      root.current
        ?.querySelector<HTMLElement>(`[data-session-row="${String(id)}"] button`)
        ?.focus();
    });
  };
  /** Classes of a row while a session is dragged: the dragged one fades, one shows the line. */
  const dragClass = (id: number) => {
    const drag = props.drag;
    if (drag === null) return "";
    if (drag.id === id) return "dragging";
    if (drag.line?.id === id) return `drop-${drag.line.edge}`;
    return "";
  };
  const moveHint = t("session.moveHint");

  useEffect(() => {
    if (!refocusToggle.current) return;
    refocusToggle.current = false;
    toggleButton.current?.focus();
  }, [collapsed]);

  // The active session stays in sight, also after switching by shortcut or from quick access.
  useEffect(() => {
    if (activeId === null) return;
    root.current
      ?.querySelector(`[data-session-row="${String(activeId)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeId, collapsed]);

  // Two groups: ordinary sessions, and the ones in their own worktree (sessionGroups.ts).
  const ordinary = sessions.filter((s) => sessionGroup(s) === "sessions");
  const ownTrees = sessions.filter((s) => sessionGroup(s) === "worktrees");
  // Sessions lead with their project folder; worktree rows keep their branch-named titles.
  const folderLabels = sessionLabels(ordinary, (id) => profileById.get(id)?.name);
  /** Both lines of a session's status; the same text for the row and the rail's tooltip. */
  const lines = (session: SessionView) => {
    const state = attention.get(session.id) ?? "idle";
    const label = folderLabels.get(session.id) ?? null;
    return {
      status: statusLine(
        statusSubject(label, profileById.get(session.profileId)?.name, session.title),
        session,
        state,
        props.activityOf(session.id, now),
        now,
        t,
        props.language,
        props.reasons.get(session.id) ?? null,
      ),
      // A worktree's folder name repeats its branch; the repository's name says more.
      detail: detailLine(
        session.worktree ? { ...session, cwd: session.worktree.repoRoot } : session,
        props.branches.get(session.cwd) ?? null,
        now,
        props.language,
      ),
    };
  };
  const waitingIn = (list: readonly SessionView[]) =>
    list.filter((s) => attention.get(s.id) === "attention").map((s) => s.id);
  const agentOpen = (key: string) => !props.closedAgentGroups.includes(key);
  /** Rows a list shows: folded agent groups show none. */
  const shownIn = (list: readonly SessionView[]) =>
    agentGroups(list).flatMap((group) => shownSessions(group.sessions, agentOpen(group.key)));
  const shownOrdinary = shownIn(ordinary);
  const shownTrees = shownIn(ownTrees);
  // Waiting sessions scrolled out of view: a calm hint at that edge of each list. Rows in a folded
  // agent group are not there to scroll to; their group's header shows them instead.
  const sessionList = useRef<HTMLUListElement>(null);
  const worktreeList = useRef<HTMLUListElement>(null);
  const edgeHints = useWaitingEdges(
    sessionList,
    shownOrdinary.map((s) => s.id),
    waitingIn(shownOrdinary),
    !collapsed && !props.closedGroups.includes("sessions"),
  );
  const worktreeHints = useWaitingEdges(
    worktreeList,
    shownTrees.map((s) => s.id),
    waitingIn(shownTrees),
    !collapsed && !props.closedGroups.includes("worktrees"),
  );
  const revealRow = (id: number) => {
    root.current
      ?.querySelector(`[data-session-row="${String(id)}"]`)
      ?.scrollIntoView({ block: "nearest", behavior: reducedMotion() ? "auto" : "smooth" });
  };

  /** Folder menu of the rail, opened from the program picker's first row. */
  const [railFolderMenu, setRailFolderMenu] = useState<{ x: number; y: number } | null>(null);
  const chosen = target.source === "chosen";
  const targetTitle = targetTooltip(target, t);
  const targetName = target.path === null ? t("sidebar.pickCwd") : folderName(target.path);
  const targetParent = target.path === null ? null : parentFolder(target.path);
  /** Branch of the target, when a running session in that folder already told it; no extra git call. */
  const targetBranch = target.path === null ? null : (props.branches.get(target.path) ?? null);
  /** Props every folder menu shares; position, anchor and closing differ per place. */
  const folderMenuProps = {
    target: target.path,
    running: props.runningProjects,
    folders: props.folders,
    onSelect: props.onSelectCwd,
    onChoose: props.onPickCwd,
    onToggleFavorite: props.onToggleFavorite,
  };

  /** Mouse (detail > 0): focus to the terminal. Keyboard (detail 0): focus stays on the button. */
  const onToggleClick = (event: React.MouseEvent) => {
    if (event.detail > 0) {
      props.onToggleCollapsed();
      props.onFocusTerminal();
    } else {
      refocusToggle.current = true;
      props.onToggleCollapsed();
    }
  };
  const open = (group: SidebarGroup) => !props.closedGroups.includes(group);
  const waitingCount = sessions.filter((s) => attention.get(s.id) === "attention").length;
  const appVersion = props.version === null ? "" : displayVersion(props.version, t("version.beta"));
  const toggleKeys = formatShortcut(SHORTCUT_LABELS.toggleSidebar, t);
  const waitingText = t("sidebar.waitingCount", { count: waitingCount });
  const chipOf = (session: SessionView) =>
    sessionChip(
      session,
      attention.get(session.id) ?? "idle",
      props.reasons.get(session.id) ?? null,
      runsAgent(profileById.get(session.profileId)?.command, session.kind),
    );
  const toggleLabel = collapsed ? t("sidebar.expand") : t("sidebar.collapse");
  /** Round button on the sidebar's right edge; the same place collapsed and expanded. */
  const edgeToggle = (
    <button
      ref={toggleButton}
      type="button"
      className={collapsed ? "edge-toggle collapsed" : "edge-toggle"}
      aria-label={toggleLabel}
      aria-expanded={!collapsed}
      title={`${toggleLabel} (${toggleKeys})`}
      onClick={onToggleClick}
    >
      <ChevronIcon size={12} />
    </button>
  );
  const footer = (
    <SidebarFooter
      collapsed={collapsed}
      teamOpen={props.teamOpen}
      onToggleTeam={props.onToggleTeam}
      onOpenAccount={props.onOpenAccount}
      sideTab={props.sideTab}
      onToggleSideTab={props.onToggleSideTab}
      onOpenSettings={props.onOpenSettings}
      themeSetting={props.themeSetting}
      onSetTheme={props.onSetTheme}
      onMenuClose={props.onMenuClose}
    />
  );

  if (collapsed) {
    // First the target (opens the folder menu), then the programs, then runnable updates.
    const programItems: PopupItem[] = [
      {
        type: "action",
        key: "target",
        label: t("start.inFolder", { folder: targetName }),
        ...(chosen ? { detail: t("start.sourceChosen") } : {}),
        title: targetTitle,
        icon: <FolderIcon size={14} />,
        run: () => {
          // Opens where the program picker was.
          setRailFolderMenu(programMenu);
        },
      },
      ...(props.worktree === "no"
        ? [
            {
              type: "note",
              key: "no-worktree",
              label: t("start.worktreeNoRepo"),
              describes: "target",
            } as const,
          ]
        : []),
      { type: "separator", key: "sep-programs" },
      ...startable.map((profile): PopupItem => {
        const version = versions.get(profile.id);
        return {
          type: "action",
          key: profile.id,
          label: profile.name,
          ...(version === undefined ? {} : { detail: version }),
          icon: <Badge look={badgeFor(profile.id)} />,
          ...(props.worktree === "yes"
            ? {
                secondary: {
                  label: t("start.withWorktree", { name: profile.name }),
                  icon: <WorktreeIcon size={14} />,
                  run: () => {
                    props.onStart(profile, true);
                  },
                },
              }
            : {}),
          run: () => {
            props.onStart(profile, false);
          },
        };
      }),
    ];
    const updatable = startable.filter((profile) => runnableUpdate(props.updates, profile.id));
    if (updatable.length > 0) {
      programItems.push({ type: "separator", key: "sep-updates" });
      for (const profile of updatable) {
        const entry = props.updates.get(profile.id);
        const latest = entry?.latest ?? "";
        const installed = entry?.installed ?? "?";
        programItems.push({
          type: "action",
          key: `update:${profile.id}`,
          label: t("update.runFor", { name: profile.name }),
          detail: props.updatingPrograms.has(profile.id)
            ? t("update.running")
            : `${installed} → ${latest}`,
          title: [
            t("update.runFromTo", { name: profile.name, installed, latest }),
            entry?.needsAdmin === true ? t("update.needsAdmin") : "",
          ]
            .filter(Boolean)
            .join("\n"),
          icon: <DownloadIcon size={14} />,
          run: () => {
            props.onRequestUpdate(profile.id);
          },
        });
      }
    }
    const newKeys = formatShortcut(SHORTCUT_LABELS.newSession, t);
    return (
      <aside className="sidebar collapsed" ref={root}>
        {/* The menu sits in a head as tall as the expanded header, so the line below it and the
            round toggle on it stay at the same height in both states. */}
        <div className="rail-head">
          <AppMenu groups={props.menu} onClose={props.onMenuClose} />
        </div>
        <div className="sidebar-rule" aria-hidden="true" />
        <div className="rail-top">
          <span className="rail-logo" title={`Hangar ${appVersion}`.trim()}>
            <HangarLogo size={20} />
          </span>
          {/* Number of waiting sessions, also when the list scrolls; a ring, not color alone. */}
          {waitingCount > 0 && (
            <span className="rail-waiting" title={waitingText}>
              <span className="dot attention" aria-hidden="true" />
              <span className="rail-waiting-count" aria-hidden="true">
                {waitingCount > 9 ? "9+" : waitingCount}
              </span>
              <span className="sr-only">{waitingText}</span>
            </span>
          )}
        </div>
        {edgeToggle}
        <ul className="rail-sessions" data-session-list="">
          {sessions.map((session, index) => {
            // The same name as the expanded row's title; the status names the program after it:
            // "hangar · Claude Code is working".
            const label = folderLabels.get(session.id);
            const name = label?.byFolder === true ? label.primary : session.title;
            const state = attention.get(session.id) ?? "idle";
            const { status: line, detail: where } = lines(session);
            const status = line.full;
            const detail = exitDetail(session, t);
            const chip = chipOf(session);
            const placement = props.placements.get(session.id);
            const placeText = placement === undefined ? "" : placementText(t, placement);
            const className = [
              "rail-session",
              session.id === activeId ? "active" : "",
              state === "attention" ? "attention" : "",
              lookClass(rowLook(session, state, chip)),
            ];
            const group = sessionGroup(session);
            const previous = sessions[index - 1];
            // A thin line where the worktree sessions begin, as the two groups do when expanded.
            const divider = previous !== undefined && sessionGroup(previous) !== group;
            return (
              <Fragment key={session.id}>
                {divider && <li className="rail-divider" aria-hidden="true" />}
                <li
                  data-session-row={session.id}
                  data-session-group={agentGroupKey(session)}
                  className={[dragClass(session.id), group === "worktrees" ? "rail-worktree" : ""]
                    .filter(Boolean)
                    .join(" ")}
                >
                  <button
                    type="button"
                    className={className.filter(Boolean).join(" ")}
                    title={[
                      `${name} · ${status}`,
                      props.overlaps.get(session.id) ?? "",
                      detailText(where, t),
                      session.worktree
                        ? t("worktree.sessionHint", {
                            branch: revealInvisible(session.worktree.branch),
                          })
                        : "",
                      session.cwd,
                      placeText,
                      markerColorText(session.marker, t),
                      detail,
                      programLine(profileById.get(session.profileId), versions),
                      moveHint,
                    ]
                      .filter(Boolean)
                      .join("\n")}
                    aria-label={[
                      session.marker?.emoji,
                      name,
                      status,
                      props.overlaps.get(session.id),
                      placeText,
                      markerColorText(session.marker, t),
                    ]
                      .filter(Boolean)
                      .join(", ")}
                    aria-current={session.id === activeId ? "true" : undefined}
                    onClick={() => {
                      props.onActivate(session.id);
                    }}
                    onPointerDown={(event) => {
                      props.onDragStart(session.id, event);
                    }}
                    onKeyDown={(event) => {
                      onRowKey(event, session.id);
                    }}
                    onContextMenu={(event) => {
                      openSessionMenu(event, session.id, props.onSessionMenu);
                    }}
                  >
                    <Badge look={badgeFor(session.profileId)} />
                    {session.marker?.color != null && (
                      <span
                        className="rail-marker"
                        style={{ background: session.marker.color }}
                        aria-hidden="true"
                      />
                    )}
                    {session.marker?.emoji != null && (
                      <span className="rail-emoji" aria-hidden="true">
                        {session.marker.emoji}
                      </span>
                    )}
                    {props.overlaps.has(session.id) && (
                      <span className="rail-overlap" aria-hidden="true">
                        <OverlapIcon size={10} />
                      </span>
                    )}
                    {railShowsChip(chip) ? (
                      <span className="rail-chip">
                        <StatusChip kind={chip} compact />
                      </span>
                    ) : (
                      <span
                        className={`rail-dot dot ${state === "attention" ? "attention" : session.status}`}
                      />
                    )}
                  </button>
                </li>
              </Fragment>
            );
          })}
        </ul>
        {startable.length > 0 && (
          <button
            ref={newButton}
            type="button"
            className="icon-button rail-new"
            aria-label={t("sidebar.newSession")}
            aria-haspopup="menu"
            aria-expanded={programMenu !== null}
            title={`${t("sidebar.newSession")} (${newKeys})`}
            onClick={(event) => {
              if (programMenu !== null) {
                setProgramMenu(null);
                return;
              }
              const rect = event.currentTarget.getBoundingClientRect();
              setProgramMenu({ x: rect.right + 6, y: rect.top });
            }}
          >
            <PlusIcon />
          </button>
        )}
        {programMenu !== null && (
          <PopupMenu
            items={programItems}
            label={t("sidebar.newSession")}
            position={programMenu}
            anchor={newButton}
            onClose={() => {
              setProgramMenu(null);
              // Focus goes to the terminal only if nothing else took it (the folder menu).
              props.onMenuClose();
            }}
          />
        )}
        {railFolderMenu !== null && (
          <FolderMenu
            {...folderMenuProps}
            position={railFolderMenu}
            anchor={newButton}
            onClose={(reason) => {
              setRailFolderMenu(null);
              if (reason === "dismiss") newButton.current?.focus();
              else props.onMenuClose();
            }}
          />
        )}
        <div className="rail-bottom">
          {update.kind === "available" && (
            <button
              type="button"
              className="icon-button rail-update"
              aria-label={t("update.available", {
                version: displayVersion(update.update.version, t("version.beta")),
              })}
              title={t("update.available", {
                version: displayVersion(update.update.version, t("version.beta")),
              })}
              onClick={props.onToggleCollapsed}
            >
              <DownloadIcon />
            </button>
          )}
          {footer}
        </div>
      </aside>
    );
  }

  const sessionsOpen = open("sessions");
  const worktreesOpen = open("worktrees");
  const newSessionOpen = open("newSession");
  const subAgentTree = props.subAgentTree;
  const sessionsTitle = sessionsOpen
    ? t("sidebar.sessions")
    : t("sidebar.sessionsCount", { count: ordinary.length });
  const worktreesTitle = worktreesOpen
    ? t("sidebar.worktrees")
    : t("sidebar.worktreesCount", { count: ownTrees.length });
  /** Collapsed group title: a ring with the number of waiting sessions inside it. */
  const groupWaiting = (list: readonly SessionView[], open: boolean) => {
    const count = waitingIn(list).length;
    if (open || count === 0) return null;
    const text = t("sidebar.waitingCount", { count });
    return (
      <span className="group-waiting" title={text}>
        <span className="dot attention" aria-hidden="true" />
        <span className="sr-only">{text}</span>
      </span>
    );
  };
  /**
   * The rows of one group's list, gathered per program into agent groups whose rows hang off the
   * header like a tree; the sub-agent tree hangs below the active row.
   */
  const rows = (list: readonly SessionView[], group: SessionGroup) => {
    const labels = group === "sessions" ? folderLabels : new Map<number, SessionLabel>();
    return agentGroups(list).map((agents) => {
      const open = agentOpen(agents.key);
      const shown = shownSessions(agents.sessions, open);
      const shownIds = new Set(shown.map((s) => s.id));
      const hiddenWaiting = waitingIn(agents.sessions.filter((s) => !shownIds.has(s.id))).length;
      const profile = profileById.get(agents.profileId);
      return (
        <AgentGroupSection
          key={agents.key}
          groupKey={agents.key}
          name={profile?.name ?? agents.sessions[0]?.title ?? agents.profileId}
          look={badgeFor(agents.profileId)}
          count={agents.sessions.length}
          open={open}
          hasRows={shown.length > 0}
          hiddenWaiting={hiddenWaiting}
          holdsActive={!open && agents.sessions.some((s) => s.id === activeId)}
          onToggle={props.onToggleAgentGroup}
        >
          {shown.map((session, index) => {
            const last = index === shown.length - 1;
            const tree = subAgentTree?.sessionId === session.id ? subAgentTree : null;
            // With a tree below the row, "drop after" draws its line below the tree.
            const drag = dragClass(session.id);
            const rowDrag = tree !== null && drag === "drop-after" ? "" : drag;
            const treeDrag = tree !== null && drag !== "drop-before" ? drag : "";
            return (
              <Fragment key={session.id}>
                <SessionItem
                  lines={lines(session)}
                  label={labels.get(session.id) ?? null}
                  session={session}
                  groupKey={agents.key}
                  last={last}
                  program={programLine(profile, versions)}
                  active={session.id === activeId}
                  state={attention.get(session.id) ?? "idle"}
                  chip={chipOf(session)}
                  editing={props.renamingId === session.id}
                  dragClass={rowDrag}
                  moveHint={moveHint}
                  placement={props.placements.get(session.id) ?? null}
                  {...props}
                  onClose={onClose}
                  onKill={onKill}
                  onRestart={onRestart}
                  onRenameStart={onRename}
                  onRowKey={onRowKey}
                />
                {/* The tree belongs to the row above it; as its own item, the row's layout stays.
                    Below a middle row, the agent group's guide line runs on past it. */}
                {tree !== null && (
                  <li
                    className={["subagent-tree-row", last ? "" : "tree-continues", treeDrag]
                      .filter(Boolean)
                      .join(" ")}
                  >
                    <SubAgentTree
                      sessionId={session.id}
                      sessionTitle={session.title}
                      watchKey={tree.watchKey}
                      picked={props.subAgentsOpen === session.id ? props.subAgentPick : null}
                      hidden={tree.hidden}
                      onShow={props.onShowSubAgent}
                      onDismiss={props.onDismissSubAgents}
                    />
                  </li>
                )}
              </Fragment>
            );
          })}
        </AgentGroupSection>
      );
    });
  };

  return (
    <aside className="sidebar" ref={root}>
      <div className="sidebar-header">
        <AppMenu groups={props.menu} onClose={props.onMenuClose} />
        <span className="brand">
          <HangarLogo />
          <span className="brand-name">Hangar</span>
          {props.version !== null && (
            <VersionText version={props.version} className="brand-version" />
          )}
        </span>
        <UpdatePillButton
          state={update}
          running={sessions.filter((s) => s.status !== "exited").length}
          restoresSessions={props.restoresSessions}
          onInstall={props.onInstallUpdate}
        />
      </div>
      {edgeToggle}
      {/* Decorative: separates the app's own controls from the sessions below. */}
      <div className="sidebar-rule" aria-hidden="true" />

      {/* Sessions and worktrees stack from the top, the worktrees right below the last session;
          an open Sessions group lends the pair the free height. */}
      <div className={sessionsOpen ? "session-groups fill" : "session-groups"}>
        <GroupSection
          group="sessions"
          title={sessionsTitle}
          open={sessionsOpen}
          className="sessions-group"
          onToggle={props.onToggleGroup}
          marker={groupWaiting(ordinary, sessionsOpen)}
        >
          <WaitingEdge edge="above" hint={edgeHints.above} onReveal={revealRow} />
          <ul className="session-list" data-session-list="" ref={sessionList}>
            {sessions.length === 0 && <li className="hint">{t("sidebar.noSessions")}</li>}
            {rows(ordinary, "sessions")}
          </ul>
          <WaitingEdge edge="below" hint={edgeHints.below} onReveal={revealRow} />
        </GroupSection>

        {/* Only while a session works in its own worktree; an empty group would just be noise. */}
        {ownTrees.length > 0 && (
          <GroupSection
            group="worktrees"
            title={worktreesTitle}
            open={worktreesOpen}
            className={worktreesOpen ? "worktrees-group" : "worktrees-group closed"}
            onToggle={props.onToggleGroup}
            marker={groupWaiting(ownTrees, worktreesOpen)}
          >
            <WaitingEdge edge="above" hint={worktreeHints.above} onReveal={revealRow} />
            <ul className="session-list" data-session-list="" ref={worktreeList}>
              {rows(ownTrees, "worktrees")}
            </ul>
            <WaitingEdge edge="below" hint={worktreeHints.below} onReveal={revealRow} />
          </GroupSection>
        )}
      </div>

      <UpdateNotice state={update} onDismiss={props.onDismissUpdate} />

      {/* Space can only be shared between two open groups. */}
      {sessionsOpen && newSessionOpen && (
        <Splitter
          orientation="horizontal"
          value={props.footerHeight}
          direction={-1}
          label={t("sidebar.footerHeight")}
          onChange={(height) => {
            props.onFooterHeight(clampFooterHeight(height, root.current?.clientHeight ?? 0));
          }}
          onReset={() => {
            props.onFooterHeight(
              clampFooterHeight(FOOTER_HEIGHT.default, root.current?.clientHeight ?? 0),
            );
          }}
        />
      )}

      <GroupSection
        group="newSession"
        title={t("sidebar.newSession")}
        open={newSessionOpen}
        className={
          newSessionOpen
            ? sessionsOpen
              ? "new-session"
              : "new-session fill"
            : "new-session closed"
        }
        style={newSessionOpen && sessionsOpen ? { height: props.footerHeight } : undefined}
        onToggle={props.onToggleGroup}
      >
        <div className={chosen ? "cwd-row chosen" : "cwd-row"}>
          <button
            ref={cwdButton}
            type="button"
            className="cwd"
            title={[targetTitle, props.worktree === "no" ? t("start.worktreeNoRepo") : ""]
              .filter(Boolean)
              .join("\n")}
            aria-haspopup="menu"
            aria-expanded={folderMenu !== null}
            onClick={(event) => {
              if (folderMenu !== null) {
                setFolderMenu(null);
                return;
              }
              const rect = event.currentTarget.getBoundingClientRect();
              setFolderMenu({ x: rect.left, y: rect.bottom + 4, flipAbove: rect.top - 4 });
            }}
          >
            <span className="cwd-icon" aria-hidden="true">
              <FolderIcon size={16} />
            </span>
            <span className="cwd-text">
              <span className="cwd-label">
                {chosen ? t("start.nextIn") : t("sidebar.startsIn")}
              </span>
              <span className="cwd-name">{targetName}</span>
              {(targetParent !== null || targetBranch !== null) && (
                <span className="cwd-sub">
                  {targetParent !== null && (
                    <span className="cwd-parent">{elideMiddle(targetParent, 34)}</span>
                  )}
                  {targetBranch !== null && (
                    <span className="cwd-branch">
                      <BranchIcon size={11} />
                      <span className="sr-only">
                        {t("session.branch", { branch: revealInvisible(targetBranch) })}
                      </span>
                      <span className="cwd-branch-name" aria-hidden="true">
                        <Revealed text={targetBranch} />
                      </span>
                    </span>
                  )}
                </span>
              )}
            </span>
            <span className="cwd-chevron" aria-hidden="true">
              <ChevronIcon size={12} />
            </span>
          </button>
          {chosen && (
            <button
              type="button"
              className="icon-button cwd-reset"
              aria-label={t("start.reset")}
              title={t("start.reset")}
              onClick={props.onResetTarget}
            >
              <CloseIcon size={12} />
            </button>
          )}
        </div>
        {folderMenu !== null && (
          <FolderMenu
            {...folderMenuProps}
            position={folderMenu}
            flipAbove={folderMenu.flipAbove}
            anchor={cwdButton}
            onClose={(reason) => {
              setFolderMenu(null);
              // Escape returns to the button; a chosen folder returns to the terminal.
              if (reason === "dismiss") cwdButton.current?.focus();
              else props.onMenuClose();
            }}
          />
        )}
        {props.allHidden && <p className="hint group-hint">{t("sidebar.allHidden")}</p>}
        <ul className="profile-list">
          {startable.map((profile) => {
            const version = versions.get(profile.id);
            const programUpdate = props.updates.get(profile.id);
            return (
              <li key={profile.id} className="profile-row">
                <button
                  type="button"
                  className="profile"
                  title={[
                    [profile.command, ...profile.args].join(" "),
                    // The line below the folder is gone; Shift+click stays findable here.
                    props.worktree === "yes"
                      ? t("start.worktreeHint", { shift: t("key.shift") })
                      : "",
                  ]
                    .filter(Boolean)
                    .join("\n")}
                  onClick={(event) => {
                    props.onStart(profile, event.shiftKey && props.worktree === "yes");
                  }}
                >
                  <Badge look={badgeFor(profile.id)} />
                  <span className="profile-name">{profile.name}</span>
                  {version && !offerShowsInstalled(programUpdate) && (
                    <span className="profile-version">
                      <span aria-hidden="true">·</span> {version}
                    </span>
                  )}
                  {programUpdate?.updateAvailable === true && (
                    <span className="dot update profile-update-dot" aria-hidden="true" />
                  )}
                  <PlusIcon />
                </button>
                {props.worktree === "yes" && (
                  <button
                    type="button"
                    className="profile-worktree"
                    aria-label={t("start.withWorktree", { name: profile.name })}
                    title={t("start.withWorktree", { name: profile.name })}
                    onClick={() => {
                      props.onStart(profile, true);
                    }}
                  >
                    {t("start.worktreeButton")}
                  </button>
                )}
                {programUpdate !== undefined && (
                  <ProgramUpdateOffer
                    className="in-sidebar"
                    name={profile.name}
                    update={programUpdate}
                    updating={props.updatingPrograms.has(profile.id)}
                    confirming={props.confirmUpdate === profile.id}
                    onRequest={() => {
                      props.onRequestUpdate(profile.id);
                    }}
                    onConfirm={() => {
                      props.onConfirmUpdate(profile.id);
                    }}
                    onCancel={props.onCancelUpdate}
                  />
                )}
              </li>
            );
          })}
        </ul>
      </GroupSection>

      {footer}

      <Splitter
        orientation="vertical"
        className="sidebar-edge"
        value={props.width}
        direction={1}
        label={t("sidebar.width")}
        onChange={props.onWidth}
        onReset={() => {
          props.onWidth(SIDEBAR_WIDTH.default);
        }}
      />
    </aside>
  );
}

/**
 * Collapsible group of the sidebar (sessions, new session, more later). The title is a
 * button (Enter, Space); when collapsed, only the title row remains. The content stays in the DOM
 * (hidden) so that aria-controls always points at an element.
 */
function GroupSection({
  group,
  title,
  open,
  className,
  style,
  marker,
  onToggle,
  children,
}: {
  group: SidebarGroup;
  title: string;
  open: boolean;
  className: string;
  style?: React.CSSProperties | undefined;
  /** Extra text after the title, e.g. the hint about waiting sessions. */
  marker?: React.ReactNode;
  onToggle: (group: SidebarGroup) => void;
  children: React.ReactNode;
}) {
  const bodyId = useId();
  return (
    <section className={`sidebar-group ${className}`} style={style}>
      <h2 className="section-title group-title">
        <button
          type="button"
          className="group-toggle"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => {
            onToggle(group);
          }}
        >
          <ChevronIcon size={12} />
          <span className="group-label">{title}</span>
          {marker}
        </button>
      </h2>
      <div id={bodyId} className="group-body" hidden={!open}>
        {children}
      </div>
    </section>
  );
}

/**
 * The sessions of one program inside Sessions or Worktrees: a header with the program's badge,
 * name and number of sessions, its rows hanging below like a tree. Folded, the header names the
 * waiting sessions it hides and whether the active session is among them.
 */
function AgentGroupSection({
  groupKey,
  name,
  look,
  count,
  open,
  hasRows,
  hiddenWaiting,
  holdsActive,
  onToggle,
  children,
}: {
  groupKey: string;
  name: string;
  look: BadgeLook;
  count: number;
  open: boolean;
  hasRows: boolean;
  /** Waiting sessions whose rows the fold hides. */
  hiddenWaiting: number;
  /** Folded with the active session inside: the header stands in for its row. */
  holdsActive: boolean;
  onToggle: (key: string) => void;
  children: React.ReactNode;
}) {
  const t = useT();
  const bodyId = useId();
  const countText = t("sidebar.agentGroupCount", { count });
  const waitingText = t("sidebar.waitingCount", { count: hiddenWaiting });
  const activeText = t("sidebar.agentGroupHoldsActive");
  return (
    <li className={open ? "agent-group" : "agent-group closed"}>
      <h3 className="agent-group-title">
        <button
          type="button"
          className={
            holdsActive
              ? "group-toggle agent-group-toggle holds-active"
              : "group-toggle agent-group-toggle"
          }
          aria-expanded={open}
          aria-controls={bodyId}
          title={[
            `${name} · ${countText}`,
            holdsActive ? activeText : "",
            hiddenWaiting > 0 ? waitingText : "",
          ]
            .filter(Boolean)
            .join("\n")}
          onClick={() => {
            onToggle(groupKey);
          }}
        >
          <ChevronIcon size={12} />
          <Badge look={look} />
          <span className="agent-group-name">{name}</span>
          <span className="agent-group-count" aria-hidden="true">
            {count}
          </span>
          <span className="sr-only">{countText}</span>
          {holdsActive && <span className="sr-only">{activeText}</span>}
          {hiddenWaiting > 0 && (
            <span className="group-waiting agent-group-waiting">
              <span className="dot attention" aria-hidden="true" />
              <span className="sr-only">{waitingText}</span>
            </span>
          )}
        </button>
      </h3>
      <ul id={bodyId} className="agent-rows" hidden={!hasRows}>
        {children}
      </ul>
    </li>
  );
}

/** "1 waiting" above or below the session list while such a session is out of view. */
function WaitingEdge({
  edge,
  hint,
  onReveal,
}: {
  edge: "above" | "below";
  hint: EdgeHint | null;
  onReveal: (id: number) => void;
}) {
  const t = useT();
  if (hint === null) return null;
  const text = t(edge === "above" ? "sidebar.waitingAbove" : "sidebar.waitingBelow", {
    count: hint.count,
  });
  return (
    <button
      type="button"
      className={`waiting-edge ${edge}`}
      title={t("sidebar.waitingReveal")}
      onClick={() => {
        onReveal(hint.targetId);
      }}
    >
      <span className="dot attention" aria-hidden="true" />
      <span>{text}</span>
      <ChevronIcon size={12} />
    </button>
  );
}

function reducedMotion(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/**
 * The answers to a manual check (checking, up to date, failed). An available update shows only
 * as the button in the header.
 */
function UpdateNotice({ state, onDismiss }: { state: UpdateState; onDismiss: () => void }) {
  const t = useT();

  switch (state.kind) {
    case "checking":
      return state.manual ? <div className="notice">{t("update.checking")}</div> : null;
    case "current":
      return (
        <div className="notice">
          <span>
            {t("update.current", { version: displayVersion(state.version, t("version.beta")) })}
          </span>
          <button type="button" className="link-button" onClick={onDismiss}>
            {t("common.ok")}
          </button>
        </div>
      );
    case "error":
      return (
        <div className="notice error-notice" role="alert">
          <span>{state.message}</span>
          <button type="button" className="link-button" onClick={onDismiss}>
            {t("common.ok")}
          </button>
        </div>
      );
    default:
      return null;
  }
}

/**
 * "Update" at the top right of the sidebar, nothing else: one click installs. While installing,
 * the same pill fills with the progress (a shimmer while the size is unknown) and is disabled.
 * The tooltip names the version and what happens to running sessions.
 */
function UpdatePillButton({
  state,
  running,
  restoresSessions,
  onInstall,
}: {
  state: UpdateState;
  running: number;
  restoresSessions: boolean;
  onInstall: () => void;
}) {
  const t = useT();
  const pill = updatePill(state);
  if (pill === null) return null;
  const hint = updatePillHint(pill, running, restoresSessions, t);
  const progress = pillProgress(pill);
  return (
    <button
      type="button"
      className={pill.installing ? "update-pill installing" : "update-pill"}
      title={hint}
      aria-label={`${t("update.pill")}: ${hint}`}
      disabled={pill.installing}
      onClick={onInstall}
    >
      {pill.installing && (
        <span
          className={progress === null ? "update-pill-fill unknown" : "update-pill-fill"}
          style={progress === null ? undefined : { width: `${String(progress)}%` }}
          aria-hidden="true"
        />
      )}
      <span className="update-pill-label">{t("update.pill")}</span>
    </button>
  );
}

/** Second status line as one text for tooltips: folder, branch and run time. */
function detailText(line: DetailLine, t: ReturnType<typeof useT>): string {
  return [
    line.folder,
    line.branch === null ? "" : t("session.branch", { branch: revealInvisible(line.branch) }),
    line.runtime === null ? "" : t("session.runtime", { duration: line.runtime }),
  ]
    .filter(Boolean)
    .join(" · ");
}

/** Line "PowerShell 7 · 7.6.9" for tooltips; nothing without a known version. */
function programLine(profile: Profile | undefined, versions: ProgramVersions): string {
  const version = profile ? versions.get(profile.id) : undefined;
  return profile && version ? labelWithVersion(profile.name, version) : "";
}

interface ItemProps extends Props {
  session: SessionView;
  /** Agent group of the row (agentGroupKey), for dragging within it. */
  groupKey: string;
  /** Last row of its agent group: the tree's guide line ends at it. */
  last: boolean;
  lines: { status: StatusLine; detail: DetailLine };
  /** Folder-first naming of a Sessions row (sessionLabel.ts); `null` keeps title and folder. */
  label: SessionLabel | null;
  /** Program and version for the tooltip, empty without a known version. */
  program: string;
  active: boolean;
  state: AttentionState;
  chip: ChipKind | null;
  editing: boolean;
  /** "dragging", "drop-before", "drop-after" or empty. */
  dragClass: string;
  moveHint: string;
  /** Where the session is shown in a split layout; `null` in single view or when hidden. */
  placement: PanePlacement | null;
  onRowKey: (event: React.KeyboardEvent<HTMLElement>, id: number) => void;
}

function SessionItem({
  session,
  groupKey,
  last,
  lines,
  label,
  program,
  active,
  state,
  chip,
  editing,
  dragClass,
  moveHint,
  placement,
  onRowKey,
  onDragStart,
  onActivate,
  onRestart,
  onKill,
  onClose,
  onRename,
  onRenameStart,
  onRenameEnd,
  onSessionMenu,
  overlaps,
  filesOpen,
  onToggleFiles,
  language,
}: ItemProps) {
  const t = useT();
  const exited = session.status === "exited";
  const waiting = state === "attention";
  const line = lines.status;
  const status = line.full;
  const where = lines.detail;
  const name = label?.primary ?? session.title;
  const place = detailPlace(label, where.folder);
  const detail = exitDetail(session, t);
  const overlap = overlaps.get(session.id);
  const gateOff = gateHint(session.gate, t);
  const className = [
    "session",
    active ? "active" : "",
    exited ? "exited" : "",
    waiting ? "attention" : "",
    lookClass(rowLook(session, state, chip)),
    edgeClass(rowEdge(session, state, chip)),
    last ? "tree-last" : "",
    dragClass,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <li className={className} data-session-row={session.id} data-session-group={groupKey}>
      {/* Guide line and elbow from the agent group's header, drawn like the sub-agent tree's. */}
      <span className="tree-guide" aria-hidden="true" />
      {editing ? (
        <div className="session-main">
          <input
            className="rename"
            aria-label={t("session.newName")}
            defaultValue={session.title}
            autoFocus
            onFocus={(event) => {
              event.currentTarget.select();
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
              if (event.key === "Escape") {
                // Without saving: reset the value so onBlur applies nothing.
                event.currentTarget.value = session.title;
                event.currentTarget.blur();
              }
            }}
            onBlur={(event) => {
              const title = event.currentTarget.value.trim();
              if (title && title !== session.title) onRename(session.id, title);
              onRenameEnd();
            }}
          />
        </div>
      ) : (
        <button
          type="button"
          className="session-main"
          title={[
            // The status line may be cut at narrow widths; the tooltip always says it in full.
            `${name} · ${status}`,
            overlap ?? "",
            gateOff ?? "",
            detail,
            session.worktree
              ? t("worktree.sessionHint", { branch: revealInvisible(session.worktree.branch) })
              : "",
            session.cwd,
            program,
            placement !== null ? placementText(t, placement) : "",
            t("session.renameHint"),
            moveHint,
          ]
            .filter(Boolean)
            .join("\n")}
          aria-current={active ? "true" : undefined}
          onClick={() => {
            onActivate(session.id);
          }}
          onDoubleClick={() => {
            onRenameStart(session.id);
          }}
          onPointerDown={(event) => {
            onDragStart(session.id, event);
          }}
          onKeyDown={(event) => {
            onRowKey(event, session.id);
          }}
          onContextMenu={(event) => {
            openSessionMenu(event, session.id, onSessionMenu);
          }}
        >
          <span className="session-text">
            <span className="session-title-row">
              <SessionMarker marker={session.marker} />
              <span className="session-title">{name}</span>
              {overlap !== undefined && (
                <span className="session-overlap">
                  <OverlapIcon size={12} />
                  <span className="sr-only">{overlap}</span>
                </span>
              )}
              {/* Rare and lasting (no endpoint, unusable data path): visible, but quiet. */}
              {gateOff !== null && (
                <span className="session-gate-off">
                  <AlertIcon size={12} />
                  <span className="sr-only">{gateOff}</span>
                </span>
              )}
              {/* Right-aligned at the end of the title line, so the two lines below keep the
                  full width; the title ellipsizes first. */}
              <span className="session-title-end">
                {session.worktree && <RepoTeammates sessionId={session.id} />}
                {placement !== null && <LayoutGlyph placement={placement} />}
                {chip !== null && <StatusChip kind={chip} />}
              </span>
            </span>
            {/* The program leads its status ("Claude Code is working"); the state's shape sits in
                the chip above, so the icon replaces the former status dot. */}
            <span className="session-meta">
              <span className="state-dot" aria-hidden="true" />
              {/* Only the sentence ellipsizes; the duration after it stays whole. */}
              <span className={waiting ? "session-status attention-text" : "session-status"}>
                {line.sentence}
              </span>
              {line.duration !== null && (
                <span className="session-status-duration">
                  <span aria-hidden="true">·</span>
                  {line.duration}
                </span>
              )}
            </span>
            <span className="session-detail">
              {place !== null && <span className="session-path">{place}</span>}
              {session.worktree ? (
                <span className="session-branch worktree">
                  <span aria-hidden="true">·</span>
                  <WorktreeIcon size={11} />
                  <span className="sr-only">
                    {t("worktree.sessionHint", {
                      branch: revealInvisible(where.branch ?? session.worktree.branch),
                    })}
                  </span>
                  {/* The icon already says "own worktree"; the "hangar/" prefix would only repeat it. */}
                  <span className="session-branch-name" aria-hidden="true">
                    <Revealed text={shortBranch(where.branch ?? session.worktree.branch)} />
                  </span>
                </span>
              ) : (
                where.branch !== null && (
                  <span className="session-branch">
                    <span aria-hidden="true">·</span>
                    <BranchIcon size={11} />
                    <span className="sr-only">
                      {t("session.branch", { branch: revealInvisible(where.branch) })}
                    </span>
                    <span className="session-branch-name" aria-hidden="true">
                      <Revealed text={where.branch} />
                    </span>
                  </span>
                )
              )}
              {where.runtime !== null && (
                <span className="session-runtime">
                  <span aria-hidden="true">·</span>
                  <span className="sr-only">
                    {t("session.runtime", { duration: where.runtime })}
                  </span>
                  <span aria-hidden="true">{where.runtime}</span>
                </span>
              )}
              <ContextMeter sessionId={session.id} language={language} />
            </span>
          </span>
        </button>
      )}
      <div className="session-actions">
        {active && !editing && (
          <button
            type="button"
            className="icon-button session-files"
            aria-label={t("files.show")}
            aria-pressed={filesOpen}
            title={t("files.show")}
            onClick={onToggleFiles}
          >
            <FolderIcon />
          </button>
        )}
        {!editing && (
          <IconButton
            label={t("session.rename")}
            onClick={() => {
              onRenameStart(session.id);
            }}
          >
            <EditIcon />
          </IconButton>
        )}
        {exited ? (
          <IconButton
            label={t("session.restart")}
            onClick={() => {
              onRestart(session.id);
            }}
          >
            <RestartIcon />
          </IconButton>
        ) : (
          <IconButton
            label={t("session.kill")}
            onClick={() => {
              onKill(session.id);
            }}
          >
            <StopIcon />
          </IconButton>
        )}
        <IconButton
          label={exited ? t("session.close") : t("session.killAndClose")}
          onClick={() => {
            onClose(session.id);
          }}
        >
          <CloseIcon />
        </IconButton>
      </div>
    </li>
  );
}

/**
 * Every chip but an ordinary end trades the rail's dot for its shape, so working, pause and
 * waiting differ there by shape as well; an ended session keeps its quiet dot.
 */
function railShowsChip(chip: ChipKind | null): chip is Exclude<ChipKind, "ended"> {
  return chip !== null && chip !== "ended";
}

/**
 * One quiet line below the target about worktrees: how to start in an own worktree, or why that
 * is not possible here. Nothing while the answer is pending, so the line does not flicker.
 */
export function WorktreeHint({ chance }: { chance: WorktreeChance }) {
  const t = useT();
  if (chance === "unknown") return null;
  return (
    <p className={chance === "yes" ? "worktree-hint" : "worktree-hint off"}>
      <WorktreeIcon size={12} />
      <span>
        {chance === "yes"
          ? t("start.worktreeHint", { shift: t("key.shift") })
          : t("start.worktreeNoRepo")}
      </span>
    </p>
  );
}

/**
 * Session menu at the mouse; via the context menu key (no mouse position) below the row.
 */
function openSessionMenu(
  event: React.MouseEvent<HTMLElement>,
  id: number,
  open: (id: number, x: number, y: number) => void,
) {
  event.preventDefault();
  if (event.button === 2 || event.clientX !== 0 || event.clientY !== 0) {
    open(id, event.clientX, event.clientY);
    return;
  }
  const rect = event.currentTarget.getBoundingClientRect();
  open(id, rect.left + 24, rect.bottom);
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className="icon-button"
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
