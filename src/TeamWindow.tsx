import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { accountErrorText, isSignedIn, type Project } from "./account";
import { AccountStateSection } from "./AccountSettings";
import { useT, type MessageKey } from "./i18n";
import { CloseIcon } from "./icons";
import { nextTabIndex } from "./settingsSections";
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
  ConnectButton,
  ConnectList,
  PendingConfirmations,
  ProjectDetails,
  ReleaseRequests,
  WaitingHelp,
} from "./TeamProjects";
import { TeamTasksView, type TaskSession } from "./TeamTasksView";
import {
  anyShared,
  initialTab,
  shownProject,
  shownTab,
  teamTabs,
  type TeamTab,
} from "./teamOverview";
import { loadProjects, useAccount } from "./useAccount";
import { useTeam } from "./useTeam";

const TAB_NAMES: Record<TeamTab, MessageKey> = {
  now: "teamWindow.tab.now",
  people: "teamWindow.tab.people",
  requests: "teamWindow.tab.requests",
};

/** The project shown last while Hangar runs; the window opens on it again. */
let lastProject: string | null = null;

/**
 * The Team window: a floating panel over the terminals, not modal. The terminals stay visible and
 * keep running; Tab may leave the panel. It closes on Escape (with the focus inside), its close
 * button and a click outside; the caller then hands the focus back to the terminal.
 *
 * One state at a time: sign in, connect a first project, or the tabs "Now" (who works on what,
 * reservations), "People" (the project, sharing, repositories, invitations) and "Requests"
 * (only while something waits for an answer).
 */
export function TeamWindow(props: {
  /** This device's running sessions, for linking a reservation to one. */
  sessions: readonly TaskSession[];
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const tabsId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const tabButtons = useRef(new Map<TeamTab, HTMLButtonElement>());
  const snapshot = useAccount();
  const teamStore = useTeam();
  const { projects, projectsError, loadingProjects, view } = snapshot;
  const release = releaseRequests(teamStore.activities);
  const requestCount = teamStore.pending.length + release.length;
  const [chosenTab, setChosenTab] = useState<TeamTab>(() => initialTab(requestCount));
  const [chosenProject, setChosenProject] = useState<string | null>(null);
  const { onClose } = props;

  const projectIds = (projects ?? []).map((project) => project.id);
  const projectId = shownProject(projectIds, chosenProject, teamStore.statuses, lastProject);
  const project = projects?.find((entry) => entry.id === projectId) ?? null;
  const tab = shownTab(chosenTab, requestCount);
  const tabs = teamTabs(requestCount);
  const signedIn = isSignedIn(view);
  const showTabs =
    signedIn &&
    projects !== null &&
    projects.length > 0 &&
    anyShared(projectIds, teamStore.statuses);

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

  const selectTab = (next: TeamTab, focus: boolean) => {
    setChosenTab(next);
    if (focus) tabButtons.current.get(next)?.focus();
  };

  const onTabKey = (event: React.KeyboardEvent) => {
    const next = nextTabIndex(event.key, tabs.indexOf(tab), tabs.length);
    const target = next === null ? undefined : tabs[next];
    if (target === undefined) return;
    event.preventDefault();
    selectTab(target, true);
  };

  const focusNow = () => {
    const now = tabButtons.current.get("now");
    if (now?.isConnected === true) now.focus();
    else panel.current?.focus();
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
        <AccountStateSection view={view} titled={false} />
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
  } else if (projects.length === 0) {
    body = (
      <p className="team-help team-window-note">
        {t("team.empty")} <ReloadButton busy={loadingProjects} label={t("team.reload")} />
      </p>
    );
  } else if (!showTabs) {
    body = (
      <div className="team-window-connect">
        <p className="team-window-lead">{t("teamWindow.connectIntro")}</p>
        <ConnectList projects={projects} statuses={teamStore.statuses} busy={teamStore.busy} />
      </div>
    );
  } else if (project !== null) {
    const status = statusFor(teamStore.statuses, project.id);
    const activity = teamStore.activities.get(project.id) ?? EMPTY_ACTIVITY;
    switch (tab) {
      case "now":
        body = (
          <NowTab
            project={project}
            status={status}
            content={
              <>
                <TeamPeople activity={activity} />
                <TeamTasksView
                  projectId={project.id}
                  activity={activity}
                  sessions={props.sessions}
                  busy={teamStore.busy}
                />
              </>
            }
            busy={teamStore.busy.has(project.id)}
          />
        );
        break;
      case "people":
        body = (
          <ProjectDetails
            // A project of its own gets fresh state: no problem left over from another one.
            key={project.id}
            project={project}
            status={status}
            activity={activity}
            repos={teamStore.repos}
            busy={teamStore.busy}
          />
        );
        break;
      case "requests":
        body = (
          <div className="team-requests">
            <ReleaseRequests
              requests={release}
              projects={projects}
              busy={teamStore.busy}
              onLastAnswered={focusNow}
            />
            <PendingConfirmations
              pending={teamStore.pending}
              projects={projects}
              busy={teamStore.busy}
              onLastAnswered={focusNow}
            />
          </div>
        );
        break;
    }
  }

  return (
    <div
      ref={panel}
      className="dialog team-window"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        onClose();
      }}
    >
      <div className="dialog-header team-window-header">
        <h2 id={titleId}>{t("team.title")}</h2>
        {showTabs && project !== null && (
          <ProjectSwitch
            projects={projects}
            current={project}
            onChange={(id) => {
              setChosenProject(id);
            }}
          />
        )}
        <button
          type="button"
          className="icon-button team-window-close"
          aria-label={t("common.close")}
          title={t("common.close")}
          data-autofocus={showTabs ? undefined : true}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      {showTabs && (
        <div
          className="team-tabs"
          role="tablist"
          aria-label={t("teamWindow.tabs")}
          aria-orientation="horizontal"
          onKeyDown={onTabKey}
        >
          {tabs.map((id) => {
            const selected = id === tab;
            return (
              <button
                key={id}
                ref={(element) => {
                  if (element) tabButtons.current.set(id, element);
                  else tabButtons.current.delete(id);
                }}
                type="button"
                role="tab"
                id={`${tabsId}-${id}`}
                className="team-tab"
                aria-selected={selected}
                aria-controls={selected ? `${tabsId}-panel` : undefined}
                tabIndex={selected ? 0 : -1}
                data-autofocus={selected ? true : undefined}
                onClick={() => {
                  selectTab(id, false);
                }}
              >
                {t(TAB_NAMES[id])}
                {id === "requests" && (
                  <span className="team-tab-count">{requestCount > 9 ? "9+" : requestCount}</span>
                )}
              </button>
            );
          })}
        </div>
      )}
      <div
        className="team-window-body"
        {...(showTabs
          ? { role: "tabpanel", id: `${tabsId}-panel`, "aria-labelledby": `${tabsId}-${tab}` }
          : {})}
      >
        {body}
      </div>
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
      <span className="team-window-project" title={props.current.name}>
        {props.current.name}
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
          {project.name}
        </option>
      ))}
    </select>
  );
}

/**
 * "Now" of one project: while connected, who works on what and the reservations; otherwise what
 * the connection does, with the waiting help or "Connect" where that helps.
 */
function NowTab(props: {
  project: Project;
  status: TeamStatus;
  content: ReactNode;
  busy: boolean;
}) {
  const t = useT();
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const { status } = props;
  if (status.state === "connected") return <div className="team-now">{props.content}</div>;
  if (status.state === "waitingForConfirmation") {
    return <WaitingHelp safetyNumber={status.safetyNumber} />;
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
