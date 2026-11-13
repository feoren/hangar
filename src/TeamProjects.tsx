import { useEffect, useId, useRef, useState } from "react";

import {
  accountErrorText,
  projectMeta,
  roleText,
  type AccountProblem,
  type Project,
} from "./account";
import { useT } from "./i18n";
import { TeamRepos } from "./TeamActivityView";
import { TeamCard } from "./TeamCard";
import {
  ARM_DELAY_MS,
  armDelayLeft,
  canToggle,
  confirmationKindText,
  isArmed,
  isSharing,
  personTag,
  releaseRequestKey,
  requestPosition,
  shortId,
  shownRequest,
  statusFor,
  statusLine,
  taskTitle,
  teamErrorText,
  type PendingConfirmation,
  type RepoRef,
  type TaskView,
  type TeamActivity,
  type TeamProblem,
  type TeamStatus,
} from "./team";
import { account } from "./useAccount";
import { taskKey, team } from "./useTeam";

// The Team window's building blocks: a project's details and share switch, connecting a first
// project, the waiting help with the safety number, and the requests that wait for an answer
// (new devices to confirm, tasks to hand over).

/**
 * One project in the Team window's "People" tab: role, plan, seats (per account of the owner)
 * and pending invitations, "Share with team" with the connection's state, while connected the
 * shared repositories, and "Invite people" for owners and admins.
 */
export function ProjectDetails(props: {
  project: Project;
  status: TeamStatus;
  activity: TeamActivity;
  repos: readonly RepoRef[];
  busy: ReadonlySet<string>;
}) {
  const t = useT();
  const { project, status } = props;
  const [shareProblem, setShareProblem] = useState<TeamProblem | null>(null);
  const [inviteProblem, setInviteProblem] = useState<AccountProblem | null>(null);
  const role = roleText(project.role, t);
  const meta = projectMeta(project, t);

  return (
    <div className="team-columns team-details">
      <TeamCard
        className="team-project-card"
        title={<span title={project.name}>{project.name}</span>}
        action={role === null ? undefined : <span className="team-role">{role}</span>}
      >
        {meta.length > 0 && (
          <div className="team-project-meta" title={t("team.seatsHelp")}>
            {meta.join(" · ")}
          </div>
        )}
        <ShareSwitch
          status={status}
          busy={props.busy.has(project.id)}
          problem={shareProblem}
          onToggle={(on) => {
            setShareProblem(null);
            team
              .setSharing(project.id, on)
              .then(setShareProblem)
              .catch(() => undefined);
          }}
        />
        {project.canInvite && (
          <button
            type="button"
            className="secondary-button team-invite"
            title={t("team.inviteHelp")}
            onClick={() => {
              setInviteProblem(null);
              account
                .open({ kind: "invite", projectId: project.id })
                .then(setInviteProblem)
                .catch(() => undefined);
            }}
          >
            {t("team.invite")}
          </button>
        )}
        {inviteProblem !== null && (
          <p className="team-help error-text" role="alert">
            {accountErrorText(inviteProblem, t)}
          </p>
        )}
      </TeamCard>
      {status.state === "connected" && (
        <TeamRepos
          projectId={project.id}
          activity={props.activity}
          repos={props.repos}
          busy={props.busy}
        />
      )}
    </div>
  );
}

/**
 * Signed in, nothing shared yet: every project with one "Connect" button, which turns its
 * "Share with team" on.
 */
export function ConnectList(props: {
  projects: readonly Project[];
  statuses: ReadonlyMap<string, TeamStatus>;
  busy: ReadonlySet<string>;
}) {
  return (
    <ul className="team-connect-list">
      {props.projects.map((project) => (
        <ConnectRow
          key={project.id}
          project={project}
          status={statusFor(props.statuses, project.id)}
          busy={props.busy.has(project.id)}
        />
      ))}
    </ul>
  );
}

function ConnectRow(props: { project: Project; status: TeamStatus; busy: boolean }) {
  const t = useT();
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const { project } = props;
  const role = roleText(project.role, t);
  const details = [role, ...projectMeta(project, t)].filter((part) => part !== null);
  const line = statusLine(props.status, t);
  return (
    <li className="team-connect-row">
      <div className="team-connect-text">
        <span className="team-project-name" title={project.name}>
          {project.name}
        </span>
        {details.length > 0 && <span className="team-project-meta">{details.join(" · ")}</span>}
        {line !== null && (
          <span className={`team-status tone-${line.tone}`} role="status">
            <span className="team-status-mark" aria-hidden="true" />
            <span>{line.text}</span>
          </span>
        )}
        {problem !== null && (
          <span className="team-help error-text" role="alert">
            {teamErrorText(problem, t)}
          </span>
        )}
      </div>
      <ConnectButton
        projectId={project.id}
        projectName={project.name}
        busy={props.busy}
        disabled={!canToggle(props.status)}
        onProblem={setProblem}
      />
    </li>
  );
}

/** "Connect": turns "Share with team" on for one project. */
export function ConnectButton(props: {
  projectId: string;
  projectName: string;
  busy: boolean;
  disabled?: boolean;
  onProblem: (problem: TeamProblem | null) => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      className="primary-button team-connect"
      aria-label={t("teamWindow.connectProject", { name: props.projectName })}
      aria-busy={props.busy}
      disabled={props.busy || props.disabled === true}
      onClick={() => {
        props.onProblem(null);
        team
          .setSharing(props.projectId, true)
          .then(props.onProblem)
          .catch(() => undefined);
      }}
    >
      {t("teamWindow.connect")}
    </button>
  );
}

/** "Share with team" with the team connection's state below it. */
export function ShareSwitch(props: {
  status: TeamStatus;
  busy: boolean;
  problem: TeamProblem | null;
  onToggle: (on: boolean) => void;
}) {
  const t = useT();
  const statusId = useId();
  const { status } = props;
  const sharing = isSharing(status);
  const line = statusLine(status, t);

  return (
    <div className="team-share">
      <label className="team-share-row" title={t("team.shareHelp")}>
        <span className="team-share-label">{t("team.share")}</span>
        <input
          type="checkbox"
          role="switch"
          className="switch"
          checked={sharing}
          aria-checked={sharing}
          aria-describedby={line === null ? undefined : statusId}
          aria-busy={props.busy}
          disabled={props.busy || !canToggle(status)}
          onChange={(event) => {
            props.onToggle(event.currentTarget.checked);
          }}
        />
      </label>
      {line !== null && (
        <div id={statusId} className={`team-status tone-${line.tone}`} role="status">
          <span className="team-status-mark" aria-hidden="true" />
          <span>{line.text}</span>
        </div>
      )}
      {props.problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(props.problem, t)}
        </p>
      )}
    </div>
  );
}

/**
 * This device waits until another device of the person, or an owner or admin, confirms it: what
 * to do, and the safety number to compare, large.
 */
export function WaitingHelp({ safetyNumber }: { safetyNumber: readonly string[] }) {
  const t = useT();
  return (
    <div className="team-waiting">
      <p className="team-waiting-title">{t("team.status.waiting")}</p>
      <p className="team-help">{t("team.status.waitingHelp")}</p>
      <SafetyNumber groups={safetyNumber} />
    </div>
  );
}

/** The 6 groups of a safety number, monospaced in a 3 by 2 grid for comparing at a glance. */
export function SafetyNumber({ groups }: { groups: readonly string[] }) {
  const t = useT();
  return (
    <div className="safety-number" role="group" aria-label={t("team.safetyNumber")}>
      {groups.map((group, index) => (
        // The groups are a fixed sequence; their position is their identity.
        <span key={index}>{group}</span>
      ))}
    </div>
  );
}

/**
 * New devices waiting for this device's confirmation, one at a time: the same request stays while
 * it is pending, the next one follows once it has been answered or withdrawn.
 */
export function PendingConfirmations(props: {
  pending: readonly PendingConfirmation[];
  projects: readonly Project[];
  busy: ReadonlySet<string>;
  /** After the last answer the panel is gone: the caller takes the focus. */
  onLastAnswered: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  const [shownId, setShownId] = useState<string | null>(null);

  const request = shownRequest(props.pending, shownId);
  const nextId = request?.id ?? null;
  // Adjusting state while rendering (instead of in an effect) avoids one frame with the old id.
  if (nextId !== shownId) setShownId(nextId);
  if (request === null) return null;

  const position = requestPosition(props.pending, request.id);
  const projectName = props.projects.find((project) => project.id === request.projectId)?.name;

  return (
    <section ref={sectionRef} className="team-confirm" aria-labelledby={titleId} tabIndex={-1}>
      <div className="team-confirm-head">
        <h3 id={titleId} className="team-confirm-title">
          {t("team.confirm.title")}
        </h3>
        {position !== null && position.count > 1 && (
          <span className="team-confirm-position">
            {t("team.confirm.position", { index: position.index, count: position.count })}
          </span>
        )}
      </div>
      <PendingRequest
        // A new request gets fresh state: disarmed answers, no open reject question, no error.
        key={request.id}
        request={request}
        projectName={projectName}
        busy={props.busy.has(request.id)}
        onAnswered={() => {
          // Keep the keyboard in the panel, but never on the next request's Confirm button: a
          // second Enter must not confirm a device whose safety number nobody compared. After the
          // last request the panel is gone; the caller takes the focus instead.
          requestAnimationFrame(() => {
            if (sectionRef.current?.isConnected === true) sectionRef.current.focus();
            else props.onLastAnswered();
          });
        }}
      />
    </section>
  );
}

/** One request with its safety number, the answers and the question before rejecting. */
function PendingRequest(props: {
  request: PendingConfirmation;
  projectName: string | undefined;
  busy: boolean;
  onAnswered: () => void;
}) {
  const t = useT();
  const { request } = props;
  const [armed, setArmed] = useState(false);
  const [askingReject, setAskingReject] = useState(false);
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  const rejectRef = useRef<HTMLButtonElement>(null);
  const askedOnce = useRef(false);

  // The answers stay disabled for ARM_DELAY_MS after the request appears; the component is keyed
  // by the request, so every new request starts disarmed.
  useEffect(() => {
    const shownAt = performance.now();
    let timer = 0;
    const check = () => {
      const now = performance.now();
      if (isArmed(shownAt, now)) setArmed(true);
      else timer = window.setTimeout(check, armDelayLeft(shownAt, now));
    };
    timer = window.setTimeout(check, ARM_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, []);

  // The question before rejecting takes the focus on its safe answer; going back returns it to
  // Reject, where it came from.
  useEffect(() => {
    if (askingReject) {
      askedOnce.current = true;
      keepRef.current?.focus();
    } else if (askedOnce.current) {
      rejectRef.current?.focus();
    }
  }, [askingReject]);

  const answer = (confirm: boolean) => {
    setProblem(null);
    (confirm ? team.confirm(request.id) : team.reject(request.id))
      .then((next) => {
        if (next !== null && next.kind !== "unknownRequest") {
          setProblem(next);
          setAskingReject(false);
          return;
        }
        props.onAnswered();
      })
      .catch(() => undefined);
  };

  const disabled = props.busy || !armed;

  return (
    <div className="team-confirm-item">
      <div className="team-confirm-kind">{confirmationKindText(request.kind, t)}</div>
      <div className="team-confirm-meta">
        {props.projectName !== undefined && (
          <span className="team-confirm-project" title={props.projectName}>
            {props.projectName}
          </span>
        )}
        {request.kind !== "ownDevice" && (
          <span title={request.member}>
            {t("team.confirm.member", { id: shortId(request.member) })}
          </span>
        )}
        <span title={request.device}>
          {t("team.confirm.device", { id: shortId(request.device) })}
        </span>
      </div>
      {request.readmitted && <p className="team-confirm-warning">{t("team.confirm.readmitted")}</p>}
      <SafetyNumber groups={request.safetyNumber} />
      <p className="team-help">{t("team.confirm.compare")}</p>
      {askingReject ? (
        <div className="team-confirm-ask" role="group" aria-label={t("team.confirm.reject")}>
          <p className="team-help">{t("team.confirm.rejectQuestion")}</p>
          <div className="team-confirm-actions">
            <button
              type="button"
              className="primary-button danger"
              disabled={props.busy}
              onClick={() => {
                answer(false);
              }}
            >
              {t("team.confirm.reject")}
            </button>
            <button
              ref={keepRef}
              type="button"
              className="secondary-button"
              disabled={props.busy}
              onClick={() => {
                setAskingReject(false);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div className="team-confirm-actions">
          <button
            type="button"
            className="primary-button"
            disabled={disabled}
            onClick={() => {
              answer(true);
            }}
          >
            {t("team.confirm.accept")}
          </button>
          <button
            ref={rejectRef}
            type="button"
            className="secondary-button"
            disabled={disabled}
            onClick={() => {
              setAskingReject(true);
            }}
          >
            {t("team.confirm.reject")}
          </button>
        </div>
      )}
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </div>
  );
}

/**
 * Release requests for tasks this device holds, one at a time like the device confirmations: the
 * same request stays while it is open, Release and Keep stay disabled for a moment whenever the
 * shown request changes, and an answer never lands on the next request's buttons.
 */
export function ReleaseRequests(props: {
  requests: readonly { projectId: string; task: TaskView }[];
  projects: readonly Project[];
  busy: ReadonlySet<string>;
  /** After the last answer the panel is gone: the caller takes the focus. */
  onLastAnswered: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  const [shownKey, setShownKey] = useState<string | null>(null);
  const keyed = props.requests.map((entry) => ({
    ...entry,
    key: releaseRequestKey(entry.projectId, entry.task),
  }));
  const shown = keyed.find((entry) => entry.key === shownKey) ?? keyed[0] ?? null;
  const nextKey = shown?.key ?? null;
  if (nextKey !== shownKey) setShownKey(nextKey);
  if (shown === null) return null;
  const index = keyed.indexOf(shown) + 1;
  const projectName = props.projects.find((project) => project.id === shown.projectId)?.name;

  return (
    <section ref={sectionRef} className="team-confirm" aria-labelledby={titleId} tabIndex={-1}>
      <div className="team-confirm-head">
        <h3 id={titleId} className="team-confirm-title">
          {t("team.request.title")}
        </h3>
        {keyed.length > 1 && (
          <span className="team-confirm-position">
            {t("team.confirm.position", { index, count: keyed.length })}
          </span>
        )}
      </div>
      <ReleaseRequest
        key={shown.key}
        projectId={shown.projectId}
        task={shown.task}
        projectName={projectName}
        busy={props.busy.has(taskKey(shown.projectId, shown.task.task))}
        onAnswered={() => {
          requestAnimationFrame(() => {
            if (sectionRef.current?.isConnected === true) sectionRef.current.focus();
            else props.onLastAnswered();
          });
        }}
      />
    </section>
  );
}

function ReleaseRequest(props: {
  projectId: string;
  task: TaskView;
  projectName: string | undefined;
  busy: boolean;
  onAnswered: () => void;
}) {
  const t = useT();
  const { task } = props;
  const [armed, setArmed] = useState(false);
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const [sent, setSent] = useState(false);

  // Keyed by the request: every new request starts disarmed for ARM_DELAY_MS.
  useEffect(() => {
    const shownAt = performance.now();
    let timer = 0;
    const check = () => {
      const now = performance.now();
      if (isArmed(shownAt, now)) setArmed(true);
      else timer = window.setTimeout(check, armDelayLeft(shownAt, now));
    };
    timer = window.setTimeout(check, ARM_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, []);

  const request = task.request;
  const answer = (accept: boolean) => {
    if (request === null || sent) return;
    // One answer per card: the buttons stay disabled until the backend says otherwise.
    setSent(true);
    setProblem(null);
    team
      .answerRelease(props.projectId, task.task, request.member, accept)
      .then((next) => {
        if (next !== null && next.kind !== "unknownTask") {
          setProblem(next);
          setSent(false);
          return;
        }
        props.onAnswered();
      })
      .catch(() => undefined);
  };
  const person = request === null ? "" : personTag(request, t);
  const disabled = props.busy || !armed || sent;

  return (
    <div className="team-confirm-item">
      {props.projectName !== undefined && (
        <div className="team-confirm-meta">
          <span className="team-confirm-project" title={props.projectName}>
            {props.projectName}
          </span>
        </div>
      )}
      <p className="team-help team-request-text">
        {t("team.request.text", { person, title: taskTitle(task.title, t) })}
      </p>
      <div className="team-confirm-actions">
        <button
          type="button"
          className="primary-button"
          disabled={disabled}
          onClick={() => {
            answer(true);
          }}
        >
          {t("team.request.release")}
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={disabled}
          onClick={() => {
            answer(false);
          }}
        >
          {t("team.request.keep")}
        </button>
      </div>
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </div>
  );
}
