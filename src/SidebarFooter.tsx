import { useEffect } from "react";

import { api } from "./api";

import { accountButtonText, isSignedIn } from "./account";
import { Avatar } from "./Avatar";
import { useT } from "./i18n";
import { CoffeeIcon, GearIcon, MoonIcon, SunIcon, TeamIcon } from "./icons";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import { personName, releaseRequests } from "./team";
import { meInitials, pillLabel, pillText, teamPill, type OnlinePerson } from "./teamOverview";
import type { Theme } from "./theme";
import { useAccount, useAccountAvatar } from "./useAccount";
import { loadTeamStatuses, useTeam } from "./useTeam";

/**
 * The sidebar's last row: who is signed in (a plain button: signed out it opens the Team window to
 * sign in, signed in Settings, Account; in a narrow sidebar only the avatar), the team pill (its
 * state mark and how many people are online, the rest in its tooltip; opens the Team window), the
 * light and dark switch, the support link and Settings. The rail shows the same as icons.
 */
export function SidebarFooter(props: {
  collapsed: boolean;
  teamOpen: boolean;
  onToggleTeam: () => void;
  /** Settings on the Account section: where the signed-in person signs out. */
  onOpenAccount: () => void;
  onOpenSettings: () => void;
  /** The color mode on screen; the switch flips it (System becomes a fixed choice). */
  theme: Theme;
  onToggleTheme: () => void;
}) {
  const t = useT();
  const { view, projects } = useAccount();
  const avatar = useAccountAvatar();
  const teamStore = useTeam();

  // The pill needs the projects' states even before the Team window was ever opened.
  useEffect(() => {
    if (projects !== null) loadTeamStatuses(projects.map((project) => project.id));
  }, [projects]);

  const signedIn = isSignedIn(view);
  const user = view?.state.kind === "signedIn" ? view.state.user : null;
  const requests = teamStore.pending.length + releaseRequests(teamStore.activities).length;
  const pill = teamPill(teamStore.statuses, teamStore.activities, requests);
  const nameOf = (person: OnlinePerson) => personName(person, t);
  const teamLabel = pillLabel(pill, t, nameOf);
  const settingsTitle = `${t("settings.title")} (${formatShortcut(SHORTCUT_LABELS.openSettings, t)})`;
  const account = accountButtonText(view, t);

  const onAccountClick = () => {
    if (signedIn) props.onOpenAccount();
    else if (!props.teamOpen) props.onToggleTeam();
  };

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

  const coffeeLabel = t("sidebar.coffee");
  const coffee = (
    <button
      type="button"
      className="icon-button footer-coffee"
      aria-label={coffeeLabel}
      title={coffeeLabel}
      onClick={() => {
        api.openWebUrl(SUPPORT_URL).catch((error: unknown) => {
          console.warn("could not open the support page", error);
        });
      }}
    >
      <CoffeeIcon />
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
          aria-label={account.label}
          title={account.title}
          onClick={onAccountClick}
        >
          <Avatar name="" text={meInitials(user)} src={avatar} size="small" />
        </button>
        {themeSwitch}
        {coffee}
        {settings}
      </div>
    );
  }

  return (
    <div className="sidebar-footer">
      <button
        type="button"
        className="footer-account"
        aria-label={account.label}
        title={account.title}
        onClick={onAccountClick}
      >
        <Avatar name="" text={meInitials(user)} src={avatar} />
        <span className="footer-name">{account.text}</span>
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
          <span className="team-pill-text" aria-hidden="true">
            {pillText(pill, t)}
          </span>
        </button>
      )}
      {themeSwitch}
      {coffee}
      {settings}
    </div>
  );
}

/** The project's Buy Me a Coffee page, opened in the default browser. */
export const SUPPORT_URL = "https://buymeacoffee.com/feoren";
