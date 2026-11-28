import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";

import { api } from "./api";
import { PersonAvatar } from "./Avatar";
import { commitAge, commitDate } from "./gitHistory";
import { useT, type Language } from "./i18n";
import { canBackUp, driveMarkKey, type DriveView } from "./hangarDrive";
import { CheckIcon, GlobeIcon, RestartIcon } from "./icons";
import {
  GIT_CALLS_AT_ONCE,
  TASKS_SHOWN,
  changesTarget,
  createLimiter,
  groupProjects,
  loadProjectGit,
  parseRepoPlaces,
  placePaths,
  projectDevServers,
  projectTeam,
  projectToken,
  totalChanges,
  type OverviewSession,
  type Project,
  type ProjectGit,
  type RepoPlace,
} from "./projectCards";
import type { PartnerBook } from "./partners";
import { ProjectPartners, type PartnerActions } from "./ProjectPartners";
import { teammateNames, teammateStack } from "./repoPeers";
import { Revealed } from "./Revealed";
import { revealInvisible } from "./invisible";
import { taskTitle, type TaskView, type TeamActivity } from "./team";
import { useHangarDrive } from "./useHangarDrive";
import { useTeam } from "./useTeam";

/** A session row as the sidebar would show it: badge, state color and the state in words. */
export interface ProjectRow {
  title: string;
  badge: ReactNode;
  /** CSS class of the state's color (`edge-*`, sessionChip.ts). */
  edge: string;
  status: string;
}

interface Props {
  sessions: readonly OverviewSession[];
  rows: ReadonlyMap<number, ProjectRow>;
  activeId: number | null;
  devServers: ReadonlyMap<number, string>;
  /** How often each session finished a stretch of work; a card asks git again when it grows. */
  finished: ReadonlyMap<number, number>;
  language: Language;
  onActivate: (id: number) => void;
  onOpenDevServer: (id: number) => void;
  onShowChanges: (id: number) => void;
  /** Starts a session from the empty state; `null` when no program can start. */
  onNewSession: (() => void) | null;
  /** Hands a project folder to Hangar Drive (ADR 0024), which asks the person to confirm. */
  onBackUp: (folder: string) => void;
  /** The partner projects (ADR 0012), kept per card. */
  partners: PartnerBook;
  partnerActions: PartnerActions;
  /**
   * A project chosen in quick access (`Project.key`): its card scrolls into view and its name
   * takes the focus, once the card is there; a new `token` does it again.
   */
  focus?: { key: string; token: number } | null;
}

const MAX_AVATARS = 3;

/** One queue for every card: a long list never starts more than a few git processes at once. */
const limitGit = createLimiter(GIT_CALLS_AT_ONCE);

const NO_PLACES: ReadonlyMap<string, RepoPlace | null> = new Map();

/**
 * Project overview in the home view: one card per project folder of the sessions. Git is asked
 * when the view opens, on "Refresh" and when a session of a card finished its work, each card
 * only once it scrolled into view. No polling of its own.
 */
export function ProjectOverview(props: Props) {
  const { sessions } = props;
  const t = useT();
  const id = useId();
  const [refresh, setRefresh] = useState(0);
  const [places, setPlaces] = useState<ReadonlyMap<string, RepoPlace | null> | null>(null);
  const { activities } = useTeam();

  const paths = placePaths(sessions);
  const pathsKey = paths.join("\n");
  const pathsRef = useRef(paths);
  useEffect(() => {
    pathsRef.current = paths;
  });

  // Repository places when the view opens, the folders change or on "Refresh"; no process.
  useEffect(() => {
    const asked = pathsRef.current;
    if (asked.length === 0) {
      setPlaces(NO_PLACES);
      return;
    }
    let alive = true;
    void api
      .gitRepoPlaces([...asked])
      .then(parseRepoPlaces, () => new Map<string, RepoPlace | null>())
      .then((next) => {
        if (alive) setPlaces(next);
      });
    return () => {
      alive = false;
    };
  }, [pathsKey, refresh]);

  const projects = useMemo(
    () => (places === null ? [] : groupProjects(sessions, places)),
    [sessions, places],
  );
  const drive = useHangarDrive(projects.map((project) => project.root));

  if (!sessions.some((session) => session.kind !== "update" && session.cwd.trim() !== "")) {
    return <ProjectsEmpty onNewSession={props.onNewSession} />;
  }

  return (
    <section className="projects" aria-labelledby={`${id}-title`}>
      <div className="projects-head">
        <h2 id={`${id}-title`} tabIndex={-1} className="projects-title">
          {t("projects.title")}
        </h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("projects.refresh")}
          title={t("projects.refresh")}
          onClick={() => {
            setRefresh((value) => value + 1);
          }}
        >
          <RestartIcon />
        </button>
      </div>
      <div className="projects-grid">
        {projects.map((project) => (
          <ProjectCard
            key={project.key}
            project={project}
            refresh={refresh}
            activities={activities}
            rows={props.rows}
            activeId={props.activeId}
            devServers={props.devServers}
            finished={props.finished}
            language={props.language}
            onActivate={props.onActivate}
            onOpenDevServer={props.onOpenDevServer}
            onShowChanges={props.onShowChanges}
            drive={drive}
            onBackUp={props.onBackUp}
            partners={props.partners}
            partnerActions={props.partnerActions}
            focusToken={props.focus?.key === project.key ? props.focus.token : null}
          />
        ))}
      </div>
    </section>
  );
}

/** No project yet: what to do instead, with a start button when a program can start. */
export function ProjectsEmpty({ onNewSession }: { onNewSession: (() => void) | null }) {
  const t = useT();
  const id = useId();
  return (
    <section className="projects projects-empty" aria-labelledby={`${id}-title`}>
      <h2 id={`${id}-title`} tabIndex={-1} className="projects-empty-title">
        {t("projects.emptyTitle")}
      </h2>
      <p>{t("projects.emptyHint")}</p>
      {onNewSession !== null && (
        <button type="button" className="primary-button" onClick={onNewSession}>
          {t("menu.newSession")}
        </button>
      )}
    </section>
  );
}

/** Whether the element has come into view once; without IntersectionObserver at once. */
function useSeen(element: React.RefObject<HTMLElement | null>): boolean {
  const [seen, setSeen] = useState(() => typeof IntersectionObserver === "undefined");
  useEffect(() => {
    const target = element.current;
    if (seen || target === null) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setSeen(true);
    });
    observer.observe(target);
    return () => {
      observer.disconnect();
    };
  }, [element, seen]);
  return seen;
}

type CardProps = Pick<
  Props,
  | "rows"
  | "activeId"
  | "devServers"
  | "finished"
  | "language"
  | "onActivate"
  | "onOpenDevServer"
  | "onShowChanges"
  | "onBackUp"
  | "partners"
  | "partnerActions"
> & {
  project: Project<OverviewSession>;
  /** Grows with "Refresh": the card asks git again. */
  refresh: number;
  /** Hangar Drive's marks for the cards; nothing shows while it cannot take a folder. */
  drive: DriveView;
  /** The team store's projects; empty without a team. */
  activities: ReadonlyMap<string, TeamActivity>;
  /** Grows when quick access asks for this card: it scrolls into view, its name takes the focus. */
  focusToken?: number | null;
};

/** One project: its sessions, then what git, the team and the dev servers add; each block only when it has something. */
export function ProjectCard({
  project,
  refresh,
  rows,
  activeId,
  devServers,
  finished,
  activities,
  language,
  onActivate,
  onOpenDevServer,
  onShowChanges,
  drive,
  onBackUp,
  partners,
  partnerActions,
  focusToken = null,
}: CardProps) {
  const t = useT();
  const id = useId();
  const box = useRef<HTMLElement>(null);
  const name = useRef<HTMLHeadingElement>(null);
  const shownFor = useRef<number | null>(null);
  useEffect(() => {
    if (focusToken === null || shownFor.current === focusToken) return;
    shownFor.current = focusToken;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    box.current?.scrollIntoView({ block: "nearest", behavior: reduced ? "auto" : "smooth" });
    name.current?.focus({ preventScroll: true });
  }, [focusToken]);
  const seen = useSeen(box);
  /** With the time git answered: the commit ages count from then, not from each render. */
  const [git, setGit] = useState<(ProjectGit & { at: number }) | null>(null);

  const sessionIds = project.sessions.map((session) => session.id);
  const token = projectToken(sessionIds, finished);
  const treesKey = project.workTrees.map((tree) => tree.path).join("\n");
  const projectRef = useRef(project);
  useEffect(() => {
    projectRef.current = project;
  });

  useEffect(() => {
    const current = projectRef.current;
    if (!seen || !current.repo) return;
    let alive = true;
    // A card that left the view or asked again while it waited starts no git process.
    limitGit(() =>
      alive
        ? loadProjectGit(current, { changedCount: api.gitChangedCount, log: api.gitLog })
        : Promise.resolve(null),
    ).then(
      (next) => {
        if (alive && next !== null) setGit({ ...next, at: Date.now() });
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [seen, project.repo, project.root, treesKey, token, refresh]);

  const team = projectTeam(activities, sessionIds);
  const servers = projectDevServers(sessionIds, devServers);
  const changes = git === null ? null : totalChanges(git.changes);
  const target = git === null ? null : changesTarget(project, git.changes);
  return (
    <section className="project-card" ref={box} aria-labelledby={`${id}-name`}>
      <header className="project-head">
        <h3 id={`${id}-name`} ref={name} tabIndex={-1} className="project-name">
          {project.name}
        </h3>
        <span className="project-path" title={project.root}>
          {project.root}
        </span>
      </header>

      <ul className="project-sessions">
        {project.sessions.map((session) => {
          const row = rows.get(session.id);
          if (row === undefined) return null;
          return (
            <li key={session.id}>
              <button
                type="button"
                className={`project-session ${row.edge}`}
                aria-current={session.id === activeId ? "true" : undefined}
                onClick={() => {
                  onActivate(session.id);
                }}
              >
                <span className="project-dot" aria-hidden="true" />
                {row.badge}
                <span className="project-session-name">{row.title}</span>
                <span className="project-session-status">{row.status}</span>
              </button>
            </li>
          );
        })}
      </ul>

      {changes !== null &&
        (target === null ? (
          <p className="project-changes">{t("projects.changesNone")}</p>
        ) : (
          <button
            type="button"
            className="project-changes project-link"
            title={t("projects.showChanges")}
            onClick={() => {
              onShowChanges(target);
            }}
          >
            {changes === 1
              ? t("projects.changesOne")
              : t("projects.changesMany", { count: changes.toLocaleString(language) })}
          </button>
        ))}

      {git !== null && git.commits.length > 0 && (
        <div className="project-block">
          <h4 className="project-label">{t("projects.commits")}</h4>
          <ol className="project-commits">
            {git.commits.map((commit) => (
              <li
                key={commit.sha}
                title={`${commit.shortSha} · ${commit.author} · ${commitDate(commit.authoredAt, language)}`}
              >
                <span className="project-commit-subject">
                  <Revealed text={commit.subject} />
                </span>
                <span className="project-commit-age">
                  {commitAge(commit.authoredAt, git.at, language)}
                </span>
              </li>
            ))}
          </ol>
        </div>
      )}

      {team !== null && (
        <div className="project-block project-team">
          <h4 className="project-label">{t("projects.team")}</h4>
          {team.teammates.length > 0 && <Teammates people={team.teammates} />}
          {team.tasks.length > 0 && (
            <ul className="project-tasks" aria-label={t("projects.reservations")}>
              {team.tasks.slice(0, TASKS_SHOWN).map((task) => (
                <li key={task.task}>
                  <span className="project-task-title">{taskTitle(task.title, t)}</span>
                  <span className="project-task-holder">
                    {task.mine || task.own ? t("team.tasks.you") : holderName(task)}
                  </span>
                </li>
              ))}
              {team.tasks.length > TASKS_SHOWN && (
                <li className="project-more">
                  {t("projects.moreTasks", { count: team.tasks.length - TASKS_SHOWN })}
                </li>
              )}
            </ul>
          )}
        </div>
      )}

      {servers.length > 0 && (
        <div className="project-servers">
          {servers.map((server) => (
            <button
              key={server.url}
              type="button"
              className="project-server"
              aria-label={t("session.openDevServer", { url: server.url })}
              title={t("session.openDevServer", { url: server.url })}
              onClick={() => {
                onOpenDevServer(server.sessionId);
              }}
            >
              <GlobeIcon size={12} />
              <span>{serverLabel(server.url)}</span>
            </button>
          ))}
        </div>
      )}

      <ProjectPartners root={project.root} book={partners} {...partnerActions} />

      <DriveLine root={project.root} drive={drive} onBackUp={onBackUp} />
    </section>
  );
}

/**
 * The card's last line while Hangar Drive can take a folder: the quiet mark of the folder that
 * holds the project, or the way to back it up (one hand-over per click). Nothing for a folder
 * Hangar Drive does not take.
 */
export function DriveLine({
  root,
  drive,
  onBackUp,
}: {
  root: string;
  drive: DriveView;
  onBackUp: (folder: string) => void;
}) {
  const t = useT();
  const state = drive.ready ? drive.marks.get(root) : undefined;
  if (state !== undefined) {
    return (
      <p className="project-drive">
        <CheckIcon size={12} />
        <span>{t(driveMarkKey(state))}</span>
      </p>
    );
  }
  if (!canBackUp(drive, root)) return null;
  return (
    <button
      type="button"
      className="project-drive project-link"
      onClick={() => {
        onBackUp(root);
      }}
    >
      {t("drive.backUp")}
    </button>
  );
}

/** The holder as the team names them: the name their device sent, else the member id. */
function holderName(task: TaskView): string {
  const name = revealInvisible(task.name.trim());
  return name === "" ? task.member : name;
}

/** "localhost:5173" for "http://localhost:5173/": the scheme and slash say nothing here. */
function serverLabel(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

function Teammates({ people }: { people: Parameters<typeof teammateNames>[0] }) {
  const t = useT();
  const { shown, more } = teammateStack(people, MAX_AVATARS);
  const names = teammateNames(people);
  return (
    <p className="project-teammates" title={names}>
      <span className="project-avatars" aria-hidden="true">
        {shown.map((person) => (
          <PersonAvatar
            key={person.member}
            name={person.name}
            presence={person.presence}
            size="mini"
          />
        ))}
        {more > 0 && <span className="session-teammates-more">+{more}</span>}
      </span>
      <span className="project-teammate-names">{t("projects.teammates", { names })}</span>
    </p>
  );
}
