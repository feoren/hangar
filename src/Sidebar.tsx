import { useEffect, useId, useRef, useState } from "react";

import type { NoteFolder, Profile } from "./api";
import { AppMenu, type MenuGroup } from "./AppMenu";
import type { AttentionState } from "./attention";
import { Badge } from "./Badge";
import { FolderMenu } from "./FolderMenu";
import type { FolderPrefs } from "./folders";
import { exitDetail, shortPath, statusText } from "./format";
import { useT } from "./i18n";
import {
  ChevronIcon,
  CloseIcon,
  CollapseIcon,
  DownloadIcon,
  EditIcon,
  ExpandIcon,
  FolderIcon,
  HangarLogo,
  MoonIcon,
  NotesIcon,
  PlusIcon,
  CheckIcon,
  CopyIcon,
  RestartIcon,
  StopIcon,
  SunIcon,
} from "./icons";
import { clampFooterHeight, FOOTER_HEIGHT, SIDEBAR_WIDTH, type SidebarGroup } from "./layout";
import { folderLabel, sameFolder, type NoteFolderInfo } from "./notes";
import type { BadgeLook } from "./programPrefs";
import type { SessionView } from "./sessions";
import { labelWithVersion, type ProgramVersions } from "./programs";
import { availableVersion, type ProgramUpdates } from "./programUpdates";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import { Splitter } from "./Splitter";
import type { Theme } from "./theme";
import { SessionActionGuard } from "./timing";
import type { UpdateState } from "./useUpdater";
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
  notesOpen: boolean;
  onToggleNotes: () => void;
  /** Scopes that contain notes (list_note_folders). */
  noteFolders: readonly NoteFolderInfo[];
  /** Scope shown in the notes panel, `null` when the panel is closed. */
  notesFolder: NoteFolder | null;
  onOpenNotes: (folder: NoteFolder) => void;
  /** Launchable entries in the chosen order, hidden ones left out. */
  startable: Profile[];
  /** Installed entries exist, but all of them are hidden. */
  allHidden: boolean;
  /** Badge appearance per profile ID (user choice, program icon or letters). */
  badgeFor: (profileId: string) => BadgeLook;
  closedGroups: readonly SidebarGroup[];
  onToggleGroup: (group: SidebarGroup) => void;
  /** After a mouse click on a header button: focus goes back to the terminal. */
  onFocusTerminal: () => void;
  /** Hangar's own version (as getVersion returns it), `null` until known. */
  version: string | null;
  /** Currently visible appearance (the resolved one for "System"). */
  theme: Theme;
  onToggleTheme: () => void;
  cwd: string | null;
  collapsed: boolean;
  width: number;
  footerHeight: number;
  menu: MenuGroup[];
  update: UpdateState;
  attention: ReadonlyMap<number, AttentionState>;
  /** Session whose name is being edited in the sidebar right now. */
  renamingId: number | null;
  onRenameStart: (id: number) => void;
  onRenameEnd: () => void;
  onWidth: (width: number) => void;
  onFooterHeight: (height: number) => void;
  onToggleCollapsed: () => void;
  onMenuClose: () => void;
  onActivate: (id: number) => void;
  onStart: (profile: Profile) => void;
  /** Profile IDs with a running session (hint about locked files during an update). */
  runningPrograms: ReadonlySet<string>;
  /** Runs a program's update as a new session (only with `canRun`). */
  onRunUpdate: (profileId: string) => void;
  /** Opens the folder dialog for the working folder of new sessions. */
  onPickCwd: () => void;
  /** Favorite and recent folders for the "Starts in" menu. */
  folders: FolderPrefs;
  onSelectCwd: (path: string) => void;
  onToggleFavorite: (path: string) => void;
  onRestart: (id: number) => void;
  onKill: (id: number) => void;
  onClose: (id: number) => void;
  onRename: (id: number, title: string) => void;
  onInstallUpdate: () => void;
  onDismissUpdate: () => void;
}

export function Sidebar(props: Props) {
  const { sessions, activeId, profiles, versions, badgeFor, cwd, collapsed, update, attention } =
    props;
  const { startable } = props;
  const t = useT();
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

  useEffect(() => {
    if (!refocusToggle.current) return;
    refocusToggle.current = false;
    toggleButton.current?.focus();
  }, [collapsed]);

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

  if (collapsed) {
    const programItems: PopupItem[] = startable.map((profile) => {
      const version = versions.get(profile.id);
      return {
        type: "action",
        key: profile.id,
        label: profile.name,
        ...(version === undefined ? {} : { detail: version }),
        icon: <Badge look={badgeFor(profile.id)} />,
        run: () => {
          props.onStart(profile);
        },
      };
    });
    const newKeys = formatShortcut(SHORTCUT_LABELS.newSession, t);
    return (
      <aside className="sidebar collapsed" ref={root}>
        {/* Same order as in the expanded header: menu, logo, notes, theme, collapse. */}
        <div className="rail-top">
          <AppMenu groups={props.menu} onClose={props.onMenuClose} />
          <span className="rail-logo" title={`Hangar ${appVersion}`.trim()}>
            <HangarLogo size={20} />
          </span>
          <NotesButton open={props.notesOpen} onToggle={props.onToggleNotes} />
          <ThemeButton
            theme={props.theme}
            onToggle={props.onToggleTheme}
            onFocusTerminal={props.onFocusTerminal}
          />
          <button
            ref={toggleButton}
            type="button"
            className="icon-button"
            aria-label={t("sidebar.expand")}
            title={`${t("sidebar.expand")} (${toggleKeys})`}
            onClick={onToggleClick}
          >
            <ExpandIcon />
          </button>
        </div>
        <ul className="rail-sessions">
          {sessions.map((session) => {
            const state = attention.get(session.id) ?? "idle";
            const status = statusText(session, t, state);
            const detail = exitDetail(session, t);
            const className = [
              "rail-session",
              session.id === activeId ? "active" : "",
              state === "attention" ? "attention" : "",
            ];
            return (
              <li key={session.id}>
                <button
                  type="button"
                  className={className.filter(Boolean).join(" ")}
                  title={[
                    `${session.title} · ${status} · ${session.cwd}`,
                    detail,
                    programLine(profileById.get(session.profileId), versions),
                  ]
                    .filter(Boolean)
                    .join("\n")}
                  aria-label={`${session.title}, ${status}`}
                  aria-current={session.id === activeId ? "true" : undefined}
                  onClick={() => {
                    props.onActivate(session.id);
                  }}
                >
                  <Badge look={badgeFor(session.profileId)} />
                  <span
                    className={`rail-dot dot ${state === "attention" ? "attention" : session.status}`}
                  />
                </button>
              </li>
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
              props.onMenuClose();
            }}
          />
        )}
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
      </aside>
    );
  }

  const sessionsOpen = open("sessions");
  const notesGroupOpen = open("notes");
  const newSessionOpen = open("newSession");
  const withNotes = props.noteFolders.filter((folder) => folder.noteCount > 0);
  const sessionsTitle = sessionsOpen
    ? t("sidebar.sessions")
    : t("sidebar.sessionsCount", { count: sessions.length });
  const waitingText = t("sidebar.waitingCount", { count: waitingCount });

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
        <NotesButton open={props.notesOpen} onToggle={props.onToggleNotes} />
        <ThemeButton
          theme={props.theme}
          onToggle={props.onToggleTheme}
          onFocusTerminal={props.onFocusTerminal}
        />
        <button
          ref={toggleButton}
          type="button"
          className="icon-button"
          aria-label={t("sidebar.collapse")}
          title={`${t("sidebar.collapse")} (${toggleKeys})`}
          onClick={onToggleClick}
        >
          <CollapseIcon />
        </button>
      </div>

      <GroupSection
        group="sessions"
        title={sessionsTitle}
        open={sessionsOpen}
        className={sessionsOpen ? "sessions-group fill" : "sessions-group"}
        onToggle={props.onToggleGroup}
        marker={
          !sessionsOpen && waitingCount > 0 ? (
            <span className="group-waiting" title={waitingText}>
              <span className="dot attention" aria-hidden="true" />
              <span className="sr-only">{waitingText}</span>
            </span>
          ) : null
        }
      >
        <ul className="session-list">
          {sessions.length === 0 && <li className="hint">{t("sidebar.noSessions")}</li>}
          {sessions.map((session) => (
            <SessionItem
              key={session.id}
              session={session}
              look={badgeFor(session.profileId)}
              program={programLine(profileById.get(session.profileId), versions)}
              active={session.id === activeId}
              state={attention.get(session.id) ?? "idle"}
              editing={props.renamingId === session.id}
              {...props}
              onClose={onClose}
              onKill={onKill}
              onRestart={onRestart}
              onRenameStart={onRename}
            />
          ))}
        </ul>
      </GroupSection>

      <UpdateNotice
        state={update}
        running={sessions.filter((s) => s.status !== "exited").length}
        onInstall={props.onInstallUpdate}
        onDismiss={props.onDismissUpdate}
      />

      <GroupSection
        group="notes"
        title={
          notesGroupOpen ? t("sidebar.notes") : t("sidebar.notesCount", { count: withNotes.length })
        }
        open={notesGroupOpen}
        className="notes-group"
        onToggle={props.onToggleGroup}
      >
        <ul className="note-folder-list">
          {withNotes.length === 0 && <li className="hint">{t("sidebar.noNotes")}</li>}
          {withNotes.map((info) => {
            const folder: NoteFolder = { path: info.path };
            const label = folderLabel(folder, t("notes.general"));
            const current = props.notesFolder !== null && sameFolder(props.notesFolder, folder);
            return (
              <li key={info.folderKey}>
                <button
                  type="button"
                  className={current ? "note-folder current" : "note-folder"}
                  title={info.path ?? t("notes.general")}
                  aria-current={current ? "true" : undefined}
                  aria-label={t("sidebar.noteFolder", { folder: label, count: info.noteCount })}
                  onClick={() => {
                    props.onOpenNotes(folder);
                  }}
                >
                  <NotesIcon size={14} />
                  <span className="note-folder-name">{label}</span>
                  <span className="note-folder-count" aria-hidden="true">
                    {info.noteCount}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </GroupSection>

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
        <button
          ref={cwdButton}
          type="button"
          className="cwd"
          title={cwd ?? t("sidebar.pickCwdTitle")}
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
          <FolderIcon />
          <span className="cwd-text">
            <span className="cwd-label">{t("sidebar.startsIn")}</span>
            <span className="cwd-path">{cwd ? shortPath(cwd) : t("sidebar.pickCwd")}</span>
          </span>
        </button>
        {folderMenu !== null && (
          <FolderMenu
            cwd={cwd}
            folders={props.folders}
            position={folderMenu}
            flipAbove={folderMenu.flipAbove}
            anchor={cwdButton}
            onSelect={props.onSelectCwd}
            onChoose={props.onPickCwd}
            onToggleFavorite={props.onToggleFavorite}
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
            const latest = availableVersion(props.updates, profile.id);
            const programUpdate = props.updates.get(profile.id);
            const hint = programUpdate?.updateHint ?? null;
            return (
              <li key={profile.id} className="profile-row">
                <button
                  type="button"
                  className="profile"
                  title={[profile.command, ...profile.args].join(" ")}
                  onClick={() => {
                    props.onStart(profile);
                  }}
                >
                  <Badge look={badgeFor(profile.id)} />
                  <span className="profile-name">{profile.name}</span>
                  {version && (
                    <span className="profile-version">
                      <span aria-hidden="true">·</span> {version}
                    </span>
                  )}
                  {latest !== null && hint === null && (
                    <span
                      className="profile-update"
                      title={t("update.programAvailable", { version: latest })}
                    >
                      <span className="dot update" aria-hidden="true" />
                      <span aria-hidden="true">→ {latest}</span>
                      <span className="sr-only">
                        {t("update.programAvailable", { version: latest })}
                      </span>
                    </span>
                  )}
                  <PlusIcon />
                </button>
                {latest !== null && hint !== null && (
                  <UpdateButtons
                    latest={latest}
                    command={hint}
                    canRun={programUpdate?.canRun ?? false}
                    lockedHint={
                      props.runningPrograms.has(profile.id)
                        ? t("update.lockedHint", { name: profile.name })
                        : null
                    }
                    onRun={() => {
                      props.onRunUpdate(profile.id);
                    }}
                  />
                )}
              </li>
            );
          })}
        </ul>
      </GroupSection>

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

/** Show and hide the notes panel; state via aria-pressed. */
function NotesButton({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  const t = useT();
  const label = `${open ? t("notes.hide") : t("notes.show")} (${formatShortcut(SHORTCUT_LABELS.toggleNotes, t)})`;
  return (
    <button
      type="button"
      className={open ? "icon-button toggle on" : "icon-button toggle"}
      aria-pressed={open}
      aria-label={t("notes.title")}
      title={label}
      onClick={onToggle}
    >
      <NotesIcon />
    </button>
  );
}

/** Toggle light and dark per click. The label names the target, the icon shows it (sun means light). */
function ThemeButton({
  theme,
  onToggle,
  onFocusTerminal,
}: {
  theme: Theme;
  onToggle: () => void;
  onFocusTerminal: () => void;
}) {
  const t = useT();
  const label = theme === "dark" ? t("sidebar.toLight") : t("sidebar.toDark");
  return (
    <button
      type="button"
      className="icon-button"
      aria-label={label}
      title={label}
      onClick={(event) => {
        onToggle();
        // Mouse click: back to work. With the keyboard, focus stays here.
        if (event.detail > 0) onFocusTerminal();
      }}
    >
      {theme === "dark" ? <SunIcon /> : <MoonIcon />}
    </button>
  );
}

function UpdateNotice({
  state,
  running,
  onInstall,
  onDismiss,
}: {
  state: UpdateState;
  running: number;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const t = useT();

  switch (state.kind) {
    case "idle":
      return null;
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
    case "installing":
      return (
        <div className="notice accent">
          <span>
            {state.percent === null
              ? t("update.installing", {
                  version: displayVersion(state.update.version, t("version.beta")),
                })
              : t("update.installingProgress", {
                  version: displayVersion(state.update.version, t("version.beta")),
                  percent: state.percent,
                })}
          </span>
          <div className="progress" aria-hidden="true">
            <div className="progress-bar" style={{ width: `${String(state.percent ?? 15)}%` }} />
          </div>
        </div>
      );
    case "available":
      return (
        <div className="notice accent">
          <span className="notice-title">
            <DownloadIcon />{" "}
            {t("update.available", {
              version: displayVersion(state.update.version, t("version.beta")),
            })}
          </span>
          {state.update.body && <span className="notice-body">{state.update.body}</span>}
          {confirming && running > 0 ? (
            <>
              <span className="notice-body">
                {running === 1 ? t("update.endsOne") : t("update.endsMany", { count: running })}
              </span>
              <div className="notice-actions">
                <button type="button" className="primary-button" onClick={onInstall}>
                  {t("update.installNow")}
                </button>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setConfirming(false);
                  }}
                >
                  {t("common.cancel")}
                </button>
              </div>
            </>
          ) : (
            <div className="notice-actions">
              <button
                type="button"
                className="primary-button"
                onClick={() => {
                  if (running > 0) setConfirming(true);
                  else onInstall();
                }}
              >
                {t("update.install")}
              </button>
              <button type="button" className="link-button" onClick={onDismiss}>
                {t("update.later")}
              </button>
            </div>
          )}
        </div>
      );
  }
}

/**
 * Small "→ 1.2.3" button next to a program with an update. If Hangar can run the update
 * (`canRun`), it starts it as a new session, with a copy icon next to it as a fallback.
 * Otherwise it copies the command (often just the install command). If copying fails, the
 * command appears below in selectable form.
 */
function UpdateButtons({
  latest,
  command,
  canRun,
  lockedHint,
  onRun,
}: {
  latest: string;
  command: string;
  canRun: boolean;
  /** Hint that the program is running right now; `null` if not. */
  lockedHint: string | null;
  onRun: () => void;
}) {
  const t = useT();
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const label = t("update.copyCommand", { version: latest });
  const runLabel = t("update.runTo", { version: latest });
  const copy = () => {
    navigator.clipboard
      .writeText(command)
      .then(() => {
        setState("copied");
      })
      .catch(() => {
        setState("failed");
      });
  };
  useEffect(() => {
    if (state !== "copied") return;
    const timer = window.setTimeout(() => {
      setState("idle");
    }, 4000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [state]);
  return (
    <>
      {canRun ? (
        <>
          <button
            type="button"
            className="profile-update-button"
            aria-label={runLabel}
            title={[runLabel, t("update.runsCommand", { command }), lockedHint]
              .filter(Boolean)
              .join("\n")}
            onClick={onRun}
          >
            <span className="dot update" aria-hidden="true" />
            <span aria-hidden="true">→ {latest}</span>
          </button>
          <button
            type="button"
            className={
              state === "copied"
                ? "icon-button profile-update-copy copied"
                : "icon-button profile-update-copy"
            }
            aria-label={label}
            title={`${label}\n${command}`}
            onClick={copy}
          >
            {state === "copied" ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
          </button>
        </>
      ) : (
        <button
          type="button"
          className={state === "copied" ? "profile-update-button copied" : "profile-update-button"}
          aria-label={label}
          title={`${label}\n${command}`}
          onClick={copy}
        >
          {state === "copied" ? (
            <CheckIcon size={12} />
          ) : (
            <span className="dot update" aria-hidden="true" />
          )}
          <span aria-hidden="true">→ {latest}</span>
        </button>
      )}
      <span
        className={
          state === "failed"
            ? "profile-update-feedback error-text selectable"
            : "profile-update-feedback"
        }
        role="status"
      >
        {state === "copied"
          ? t("update.commandCopied")
          : state === "failed"
            ? t("settings.copyFailed", { command })
            : ""}
      </span>
    </>
  );
}

/** Line "PowerShell 7 · 7.6.9" for tooltips; nothing without a known version. */
function programLine(profile: Profile | undefined, versions: ProgramVersions): string {
  const version = profile ? versions.get(profile.id) : undefined;
  return profile && version ? labelWithVersion(profile.name, version) : "";
}

interface ItemProps extends Props {
  session: SessionView;
  look: BadgeLook;
  /** Program and version for the tooltip, empty without a known version. */
  program: string;
  active: boolean;
  state: AttentionState;
  editing: boolean;
}

function SessionItem({
  session,
  look,
  program,
  active,
  state,
  editing,
  onActivate,
  onRestart,
  onKill,
  onClose,
  onRename,
  onRenameStart,
  onRenameEnd,
}: ItemProps) {
  const t = useT();
  const exited = session.status === "exited";
  const waiting = state === "attention";
  const status = statusText(session, t, state);
  const detail = exitDetail(session, t);
  const className = [
    "session",
    active ? "active" : "",
    exited ? "exited" : "",
    waiting ? "attention" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <li className={className}>
      {editing ? (
        <div className="session-main">
          <Badge look={look} />
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
            waiting ? `${session.title} · ${status}` : "",
            detail,
            program,
            t("session.renameHint"),
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
        >
          <Badge look={look} />
          <span className="session-text">
            <span className="session-title">{session.title}</span>
            <span className="session-meta">
              <span className={`dot ${waiting ? "attention" : session.status}`} />
              <span className={waiting ? "session-status attention-text" : "session-status"}>
                {status}
              </span>
              <span aria-hidden="true">·</span>
              <span className="session-path">{shortPath(session.cwd)}</span>
            </span>
          </span>
        </button>
      )}
      <div className="session-actions">
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
