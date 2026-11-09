import { useEffect, useId, useRef, useState, type ReactNode } from "react";

import {
  accountErrorText,
  projectMeta,
  roleText,
  type AccountProblem,
  type Project,
} from "./account";
import { useT } from "./i18n";
import {
  ARM_DELAY_MS,
  armDelayLeft,
  canToggle,
  confirmationKindText,
  isArmed,
  isSharing,
  requestPosition,
  shortId,
  shownRequest,
  statusFor,
  statusLine,
  teamErrorText,
  type PendingConfirmation,
  type TeamProblem,
  type TeamStatus,
} from "./team";
import { account, loadProjects, type AccountSnapshot } from "./useAccount";
import { loadTeamStatuses, team, useTeam } from "./useTeam";

/**
 * Body of the sidebar's Team group: new devices waiting for confirmation (only when there are
 * any), then the account's projects with role, plan, seats (per account of the owner), pending
 * invitations, the "Share with team" switch with the team connection's state, and "Invite people"
 * for owners and admins. Shown only while signed in.
 */
export function TeamProjects({ snapshot }: { snapshot: AccountSnapshot }) {
  const t = useT();
  const teamStore = useTeam();
  const [problem, setProblem] = useState<AccountProblem | null>(null);
  const [shareProblems, setShareProblems] = useState<ReadonlyMap<string, TeamProblem>>(
    () => new Map(),
  );
  const { projects, projectsError, loadingProjects } = snapshot;

  useEffect(() => {
    if (projects !== null) loadTeamStatuses(projects.map((project) => project.id));
  }, [projects]);

  const setShareProblem = (projectId: string, next: TeamProblem | null) => {
    setShareProblems((previous) => {
      const map = new Map(previous);
      if (next === null) map.delete(projectId);
      else map.set(projectId, next);
      return map;
    });
  };

  const pending =
    teamStore.pending.length > 0 ? (
      <PendingConfirmations
        pending={teamStore.pending}
        projects={projects ?? []}
        busy={teamStore.busy}
      />
    ) : null;

  const reload = (
    <button
      type="button"
      className="link-button team-reload"
      disabled={loadingProjects}
      onClick={() => {
        void loadProjects();
      }}
    >
      {t("team.reload")}
    </button>
  );

  let body: ReactNode;
  if (projectsError !== null) {
    body = (
      <p className="hint group-hint team-hint" role="status">
        {projectsError.kind === "sessionEnded"
          ? t("account.sessionEnded")
          : accountErrorText(projectsError, t)}{" "}
        {projectsError.kind !== "sessionEnded" && reload}
      </p>
    );
  } else if (projects === null) {
    body = <p className="hint group-hint team-hint">{t("team.loading")}</p>;
  } else if (projects.length === 0) {
    body = (
      <p className="hint group-hint team-hint">
        {t("team.empty")} {reload}
      </p>
    );
  } else {
    body = (
      <ul className="team-list">
        {projects.map((project) => {
          const role = roleText(project.role, t);
          const meta = projectMeta(project, t);
          return (
            <li key={project.id} className="team-project">
              <div className="team-project-head">
                <span className="team-project-name" title={project.name}>
                  {project.name}
                </span>
                {role !== null && <span className="team-role">{role}</span>}
              </div>
              {meta.length > 0 && (
                <div className="team-project-meta" title={t("team.seatsHelp")}>
                  {meta.join(" · ")}
                </div>
              )}
              <ShareSwitch
                project={project}
                status={statusFor(teamStore.statuses, project.id)}
                busy={teamStore.busy.has(project.id)}
                problem={shareProblems.get(project.id) ?? null}
                onToggle={(on) => {
                  setShareProblem(project.id, null);
                  team
                    .setSharing(project.id, on)
                    .then((next) => {
                      setShareProblem(project.id, next);
                    })
                    .catch(() => undefined);
                }}
              />
              {project.canInvite && (
                <button
                  type="button"
                  className="secondary-button team-invite"
                  title={t("team.inviteHelp")}
                  onClick={() => {
                    setProblem(null);
                    account
                      .open({ kind: "invite", projectId: project.id })
                      .then(setProblem)
                      .catch(() => undefined);
                  }}
                >
                  {t("team.invite")}
                </button>
              )}
            </li>
          );
        })}
      </ul>
    );
  }

  return (
    <>
      {pending}
      {body}
      {problem !== null && (
        <p className="hint group-hint error-text" role="alert">
          {accountErrorText(problem, t)}
        </p>
      )}
    </>
  );
}

/** "Share with team" with the team connection's state below it. */
function ShareSwitch(props: {
  project: Project;
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
      {status.state === "waitingForConfirmation" && (
        <>
          <p className="team-help">{t("team.status.waitingHelp")}</p>
          <SafetyNumber groups={status.safetyNumber} />
        </>
      )}
      {props.problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(props.problem, t)}
        </p>
      )}
    </div>
  );
}

/** The 6 groups of a safety number, monospaced in a 3 by 2 grid for comparing at a glance. */
function SafetyNumber({ groups }: { groups: readonly string[] }) {
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
function PendingConfirmations(props: {
  pending: readonly PendingConfirmation[];
  projects: readonly Project[];
  busy: ReadonlySet<string>;
}) {
  const t = useT();
  const titleId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  // The Team group's heading button, found while the panel is mounted: after the last answer
  // the panel is gone, and the focus would otherwise fall back to the page body.
  const headingRef = useRef<HTMLElement | null>(null);
  const [shownId, setShownId] = useState<string | null>(null);

  useEffect(() => {
    const heading = sectionRef.current
      ?.closest(".sidebar-group")
      ?.querySelector<HTMLElement>(".group-toggle");
    if (heading !== undefined && heading !== null) headingRef.current = heading;
  });

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
          // last request the panel is gone; the Team group's heading takes the focus instead.
          requestAnimationFrame(() => {
            const target = sectionRef.current ?? headingRef.current;
            if (target?.isConnected === true) target.focus();
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
