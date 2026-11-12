import { useMemo } from "react";

import { Avatar } from "./Avatar";
import { useT } from "./i18n";
import { peersOf, repoTeammates, teammateNames, teammateStack } from "./repoPeers";
import { useTeam } from "./useTeam";

const MAX_AVATARS = 2;

/**
 * Small avatars on the title line of a worktree row: the teammates working in the same
 * repository. Nothing without a team or without anyone there. The names are in the tooltip and,
 * for screen readers, in words.
 */
export function RepoTeammates({ sessionId }: { sessionId: number }) {
  const t = useT();
  const { activities } = useTeam();
  const people = useMemo(
    () => repoTeammates(peersOf(activities, sessionId)),
    [activities, sessionId],
  );
  if (people.length === 0) return null;
  const { shown, more } = teammateStack(people, MAX_AVATARS);
  const text = t("worktree.teammates", { names: teammateNames(people) });
  return (
    <span className="session-teammates" title={text}>
      {shown.map((person) => (
        <Avatar key={person.member} name={person.name} size="mini" />
      ))}
      {more > 0 && (
        <span className="session-teammates-more" aria-hidden="true">
          +{more}
        </span>
      )}
      <span className="sr-only">{text}</span>
    </span>
  );
}
