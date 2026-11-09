import { useId, useRef, useState, type SyntheticEvent } from "react";

import { useT } from "./i18n";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import {
  attemptText,
  canAskRelease,
  clockTime,
  holderName,
  linkableSessions,
  personTag,
  taskTitle,
  teamErrorText,
  type AttemptView,
  type TaskView,
  type TeamActivity,
  type TeamProblem,
} from "./team";
import { taskKey, team } from "./useTeam";

/** A running session the person can link a reservation to. */
export interface TaskSession {
  id: number;
  title: string;
}

/**
 * "Tasks" of a connected shared project ("I'm working on ..."): one "Reserve task" entry, the
 * person's attempts that someone else was first for, and every reservation of the team with its
 * holder, since when, when it runs out, and one action each. The relay decides who holds a task;
 * nothing here changes before its answer.
 */
export function TeamTasksView(props: {
  projectId: string;
  activity: TeamActivity;
  sessions: readonly TaskSession[];
  busy: ReadonlySet<string>;
}) {
  const t = useT();
  const titleId = useId();
  const [open, setOpen] = useState(false);
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const { tasks, attempts } = props.activity;
  const run = (action: Promise<TeamProblem | null>) => {
    setProblem(null);
    action.then(setProblem).catch(() => undefined);
  };

  return (
    <section className="team-tasks" aria-labelledby={titleId}>
      <h4 id={titleId} className="team-activity-title">
        {t("team.tasks.title")}
      </h4>
      {open ? (
        <ReserveForm
          projectId={props.projectId}
          sessions={linkableSessions(props.sessions, props.activity)}
          busy={props.busy.has(taskKey(props.projectId, "new"))}
          onDone={() => {
            setOpen(false);
          }}
        />
      ) : (
        <button
          type="button"
          className="secondary-button team-task-reserve"
          onClick={() => {
            setProblem(null);
            setOpen(true);
          }}
        >
          {t("team.tasks.reserve")}
        </button>
      )}
      {attempts.length > 0 && (
        <ul className="team-task-list">
          {attempts.map((attempt) => (
            <Attempt
              key={attempt.task === "" ? attempt.title : attempt.task}
              attempt={attempt}
              busy={props.busy.has(taskKey(props.projectId, attempt.task))}
              onAsk={() => {
                run(team.askRelease(props.projectId, attempt.task));
              }}
              onCancel={() => {
                run(team.releaseTask(props.projectId, attempt.task));
              }}
            />
          ))}
        </ul>
      )}
      {tasks.length === 0 ? (
        <p className="team-help">{t("team.tasks.none")}</p>
      ) : (
        <ul className="team-task-list">
          {tasks.map((task) => (
            <Reservation
              key={task.task}
              task={task}
              busy={props.busy.has(taskKey(props.projectId, task.task))}
              onRelease={() => {
                run(team.releaseTask(props.projectId, task.task));
              }}
              onAsk={() => {
                run(team.askRelease(props.projectId, task.task));
              }}
            />
          ))}
        </ul>
      )}
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </section>
  );
}

function ReserveForm(props: {
  projectId: string;
  sessions: readonly TaskSession[];
  busy: boolean;
  onDone: () => void;
}) {
  const t = useT();
  const ids = useId();
  const [title, setTitle] = useState("");
  const [session, setSession] = useState("");
  const [area, setArea] = useState("");
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const sessionId = session === "" ? null : Number(session);

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (title.trim() === "") {
      titleRef.current?.focus();
      return;
    }
    setProblem(null);
    const folder = sessionId === null || area.trim() === "" ? null : area.trim();
    team
      .reserveTask(props.projectId, title.trim(), sessionId, folder)
      .then((next) => {
        if (next === null) props.onDone();
        else setProblem(next);
      })
      .catch(() => undefined);
  };

  return (
    <form className="team-task-form" onSubmit={submit}>
      <label className="team-task-field" htmlFor={`${ids}-title`}>
        {t("team.tasks.titleLabel")}
      </label>
      <input
        ref={titleRef}
        id={`${ids}-title`}
        className="text-input"
        value={title}
        maxLength={120}
        placeholder={t("team.tasks.titlePlaceholder")}
        // The form opens because the person asked for it: the title takes the keyboard.
        autoFocus
        onChange={(event) => {
          setTitle(event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") props.onDone();
        }}
      />
      {props.sessions.length > 0 && (
        <>
          <label className="team-task-field" htmlFor={`${ids}-session`}>
            {t("team.tasks.session")}
          </label>
          <select
            id={`${ids}-session`}
            className="text-input"
            value={session}
            onChange={(event) => {
              setSession(event.currentTarget.value);
            }}
          >
            <option value="">{t("team.tasks.noSession")}</option>
            {props.sessions.map((entry) => (
              <option key={entry.id} value={String(entry.id)}>
                {revealInvisible(entry.title)}
              </option>
            ))}
          </select>
        </>
      )}
      {sessionId !== null && (
        <>
          <label className="team-task-field" htmlFor={`${ids}-area`}>
            {t("team.tasks.area")}
          </label>
          <input
            id={`${ids}-area`}
            className="text-input mono"
            value={area}
            placeholder="src/auth"
            aria-describedby={`${ids}-area-help`}
            onChange={(event) => {
              setArea(event.currentTarget.value);
            }}
          />
          <p id={`${ids}-area-help`} className="team-help">
            {t("team.tasks.areaHelp")}
          </p>
        </>
      )}
      <div className="team-confirm-actions">
        <button type="submit" className="primary-button" disabled={props.busy}>
          {t("team.tasks.confirm")}
        </button>
        <button type="button" className="secondary-button" onClick={props.onDone}>
          {t("common.cancel")}
        </button>
      </div>
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </form>
  );
}

function Reservation(props: {
  task: TaskView;
  busy: boolean;
  onRelease: () => void;
  onAsk: () => void;
}) {
  const t = useT();
  const { task } = props;
  const title = taskTitle(task.title, t);
  const meta = [holderName(task, t), t("team.tasks.since", { time: clockTime(task.since) })];
  if (task.expiresAt !== null) {
    meta.push(t("team.tasks.expires", { time: clockTime(task.expiresAt) }));
  }
  if (task.request !== null) {
    if (task.request.mine) meta.push(t("team.tasks.asked"));
    else if (task.request.confirmed) {
      meta.push(t("team.tasks.requested", { person: personTag(task.request, t) }));
    }
    // The relay's word without the requester's signature: a quiet line, no card.
    else meta.push(t("team.tasks.unconfirmed"));
  }
  return (
    <li className={task.expiring ? "team-task expiring" : "team-task"}>
      <div className="team-task-head">
        <span className="team-task-title" title={title}>
          <Revealed text={task.title.trim() === "" ? title : task.title} />
        </span>
        {task.mine && (
          <button
            type="button"
            className="link-button"
            disabled={props.busy}
            onClick={props.onRelease}
          >
            {t("team.tasks.release")}
          </button>
        )}
        {canAskRelease(task) && (
          <button type="button" className="link-button" disabled={props.busy} onClick={props.onAsk}>
            {t("team.tasks.ask")}
          </button>
        )}
      </div>
      <div className="team-task-meta" title={task.member}>
        {meta.join(" · ")}
        {task.area !== null && (
          <>
            {" · "}
            <span className="team-task-area">{revealInvisible(task.area)}</span>
          </>
        )}
      </div>
    </li>
  );
}

function Attempt(props: {
  attempt: AttemptView;
  busy: boolean;
  onAsk: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const { attempt } = props;
  const blocked = attempt.state === "blocked";
  return (
    <li className="team-task attempt" role="status">
      <div className="team-task-head">
        <span className="team-task-title" title={revealInvisible(attempt.title)}>
          <Revealed text={attempt.title} />
        </span>
        {blocked && !attempt.asked && attempt.task !== "" && (
          <button type="button" className="link-button" disabled={props.busy} onClick={props.onAsk}>
            {t("team.tasks.ask")}
          </button>
        )}
        {attempt.task !== "" && (
          <button
            type="button"
            className="link-button"
            disabled={props.busy}
            onClick={props.onCancel}
          >
            {t("common.cancel")}
          </button>
        )}
      </div>
      <div className="team-task-meta">
        {[
          attemptText(attempt, t),
          ...(blocked && attempt.asked ? [t("team.tasks.asked")] : []),
        ].join(" · ")}
      </div>
    </li>
  );
}
