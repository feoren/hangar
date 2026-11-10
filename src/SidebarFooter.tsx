import { useEffect, useRef, useState } from "react";

import { displayName, isSignedIn } from "./account";
import { accountMenuEntries, type AccountMenuEntry } from "./accountMenu";
import { Avatar } from "./Avatar";
import { useT, type Translate } from "./i18n";
import {
  ChevronIcon,
  GearIcon,
  MonitorIcon,
  MoonIcon,
  NotesIcon,
  PersonIcon,
  SignOutIcon,
  SunIcon,
  TeamIcon,
} from "./icons";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import { personName, releaseRequests } from "./team";
import {
  meInitials,
  pillLabel,
  pillText,
  PILL_AVATARS,
  teamPill,
  type OnlinePerson,
} from "./teamOverview";
import type { ThemeSetting } from "./theme";
import { account, useAccount } from "./useAccount";
import { loadTeamStatuses, useTeam } from "./useTeam";

/**
 * The sidebar's last row: who is signed in (opens a small menu with the notes, the color mode
 * and signing in or out), the team pill (who is online and whether something needs the person;
 * opens the Team window) and Settings. The rail shows the same as three icons.
 */
export function SidebarFooter(props: {
  collapsed: boolean;
  teamOpen: boolean;
  onToggleTeam: () => void;
  /** Settings on the Account section: where a refused sign out says why. */
  onOpenAccount: () => void;
  notesOpen: boolean;
  onToggleNotes: () => void;
  onOpenSettings: () => void;
  themeSetting: ThemeSetting;
  onSetTheme: (theme: ThemeSetting) => void;
  /** A menu entry ran: focus goes back to the terminal unless the entry took it. */
  onMenuClose: () => void;
}) {
  const t = useT();
  const { view, projects } = useAccount();
  const teamStore = useTeam();
  const accountButton = useRef<HTMLButtonElement>(null);
  /** Account menu, open at this position (above the button when space below is short). */
  const [menu, setMenu] = useState<{ x: number; y: number; flipAbove: number } | null>(null);

  // The pill needs the projects' states even before the Team window was ever opened.
  useEffect(() => {
    if (projects !== null) loadTeamStatuses(projects.map((project) => project.id));
  }, [projects]);

  const signedIn = isSignedIn(view);
  const user = view?.state.kind === "signedIn" ? view.state.user : null;
  const name = user === null ? t("settings.account") : displayName(user);
  const requests = teamStore.pending.length + releaseRequests(teamStore.activities).length;
  const pill = teamPill(teamStore.statuses, teamStore.activities, requests);
  const nameOf = (person: OnlinePerson) => personName(person, t);
  const teamLabel = pillLabel(pill, t, nameOf);
  const settingsTitle = `${t("settings.title")} (${formatShortcut(SHORTCUT_LABELS.openSettings, t)})`;
  const accountLabel = signedIn ? t("sidebar.account", { name }) : t("account.signIn");
  const accountTitle = signedIn ? accountLabel : t("sidebar.signInHint");

  const items = accountMenuEntries(signedIn, props.themeSetting, props.notesOpen).map(
    (entry): PopupItem =>
      menuItem(entry, t, {
        notes: props.onToggleNotes,
        signIn: () => {
          if (!props.teamOpen) props.onToggleTeam();
        },
        signOut: () => {
          // A refused sign out shows its reason where signing out lives: Settings, Account.
          void account.signOut().then((problem) => {
            if (problem !== null) props.onOpenAccount();
          });
        },
        theme: props.onSetTheme,
      }),
  );

  const onAccountClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (menu !== null) {
      setMenu(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    setMenu(
      props.collapsed
        ? { x: rect.right + 6, y: rect.top, flipAbove: rect.bottom }
        : { x: rect.left, y: rect.bottom + 4, flipAbove: rect.top - 4 },
    );
  };

  const accountMenu = menu !== null && (
    <PopupMenu
      items={items}
      label={t("sidebar.accountMenu")}
      position={menu}
      flipAbove={menu.flipAbove}
      anchor={accountButton}
      className="account-menu"
      onClose={(reason) => {
        setMenu(null);
        // Escape returns to the button; an entry returns to the terminal.
        if (reason === "dismiss") accountButton.current?.focus();
        else props.onMenuClose();
      }}
    />
  );

  const settings = (
    <button
      type="button"
      className="icon-button footer-settings"
      aria-label={t("settings.title")}
      title={settingsTitle}
      onClick={props.onOpenSettings}
    >
      <GearIcon />
    </button>
  );

  if (props.collapsed) {
    return (
      <div className="rail-footer">
        {signedIn && (
          <button
            type="button"
            className={`icon-button rail-team tone-${pill.tone}`}
            aria-label={teamLabel}
            aria-expanded={props.teamOpen}
            title={teamLabel}
            data-team-toggle=""
            onClick={props.onToggleTeam}
          >
            <TeamIcon />
            <span className="team-mark" aria-hidden="true" />
          </button>
        )}
        <button
          ref={accountButton}
          type="button"
          className="rail-account"
          aria-label={accountLabel}
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          title={accountTitle}
          onClick={onAccountClick}
        >
          <Avatar name="" text={meInitials(user)} size="small" />
        </button>
        {settings}
        {accountMenu}
      </div>
    );
  }

  const shown = pill.online.slice(0, PILL_AVATARS);
  return (
    <div className="sidebar-footer">
      <button
        ref={accountButton}
        type="button"
        className="footer-account"
        aria-label={accountLabel}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        title={accountTitle}
        onClick={onAccountClick}
      >
        <Avatar name="" text={meInitials(user)} />
        <span className="footer-name">{signedIn ? name : t("account.signIn")}</span>
        <span className="footer-chevron" aria-hidden="true">
          <ChevronIcon size={12} />
        </span>
      </button>
      {signedIn && (
        <button
          type="button"
          className={`team-pill tone-${pill.tone}`}
          aria-label={teamLabel}
          aria-expanded={props.teamOpen}
          title={teamLabel}
          data-team-toggle=""
          onClick={props.onToggleTeam}
        >
          <span className="team-mark" aria-hidden="true" />
          {shown.length > 0 && (
            <span className="team-pill-avatars" aria-hidden="true">
              {shown.map((person) => (
                <Avatar key={person.member} name={person.name} size="mini" />
              ))}
            </span>
          )}
          <span className="team-pill-text">{pillText(pill, t)}</span>
        </button>
      )}
      {settings}
      {accountMenu}
    </div>
  );
}

const THEME_LABEL = {
  system: "settings.themeSystem",
  light: "settings.themeLight",
  dark: "settings.themeDark",
} as const;

/** One account menu entry as a popup menu row. */
function menuItem(
  entry: AccountMenuEntry,
  t: Translate,
  run: {
    notes: () => void;
    signIn: () => void;
    signOut: () => void;
    theme: (theme: ThemeSetting) => void;
  },
): PopupItem {
  switch (entry.kind) {
    case "notes":
      return {
        type: "action",
        key: "notes",
        label: entry.open ? t("notes.hide") : t("notes.show"),
        title: t("notes.quickNoteHint", { keys: formatShortcut(SHORTCUT_LABELS.quickNote, t) }),
        icon: <NotesIcon size={14} />,
        run: run.notes,
      };
    case "signIn":
      return {
        type: "action",
        key: "sign-in",
        label: t("account.signIn"),
        title: t("sidebar.signInHint"),
        icon: <PersonIcon size={14} />,
        run: run.signIn,
      };
    case "signOut":
      return {
        type: "action",
        key: "sign-out",
        label: t("account.signOut"),
        icon: <SignOutIcon size={14} />,
        run: run.signOut,
      };
    case "themeHeading":
      return { type: "heading", key: "theme-heading", label: t("settings.theme") };
    case "theme":
      return {
        type: "action",
        key: `theme-${entry.setting}`,
        label: t(THEME_LABEL[entry.setting]),
        current: entry.current,
        icon:
          entry.setting === "system" ? (
            <MonitorIcon size={14} />
          ) : entry.setting === "light" ? (
            <SunIcon size={14} />
          ) : (
            <MoonIcon size={14} />
          ),
        run: () => {
          run.theme(entry.setting);
        },
      };
    case "separator":
      return { type: "separator", key: entry.key };
  }
}
