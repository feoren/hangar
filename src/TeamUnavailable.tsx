import type { Profile } from "./api";
import { useT } from "./i18n";
import { teamBlockHint, unavailableForTeam } from "./startMode";

/**
 * Below the Team list of "New session": one quiet, collapsed line naming how many installed
 * agents could be team sessions after a fix on this machine (version, administrator settings,
 * fixed folder), each with its hint once opened. The list itself keeps only what works; agents
 * without a recipe or measurement are not mentioned. Nothing when there is nothing to fix.
 */
export function TeamUnavailable(props: { profiles: readonly Profile[] }) {
  const t = useT();
  const unavailable = unavailableForTeam(props.profiles);
  if (unavailable.length === 0) return null;
  return (
    <details className="team-unavailable">
      <summary>
        {unavailable.length === 1
          ? t("start.unavailableOne")
          : t("start.unavailable", { count: unavailable.length })}
      </summary>
      <ul>
        {unavailable.map(({ profile, block }) => (
          <li key={profile.id}>
            <span className="team-unavailable-name">{profile.name}</span>
            <span className="team-unavailable-hint">{teamBlockHint(block, t)}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}
