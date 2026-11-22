// "Recently closed" in the sidebar's New session area (recentlyClosed.ts): one click starts a
// closed session again.

import { useId } from "react";

import { Badge } from "./Badge";
import { folderName } from "./folders";
import { useT } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import type { ClosedSession } from "./recentlyClosed";

/**
 * "Recently closed" above the programs: one quiet row per session, program icon, folder and
 * title on one line, the full title and path in the tooltip. Nothing while the list is empty.
 */
export function ClosedSessionList({
  entries,
  badgeFor,
  onReopen,
}: {
  entries: readonly ClosedSession[];
  badgeFor: (profileId: string) => BadgeLook;
  onReopen: (entry: ClosedSession) => void;
}) {
  const t = useT();
  const headingId = useId();
  if (entries.length === 0) return null;
  return (
    <section className="recently-closed" aria-labelledby={headingId}>
      <h3 id={headingId} className="recently-closed-title">
        {t("recentlyClosed.title")}
      </h3>
      <ul className="profile-list">
        {entries.map((entry) => {
          const folder = folderName(entry.cwd);
          return (
            <li
              key={`${String(entry.closedAt)}:${entry.profileId}:${entry.cwd}`}
              className="profile-row"
            >
              <button
                type="button"
                className="profile closed-session"
                title={`${entry.title}\n${entry.cwd}`}
                aria-label={t("recentlyClosed.reopen", { title: entry.title, folder })}
                onClick={() => {
                  onReopen(entry);
                }}
              >
                <Badge look={badgeFor(entry.profileId)} />
                <span className="closed-session-text">
                  <span className="closed-session-folder">{folder}</span>
                  <span className="closed-session-title">{entry.title}</span>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
