import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { dayWindow, formatSpan, parseDailyReview, parseDayInput } from "./dailyReview";
import { Dialog } from "./Dialog";
import { useT, type Translate } from "./i18n";
import { CloseIcon } from "./icons";
import {
  COMMIT_COUNT,
  countText,
  FILE_COUNT,
  morningDue,
  morningSummary,
  PROJECT_COUNT,
  SESSION_COUNT,
  type MorningProject,
  type MorningSummary,
} from "./morningReview";

interface NoticeProps {
  language: string;
  /** The setting, and `false` while the stored preferences are not read yet. */
  enabled: boolean;
  /** The day whose summary was last offered, as the backend keeps it (`mainStore.ts`). */
  offeredDay: string | null;
  /** A day's summary was looked at; the caller stores the day. */
  onOffered: (day: string) => void;
  /** The answer of `daily_review` for `[start, end)`, with what is uncommitted right now. */
  load: (start: number, end: number) => Promise<unknown>;
  /** "More detailed review": the full daily review of that day. */
  onOpenReview: (day: Date) => void;
  /** The clock, for tests. */
  now?: () => Date;
}

const systemNow = () => new Date();

interface Shown {
  day: Date;
  summary: MorningSummary;
}

/**
 * On the first start of a day, or the first focus of the window after midnight: when yesterday
 * has sessions, a quiet notice with its sum and a way to the short form (MorningReviewDialog).
 * Once per day, also when the day had nothing to show.
 */
export function MorningNotice({
  language,
  enabled,
  offeredDay,
  onOffered,
  load,
  onOpenReview,
  now = systemNow,
}: NoticeProps) {
  const t = useT();
  const [shown, setShown] = useState<Shown | null>(null);
  const [open, setOpen] = useState(false);
  /** The day being loaded or already offered in this run, so a focus never loads twice. */
  const handled = useRef<string | null>(null);

  const check = useCallback(() => {
    const due = morningDue({ now: now(), enabled, offeredDay });
    if (due === null || handled.current === due) return;
    const day = parseDayInput(due);
    if (day === null) return;
    handled.current = due;
    const { start, end } = dayWindow(day);
    load(start, end)
      .then((value) => {
        const review = parseDailyReview(value);
        if (review === null) {
          handled.current = null;
          return;
        }
        onOffered(due);
        const summary = morningSummary(review);
        if (summary !== null) setShown({ day, summary });
      })
      .catch(() => {
        // Tried again on the next focus.
        handled.current = null;
      });
  }, [enabled, offeredDay, load, onOffered, now]);

  useEffect(() => {
    check();
    window.addEventListener("focus", check);
    return () => {
      window.removeEventListener("focus", check);
    };
  }, [check]);

  if (shown === null) return null;
  const close = () => {
    setShown(null);
    setOpen(false);
  };
  return (
    <>
      {!open && (
        <div className="link-notice morning-notice" role="status">
          <span>{noticeText(shown.summary, language, t)}</span>
          <div className="link-actions">
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                setOpen(true);
              }}
            >
              {t("morning.view")}
            </button>
            <button
              type="button"
              className="icon-button"
              aria-label={t("error.dismiss")}
              title={t("error.dismiss")}
              onClick={close}
            >
              <CloseIcon />
            </button>
          </div>
        </div>
      )}
      {/* On the body: the notices' corner lets clicks through and stacks below the dialogs. */}
      {open &&
        createPortal(
          <MorningReviewDialog
            summary={shown.summary}
            language={language}
            onOpenReview={() => {
              close();
              onOpenReview(shown.day);
            }}
            onClose={close}
          />,
          document.body,
        )}
    </>
  );
}

/** "Yesterday: 3 projects, 5 h, 12 commits". */
export function noticeText(summary: MorningSummary, language: string, t: Translate): string {
  return t("morning.notice", {
    projects: countText(summary.projects.length, PROJECT_COUNT, language, t),
    active: formatSpan(summary.activeMs, language, t),
    commits: countText(summary.commits, COMMIT_COUNT, language, t),
  });
}

/** The three lines of one project: time and sessions, commits, what is still open. */
export function projectLines(
  project: MorningProject,
  language: string,
  t: Translate,
): [string, string, string | null] {
  const time = t("morning.timeLine", {
    active: formatSpan(project.activeMs, language, t),
    sessions: countText(project.sessions, SESSION_COUNT, language, t),
  });
  if (!project.isRepo) return [time, t("morning.noRepo"), null];
  const commits =
    project.lastSubject === null
      ? t("morning.noCommits")
      : t("morning.commitLine", {
          commits: countText(project.commits, COMMIT_COUNT, language, t),
          subject: project.lastSubject,
        });
  const open =
    project.openFiles === 0
      ? t("morning.nothingOpen")
      : t("morning.openLine", { files: countText(project.openFiles, FILE_COUNT, language, t) });
  return [time, commits, open];
}

/** Yesterday in short: three lines per project, then the way to the full daily review. */
export function MorningReviewDialog({
  summary,
  language,
  onOpenReview,
  onClose,
}: {
  summary: MorningSummary;
  language: string;
  onOpenReview: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  return (
    <Dialog labelledBy={titleId} className="morning-review" onClose={onClose}>
      <div className="dialog-header">
        <h2 id={titleId}>{t("morning.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
          data-autofocus
        >
          <CloseIcon />
        </button>
      </div>
      <p className="muted morning-sum">{noticeText(summary, language, t)}</p>
      <ul className="morning-projects">
        {summary.projects.map((project) => {
          const [time, commits, open] = projectLines(project, language, t);
          return (
            <li key={project.path} className="morning-project">
              <h3 title={project.path}>{project.name}</h3>
              <p>{time}</p>
              <p className="morning-commits">{commits}</p>
              {open !== null && (
                <p className={project.openFiles > 0 ? "morning-open" : "muted"}>{open}</p>
              )}
            </li>
          );
        })}
      </ul>
      <div className="dialog-actions">
        <button type="button" className="link-button" onClick={onOpenReview}>
          {t("morning.full")}
        </button>
      </div>
    </Dialog>
  );
}
