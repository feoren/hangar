import { useEffect, useId, useState } from "react";

import {
  clampDay,
  dayInputValue,
  dayRange,
  dayWindow,
  formatClock,
  formatDayShort,
  formatSpan,
  limited,
  parseDailyReview,
  parseDayInput,
  reviewMarkdown,
  sameDay,
  sessionName,
  shiftDay,
  type DailyReview,
  type ReviewProject,
} from "./dailyReview";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import { useT, type Translate } from "./i18n";
import { ChevronIcon, CloseIcon } from "./icons";

type Load =
  | { kind: "loading" }
  | { kind: "ready"; review: DailyReview }
  /** `error: null`: the answer was malformed. */
  | { kind: "failed"; error: string | null };

interface Props {
  language: string;
  /** The answer of `daily_review` for `[start, end)`. */
  load: (start: number, end: number) => Promise<unknown>;
  onCopy: (markdown: string) => Promise<void>;
  /** Creates a note with this title and text. */
  onToNotes: (title: string, markdown: string) => Promise<void>;
  onClose: () => void;
  /** The clock, for tests. */
  now?: () => Date;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Daily review: one local day's sessions per project with their times, the day's commits and
 * the files they touched, today also what is uncommitted. Copy or save it as Markdown.
 */
export function DailyReviewDialog({
  language,
  load,
  onCopy,
  onToNotes,
  onClose,
  now = () => new Date(),
}: Props) {
  const t = useT();
  const titleId = useId();
  const [day, setDay] = useState(() => clampDay(now(), now()));
  // Results and notices belong to the day they were made for; another day shows "loading" and
  // no notice until its own result is there, without resetting state inside the effect.
  const [result, setResult] = useState<{ day: number; load: Load } | null>(null);
  const [notice, setNotice] = useState<{ day: number; text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const { first, last } = dayRange(now());
  const dayKey = day.getTime();
  const state: Load = result?.day === dayKey ? result.load : { kind: "loading" };
  const shownNotice = notice?.day === dayKey ? notice : null;

  useEffect(() => {
    let live = true;
    const { start, end } = dayWindow(new Date(dayKey));
    load(start, end)
      .then((value) => {
        if (!live) return;
        const review = parseDailyReview(value);
        setResult({
          day: dayKey,
          load: review === null ? { kind: "failed", error: null } : { kind: "ready", review },
        });
      })
      .catch((error: unknown) => {
        if (live) setResult({ day: dayKey, load: { kind: "failed", error: errorText(error) } });
      });
    return () => {
      live = false;
    };
  }, [dayKey, load]);

  const review = state.kind === "ready" ? state.review : null;
  const canExport = review !== null && review.projects.length > 0 && !busy;
  const markdown = () => (review === null ? "" : reviewMarkdown(review, day, language, t));

  const act = async (
    work: () => Promise<void>,
    done: string,
    failed: (error: string) => string,
  ) => {
    setBusy(true);
    try {
      await work();
      setNotice({ day: dayKey, text: done, error: false });
    } catch (error) {
      setNotice({ day: dayKey, text: failed(errorText(error)), error: true });
    } finally {
      setBusy(false);
    }
  };

  const move = (delta: number) => {
    setDay((current) => clampDay(shiftDay(current, delta), now()));
  };

  return (
    <Dialog labelledBy={titleId} className="daily-review" onClose={onClose}>
      <div className="dialog-header">
        <h2 id={titleId}>{t("review.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="review-nav">
        <button
          type="button"
          className="icon-button review-prev"
          aria-label={t("review.prevDay")}
          title={t("review.prevDay")}
          disabled={sameDay(day, first)}
          onClick={() => {
            move(-1);
          }}
        >
          <ChevronIcon />
        </button>
        <input
          type="date"
          className="text-input review-date"
          aria-label={t("review.day")}
          data-autofocus
          min={dayInputValue(first)}
          max={dayInputValue(last)}
          value={dayInputValue(day)}
          onChange={(event) => {
            const picked = parseDayInput(event.target.value);
            if (picked !== null) setDay(clampDay(picked, now()));
          }}
        />
        <button
          type="button"
          className="icon-button"
          aria-label={t("review.nextDay")}
          title={t("review.nextDay")}
          disabled={sameDay(day, last)}
          onClick={() => {
            move(1);
          }}
        >
          <ChevronIcon />
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={sameDay(day, last)}
          onClick={() => {
            setDay(last);
          }}
        >
          {t("review.today")}
        </button>
      </div>

      <div className="review-body" tabIndex={0} aria-busy={state.kind === "loading"}>
        {state.kind === "loading" && <p className="muted">{t("review.loading")}</p>}
        {state.kind === "failed" && (
          <p className="error-text">
            {t("review.failed", { error: state.error ?? t("error.unknown") })}
          </p>
        )}
        {review !== null && <ReviewContent review={review} language={language} t={t} />}
      </div>

      <div className="dialog-actions review-actions">
        <p
          className={
            shownNotice?.error === true ? "review-notice error-text" : "review-notice muted"
          }
          role="status"
        >
          {shownNotice?.text ?? ""}
        </p>
        <button
          type="button"
          className="secondary-button"
          disabled={!canExport}
          onClick={() => {
            void act(
              () => onCopy(markdown()),
              t("review.copied"),
              (error) => t("review.failed", { error }),
            );
          }}
        >
          {t("review.copy")}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={!canExport}
          onClick={() => {
            const title = t("review.noteTitle", { date: formatDayShort(day, language) });
            void act(
              () => onToNotes(title, markdown()),
              t("review.saved"),
              (error) => t("review.saveFailed", { error }),
            );
          }}
        >
          {t("review.toNotes")}
        </button>
      </div>
    </Dialog>
  );
}

interface ContentProps {
  review: DailyReview;
  language: string;
  t: Translate;
}

/** The loaded review: summary, then one section per project. */
export function ReviewContent({ review, language, t }: ContentProps) {
  const span = (ms: number) => formatSpan(ms, language, t);
  if (review.projects.length === 0) return <p className="muted">{t("review.empty")}</p>;
  return (
    <>
      <dl className="review-summary">
        <div>
          <dt>{t("review.active")}</dt>
          <dd>{span(review.activeMs)}</dd>
        </div>
        <div>
          <dt>{t("review.sessions")}</dt>
          <dd>{review.sessionCount}</dd>
        </div>
        <div>
          <dt>{t("review.projects")}</dt>
          <dd>{review.projects.length}</dd>
        </div>
        <div>
          <dt>{t("review.commits")}</dt>
          <dd>{review.commitCount}</dd>
        </div>
      </dl>
      <p className="muted review-hint">{t("review.activeHint")}</p>
      {review.projects.map((project) => (
        <ProjectSection key={project.path} project={project} language={language} t={t} />
      ))}
      {review.truncated && <p className="muted">{t("review.truncated")}</p>}
    </>
  );
}

function ProjectSection({
  project,
  language,
  t,
}: {
  project: ReviewProject;
  language: string;
  t: Translate;
}) {
  const span = (ms: number) => formatSpan(ms, language, t);
  const clock = (ms: number) => formatClock(ms, language);
  const files = limited(project.files, project.fileCount);
  return (
    <section className="review-project">
      <div className="review-project-head">
        <h3 title={project.path}>{folderName(project.path)}</h3>
        <span className="muted">
          {t("review.active")} {span(project.activeMs)} · {t("review.runTime")}{" "}
          {span(project.runMs)}
        </span>
      </div>
      <ul className="review-sessions">
        {project.sessions.map((session) => (
          <li key={session.key}>
            <span className="review-session-name">{sessionName(session)}</span>
            <span className="muted">
              {t("review.sessionTimes", { from: clock(session.first), to: clock(session.last) })}
              {" · "}
              {t("review.sessionDetail", {
                active: span(session.activeMs),
                run: span(session.runMs),
              })}
              {session.worktree && ` · ${t("review.worktree")}`}
              {session.running && ` · ${t("review.running")}`}
            </span>
          </li>
        ))}
      </ul>
      {project.commits.length > 0 && (
        <>
          <h4>{t("review.commits")}</h4>
          <ul className="review-commits">
            {project.commits.map((commit) => (
              <li key={commit.hash}>
                <code>{commit.hash.slice(0, 7)}</code> <span>{commit.subject}</span>{" "}
                <span className="muted">{clock(commit.time)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {project.fileCount > 0 && (
        <details className="review-files">
          <summary>
            {t("review.files")} ({project.fileCount})
          </summary>
          <ul>
            {files.shown.map((file) => (
              <li key={file}>
                <code>{file}</code>
              </li>
            ))}
            {files.more > 0 && (
              <li className="muted">{t("review.moreFiles", { count: files.more })}</li>
            )}
          </ul>
        </details>
      )}
      {project.uncommitted.map((tree) => {
        const shown = limited(tree.files, tree.count);
        return (
          <details key={tree.path} className="review-files">
            <summary>
              {t("review.uncommitted")} ({tree.count})
              {tree.worktree && <span className="muted"> · {folderName(tree.path)}</span>}
            </summary>
            <ul>
              {shown.shown.map((file) => (
                <li key={file}>
                  <code>{file}</code>
                </li>
              ))}
              {shown.more > 0 && (
                <li className="muted">{t("review.moreFiles", { count: shown.more })}</li>
              )}
            </ul>
          </details>
        );
      })}
    </section>
  );
}
