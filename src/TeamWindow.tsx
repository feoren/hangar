import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { accountErrorText, isSignedIn, type Project } from "./account";
import { AccountStateSection } from "./AccountSettings";
import { DialogResizeHandles, useDialogSize, type DialogSizeBounds } from "./DialogResize";
import { useT } from "./i18n";
import { ChevronIcon, CloseIcon, GearIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import {
  EMPTY_ACTIVITY,
  canToggle,
  releaseRequests,
  statusFor,
  statusLine,
  teamErrorText,
  type TeamProblem,
  type TeamStatus,
} from "./team";
import { TeamPeople } from "./TeamActivityView";
import {
  AdmissionCheck,
  ConnectButton,
  ConnectList,
  InviteButton,
  NoSeatNote,
  PendingConfirmations,
  ProjectDetails,
  ReleaseRequests,
  WaitingHelp,
} from "./TeamProjects";
import { TeamDecisionsView, type DecisionTarget } from "./TeamDecisionsView";
import { TeamTasksView, type TaskSession } from "./TeamTasksView";
import { noSeatHint, refusedForSeat, teamAccess } from "./teamPlan";
import { TeamPlanNote } from "./TeamPlanNote";
import { anyShared, shownProject } from "./teamOverview";
import { loadProjects, useAccount } from "./useAccount";
import { useTeam } from "./useTeam";

/** The project shown last while Hangar runs; the window opens on it again. */
let lastProject: string | null = null;

/**
 * Resizable like the Settings dialog. The standard size matches `--large-dialog-width` and
 * `--large-dialog-height`; the minimum keeps the header and one person row readable.
 */
const SIZE_BOUNDS: DialogSizeBounds = {
  storageKey: "hangar.teamWindowSize",
  standard: { width: 936, height: 672 },
  min: { width: 560, height: 400 },
};

/**
 * The Team window: a floating panel over the terminals, not modal. The terminals stay visible and
 * keep running; Tab may leave the panel. It closes on Escape (with the focus inside), its close
 * button and a click outside; the caller then hands the focus back to the terminal.
 *
 * One state at a time: sign in, connect a first project, or the team itself. The team opens on
 * what people ask most, without a click: who is here, what each of them works on, the tasks and
 * decisions. Whatever waits for an answer from this device sits on top, under "Waiting for you",
 * and leaves once answered. Things done rarely (sharing, invitations, repositories, devices) sit
 * behind "Manage" in the header; Escape steps back from there first.
 */
export function TeamWindow(props: {
  /** This device's running sessions, for linking a reservation to one. */
  sessions: readonly TaskSession[];
  /**
   * The session "Send to session" pastes a decision into: the active one, when its agent does
   * not get the team's decisions as context by itself; `null` hides the action.
   */
  decisionTarget: DecisionTarget | null;
  onSendDecision: (text: string, sessionId: number) => void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const manageButton = useRef<HTMLButtonElement>(null);
  const size = useDialogSize(SIZE_BOUNDS);
  const snapshot = useAccount();
  const teamStore = useTeam();
  const { projects, projectsError, loadingProjects, view } = snapshot;
  const release = releaseRequests(teamStore.activities);
  const requestCount = teamStore.pending.length + release.length;
  const [managing, setManaging] = useState(false);
  const [chosenProject, setChosenProject] = useState<string | null>(null);
  const { onClose } = props;

  const projectIds = (projects ?? []).map((project) => project.id);
  const projectId = shownProject(projectIds, chosenProject, teamStore.statuses, lastProject);
  const project = projects?.find((entry) => entry.id === projectId) ?? null;
  const signedIn = isSignedIn(view);
  /** Free without a team (teamPlan.ts): the window explains the Team plan instead. */
  const planOnly = teamAccess(view, projects) === "no";
  const shared =
    signedIn &&
    !planOnly &&
    projects !== null &&
    projects.length > 0 &&
    anyShared(projectIds, teamStore.statuses);
  const showManage = shared && project !== null && managing;

  useEffect(() => {
    if (projectId !== null) lastProject = projectId;
  }, [projectId]);

  // Layout effect, so the focus is in place before the app would hand it to the terminal.
  useLayoutEffect(() => {
    const element = panel.current;
    if (element === null) return;
    const target = element.querySelector<HTMLElement>("[data-autofocus]") ?? element;
    target.focus();
  }, []);

  // A click outside closes the window; the buttons that toggle it handle their own click.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (panel.current?.contains(target) === true) return;
      if (target.closest("[data-team-toggle]") !== null) return;
      onClose();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [onClose]);

  /** After the last answer the requests are gone: the keyboard stays in the window. */
  const focusPanel = () => {
    panel.current?.focus();
  };

  let body: ReactNode;
  if (view === null) {
    body = snapshot.unavailable ? (
      <p className="team-help team-window-note">{t("account.unavailable")}</p>
    ) : null;
  } else if (!signedIn) {
    body = (
      <div className="team-window-center">
        <p className="team-window-lead">{t("teamWindow.signInIntro")}</p>
        <AccountStateSection view={view} />
        <TeamPlanNote />
      </div>
    );
  } else if (projectsError !== null) {
    body = (
      <p className="team-help team-window-note" role="status">
        {projectsError.kind === "sessionEnded"
          ? t("account.sessionEnded")
          : accountErrorText(projectsError, t)}{" "}
        {projectsError.kind !== "sessionEnded" && (
          <ReloadButton busy={loadingProjects} label={t("team.reload")} />
        )}
      </p>
    );
  } else if (projects === null) {
    body = <p className="team-help team-window-note">{t("team.loading")}</p>;
  } else if (planOnly) {
    // Free is for one person: no project to share, no invitations, only the way to the Team plan.
    body = (
      <div className="team-window-center">
        <TeamPlanNote extra={<ReloadButton busy={loadingProjects} label={t("team.reload")} />} />
      </div>
    );
  } else if (projects.length === 0) {
    body = (
      <p className="team-help team-window-note">
        {t("team.empty")} <ReloadButton busy={loadingProjects} label={t("team.reload")} />
      </p>
    );
  } else if (!shared) {
    body = (
      <div className="team-window-connect">
        <p className="team-window-lead">{t("teamWindow.connectIntro")}</p>
        <ConnectList projects={projects} statuses={teamStore.statuses} busy={teamStore.busy} />
      </div>
    );
  } else if (project !== null) {
    const status = statusFor(teamStore.statuses, project.id);
    const activity = teamStore.activities.get(project.id) ?? EMPTY_ACTIVITY;
    const invite =
      project.canInvite && !noSeatHint(project, status) ? (
        <InviteButton projectId={project.id} />
      ) : undefined;
    body = showManage ? (
      <ProjectDetails
        // A project of its own gets fresh state: no problem left over from another one.
        key={project.id}
        project={project}
        status={status}
        activity={activity}
        repos={teamStore.repos}
        busy={teamStore.busy}
      />
    ) : (
      <TeamNow
        project={project}
        status={status}
        busy={teamStore.busy.has(project.id)}
        content={
          <>
            <TeamPeople activity={activity} invite={invite} />
            <div className="team-stack">
              <TeamTasksView
                projectId={project.id}
                activity={activity}
                sessions={props.sessions}
                busy={teamStore.busy}
              />
              <TeamDecisionsView
                projectId={project.id}
                activity={activity}
                busy={teamStore.busy}
                manages={project.role === "owner" || project.role === "admin"}
                target={props.decisionTarget}
                onSend={props.onSendDecision}
              />
            </div>
          </>
        }
      />
    );
  }

  const waiting =
    shared && requestCount > 0 ? (
      <WaitingForYou>
        <ReleaseRequests
          requests={release}
          projects={projects}
          busy={teamStore.busy}
          onLastAnswered={focusPanel}
        />
        <PendingConfirmations
          pending={teamStore.pending}
          projects={projects}
          busy={teamStore.busy}
          onLastAnswered={focusPanel}
        />
      </WaitingForYou>
    ) : null;

  return (
    <div
      ref={panel}
      className="dialog team-window resizable"
      style={size.style}
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        // From "Manage", Escape goes back to the team first; from there it closes the window.
        if (showManage) {
          setManaging(false);
          manageButton.current?.focus();
        } else onClose();
      }}
    >
      <div className="dialog-header team-window-header">
        <h2 id={titleId}>{t("team.title")}</h2>
        {shared && project !== null && (
          <ProjectSwitch
            projects={projects}
            current={project}
            onChange={(id) => {
              setChosenProject(id);
            }}
          />
        )}
        {shared && project !== null && (
          <button
            ref={manageButton}
            type="button"
            className={
              showManage ? "secondary-button team-manage back" : "secondary-button team-manage"
            }
            title={showManage ? undefined : t("teamWindow.manageHelp")}
            onClick={() => {
              setManaging(!showManage);
            }}
          >
            {showManage ? <ChevronIcon size={14} /> : <GearIcon size={14} />}
            <span>{showManage ? t("teamWindow.back") : t("teamWindow.manage")}</span>
          </button>
        )}
        <button
          type="button"
          className="icon-button team-window-close"
          aria-label={t("common.close")}
          title={t("common.close")}
          data-autofocus={shared ? undefined : true}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <div className="team-window-body">
        {waiting}
        {body}
      </div>
      <DialogResizeHandles state={size} />
    </div>
  );
}

/** The project name, or a small picker when the account has more than one project. */
function ProjectSwitch(props: {
  projects: readonly Project[];
  current: Project;
  onChange: (id: string) => void;
}) {
  const t = useT();
  if (props.projects.length < 2) {
    return (
      <span className="team-window-project" title={revealInvisible(props.current.name)}>
        <Revealed text={props.current.name} />
      </span>
    );
  }
  return (
    <select
      className="select team-window-switch"
      aria-label={t("teamWindow.project")}
      value={props.current.id}
      onChange={(event) => {
        props.onChange(event.currentTarget.value);
      }}
    >
      {props.projects.map((project) => (
        <option key={project.id} value={project.id}>
          {/* An option holds text only: the invisible characters as their placeholders. */}
          {revealInvisible(project.name)}
        </option>
      ))}
    </select>
  );
}

/**
 * Everything that waits for an answer from this device, on top of the window: release requests
 * and devices to confirm, one at a time each. Gone once nothing is left.
 */
function WaitingForYou({ children }: { children: ReactNode }) {
  const t = useT();
  const titleId = useId();
  return (
    <section className="team-attention" aria-labelledby={titleId}>
      <h3 id={titleId} className="team-attention-title">
        {t("teamWindow.waitingForYou")}
      </h3>
      {children}
    </section>
  );
}

/**
 * The team of one project: while connected, who is here and what each person works on, the tasks
 * and decisions; otherwise what the connection does, with the waiting help or "Connect" where
 * that helps.
 */
function TeamNow(props: {
  project: Project;
  status: TeamStatus;
  content: ReactNode;
  busy: boolean;
}) {
  const t = useT();
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const { status } = props;
  if (status.state === "connected")
    return <div className="team-columns team-now">{props.content}</div>;
  if (status.state === "waitingForKeyChange") {
    // The teammates stay visible; only this device's sharing waits for the new key.
    const waiting = statusLine(status, t);
    return (
      <>
        {waiting !== null && (
          <p className={`team-status tone-${waiting.tone}`} role="status">
            <span className="team-status-mark" aria-hidden="true" />
            <span>{waiting.text}</span>
          </p>
        )}
        <div className="team-columns team-now">{props.content}</div>
      </>
    );
  }
  if (status.state === "waitingForConfirmation") {
    return <WaitingHelp safetyNumber={status.safetyNumber} />;
  }
  if (status.state === "confirmAdmission") {
    return <AdmissionCheck projectId={props.project.id} status={status} busy={props.busy} />;
  }
  const line = statusLine(status, t);
  const off = status.state === "off" || status.state === "removed";
  return (
    <div className="team-window-center">
      {line !== null && (
        <p className={`team-status tone-${line.tone}`} role="status">
          <span className="team-status-mark" aria-hidden="true" />
          <span>{line.text}</span>
        </p>
      )}
      {refusedForSeat(props.project, status) && <NoSeatNote />}
      {off && <p className="team-window-lead">{t("teamWindow.notShared")}</p>}
      {off && canToggle(status) && (
        <ConnectButton
          projectId={props.project.id}
          projectName={props.project.name}
          busy={props.busy}
          onProblem={setProblem}
        />
      )}
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </div>
  );
}

function ReloadButton({ busy, label }: { busy: boolean; label: string }) {
  return (
    <button
      type="button"
      className="link-button team-reload"
      disabled={busy}
      onClick={() => {
        void loadProjects();
      }}
    >
      {label}
    </button>
  );
}
