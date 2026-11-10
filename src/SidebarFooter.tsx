import { useEffect } from "react";

import { displayName, isSignedIn } from "./account";
import { Avatar } from "./Avatar";
import { useT } from "./i18n";
import { GearIcon, TeamIcon } from "./icons";
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
import { useAccount } from "./useAccount";
import { loadTeamStatuses, useTeam } from "./useTeam";

/**
 * The sidebar's last row: who is signed in (opens Settings, Account; without an account it signs
 * in through the Team window), the team pill (who is online and whether something needs the
 * person; opens the Team window) and Settings. The rail shows the same as three icons.
 */
export function SidebarFooter(props: {
  collapsed: boolean;
  teamOpen: boolean;
  onToggleTeam: () => void;
  onOpenAccount: () => void;
  onOpenSettings: () => void;
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
  const onAccount = signedIn ? props.onOpenAccount : props.onToggleTeam;
  // Without an account the button opens the Team window: it toggles it like the pill.
  const accountToggle = signedIn ? undefined : true;

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
          aria-expanded={signedIn ? undefined : props.teamOpen}
          title={accountTitle}
          data-team-toggle={accountToggle}
          onClick={onAccount}
        >
          <Avatar name="" text={meInitials(user)} size="small" />
        </button>
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
        aria-expanded={signedIn ? undefined : props.teamOpen}
        title={accountTitle}
        data-team-toggle={accountToggle}
        onClick={onAccount}
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
      {settings}
    </div>
  );
}
