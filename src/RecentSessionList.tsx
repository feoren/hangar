// The rows of the sidebar's "Recently opened" group (recentlyOpened.ts): one click starts a
// program in a folder again, resuming its conversation where one is known.

import { useLayoutEffect, useRef } from "react";

import { Badge } from "./Badge";
import { folderName } from "./folders";
import { shortAge } from "./format";
import type { Profile } from "./api";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { modelBadge, modelNote } from "./localLlm";
import type { BadgeLook } from "./programPrefs";
import { UNKNOWN_TIME, type RecentSession } from "./recentlyOpened";

/** What the second line names: the session's own title, or the program where it adds nothing. */
export function recentDetail(entry: RecentSession, programName: string): string {
  const title = entry.title.trim();
  return title === "" || title === folderName(entry.cwd) ? programName : title;
}

/**
 * One row per entry, two lines: the program's icon and the folder, below it the session title (or
 * the program) and how long ago it was used. The whole row reopens; the × at its end, shown on
 * hover and focus, takes the entry off the list. Nothing while the list is empty.
 */
export function RecentSessionList({
  entries,
  badgeFor,
  programName,
  profileFor,
  language,
  now,
  onReopen,
  onRemove,
}: {
  entries: readonly RecentSession[];
  badgeFor: (profileId: string) => BadgeLook;
  programName: (profileId: string) => string;
  /** The program, for the label of a vendor model; missing for a program no longer listed. */
  profileFor?: (profileId: string) => Pick<Profile, "models"> | undefined;
  /** UI language, for the relative time. */
  language: string;
  /** Milliseconds since the Unix epoch, ticking (`useClock`). */
  now: number;
  onReopen: (entry: RecentSession) => void;
  onRemove: (entry: RecentSession) => void;
}) {
  const t = useT();
  const list = useRef<HTMLUListElement>(null);
  /** Row whose × was pressed: the focus goes to the row now in its place. */
  const refocus = useRef<number | null>(null);
  useLayoutEffect(() => {
    const index = refocus.current;
    if (index === null) return;
    refocus.current = null;
    const rows = list.current?.querySelectorAll<HTMLButtonElement>("button.recent-open");
    if (rows === undefined || rows.length === 0) return;
    rows[Math.min(index, rows.length - 1)]?.focus();
  });
  if (entries.length === 0) return null;
  return (
    <ul ref={list} className="profile-list recent-list">
      {entries.map((entry, index) => {
        const folder = folderName(entry.cwd);
        const name = programName(entry.profileId);
        const detail = recentDetail(entry, name);
        const profile = profileFor?.(entry.profileId);
        const model = modelBadge(entry.model, profile);
        const modelTitle = modelNote(entry.model, profile, t);
        const age =
          entry.closedAt === UNKNOWN_TIME ? null : shortAge(entry.closedAt, now, language);
        return (
          <li
            key={`${String(entry.closedAt)}:${entry.profileId}:${entry.cwd}`}
            className="profile-row recent-row"
          >
            <button
              type="button"
              className="profile recent-open"
              title={[detail === name ? name : `${detail}\n${name}`, modelTitle, entry.cwd]
                .filter((line) => line !== null)
                .join("\n")}
              aria-label={
                modelTitle === null
                  ? t("recentlyOpened.open", { name: detail, folder })
                  : `${t("recentlyOpened.open", { name: detail, folder })}, ${modelTitle}`
              }
              onClick={() => {
                onReopen(entry);
              }}
            >
              <Badge look={badgeFor(entry.profileId)} />
              <span className="recent-text">
                <span className="recent-head">
                  <span className="recent-folder">{folder}</span>
                  {model !== null && (
                    <span className="recent-model" aria-hidden="true">
                      {model}
                    </span>
                  )}
                </span>
                <span className="recent-detail">
                  <span className="recent-name">{detail}</span>
                  {age !== null && (
                    <span className="recent-age">
                      <span aria-hidden="true"> · </span>
                      {age}
                    </span>
                  )}
                </span>
              </span>
            </button>
            <button
              type="button"
              className="icon-button recent-remove"
              aria-label={t("recentlyOpened.remove", { folder })}
              title={t("recentlyOpened.remove", { folder })}
              onClick={() => {
                refocus.current = index;
                onRemove(entry);
              }}
            >
              <CloseIcon size={12} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
