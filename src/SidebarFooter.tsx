import { useEffect } from "react";

import { displayName, isSignedIn } from "./account";
import { Avatar } from "./Avatar";
import { useT } from "./i18n";
import {
  FilesIcon,
  GearIcon,
  MoonIcon,
  NotePencilIcon,
  SourceControlIcon,
  SunIcon,
  TeamIcon,
} from "./icons";
import { SIDE_TAB_LABELS, type SideTab } from "./layout";
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
import type { Theme } from "./theme";
import { useAccount } from "./useAccount";
import { loadTeamStatuses, useTeam } from "./useTeam";

/**
 * The sidebar's last row: who is signed in (a plain button: signed out it opens the Team window to
 * sign in, signed in Settings, Account), the team pill (who is online and whether something needs
 * the person; opens the Team window), the right panel's tabs, the light and dark switch and
 * Settings. The rail shows the same as icons.
 */
export function SidebarFooter(props: {
  collapsed: boolean;
  teamOpen: boolean;
  onToggleTeam: () => void;
  /** Settings on the Account section: where the signed-in person signs out. */
  onOpenAccount: () => void;
  /** The right panel's tab on screen in this window, `null` when it is closed or detached. */
  sideTab: SideTab | null;
  /** Opens the right panel on this tab (or brings the detached panel to it); shown again: closes. */
  onToggleSideTab: (tab: SideTab) => void;
  onOpenSettings: () => void;
  /** The color mode on screen; the switch flips it (System becomes a fixed choice). */
  theme: Theme;
  onToggleTheme: () => void;
}) {
  const t = useT();
  const { view, projects } = useAccount();
  const teamStore = useTeam();

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

  const onAccountClick = () => {
    if (signedIn) props.onOpenAccount();
    else if (!props.teamOpen) props.onToggleTeam();
  };

  const sideTabs = SIDE_TABS.map(({ tab, Icon }) => {
    const label = t(SIDE_TAB_LABELS[tab]);
    return (
      <button
        key={tab}
        type="button"
        className="icon-button footer-side-tab"
        aria-label={label}
        aria-pressed={props.sideTab === tab}
        title={
          tab === "notes"
            ? `${label}
${t("notes.quickNoteHint", { keys: formatShortcut(SHORTCUT_LABELS.quickNote, t) })}`
            : label
        }
        onClick={() => {
          props.onToggleSideTab(tab);
        }}
      >
        <Icon />
      </button>
    );
  });

  const themeLabel = props.theme === "dark" ? t("sidebar.toLight") : t("sidebar.toDark");
  const themeSwitch = (
    <button
      type="button"
      className="icon-button footer-theme"
      aria-label={themeLabel}
      title={themeLabel}
      onClick={props.onToggleTheme}
    >
      {props.theme === "dark" ? <SunIcon /> : <MoonIcon />}
    </button>
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
          type="button"
          className="rail-account"
          aria-label={accountLabel}
          title={accountTitle}
          onClick={onAccountClick}
        >
          <Avatar name="" text={meInitials(user)} size="small" />
        </button>
        {sideTabs}
        {themeSwitch}
        {settings}
      </div>
    );
  }

  const shown = pill.online.slice(0, PILL_AVATARS);
  return (
    <div className="sidebar-footer">
      <button
        type="button"
        className="footer-account"
        aria-label={accountLabel}
        title={accountTitle}
        onClick={onAccountClick}
      >
        <Avatar name="" text={meInitials(user)} />
        <span className="footer-name">{signedIn ? name : t("account.signIn")}</span>
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
      <span className="footer-side-tabs">{sideTabs}</span>
      {themeSwitch}
      {settings}
    </div>
  );
}

/** The right panel's tabs in its own order, each with its icon. */
const SIDE_TABS = [
  { tab: "notes", Icon: NotePencilIcon },
  { tab: "changes", Icon: SourceControlIcon },
  { tab: "files", Icon: FilesIcon },
] as const satisfies readonly {
  tab: SideTab;
  Icon: (props: { size?: number }) => React.ReactNode;
}[];
