import { useId, useState } from "react";

import { useT } from "./i18n";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import {
  divergentText,
  folderLabel,
  legacyLinks,
  needsRelink,
  personName,
  repoChoices,
  sessionStateText,
  shortId,
  teamErrorText,
  type PeerSession,
  type PeerView,
  type RepoRef,
  type TeamActivity,
  type TeamProblem,
} from "./team";
import { linkKey, team } from "./useTeam";

/** Files of one teammate session listed before "show more". */
const FILE_PAGE = 20;

/**
 * What a connected shared project shows below its switch: which repository folders this device
 * shares with the team (a checkbox each; another clone or fork of the same repository is its own
 * folder), and who else is working, with their sessions' agent, title and state and the files
 * they change. Calm: no colors without text, nothing that blinks.
 */
export function TeamActivityView(props: {
  projectId: string;
  activity: TeamActivity;
  /** Repository folders of this device's running sessions. */
  repos: readonly RepoRef[];
  busy: ReadonlySet<string>;
}) {
  const t = useT();
  const reposId = useId();
  const peopleId = useId();
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const choices = repoChoices(props.activity.linked, props.repos);
  const relink = needsRelink(props.activity.linked, props.repos);
  const legacy = legacyLinks(props.activity.linked);

  return (
    <div className="team-activity">
      <section className="team-repos" aria-labelledby={reposId}>
        <h4 id={reposId} className="team-activity-title">
          {t("team.activity.repos")}
        </h4>
        {choices.length === 0 ? (
          <p className="team-help">{t("team.activity.noRepos")}</p>
        ) : (
          <ul className="team-repo-list">
            {choices.map(({ repo, linked }) => {
              const folder = repo.folder ?? "";
              const key = linkKey(props.projectId, repo.root, folder);
              const where = folderLabel(folder);
              return (
                <li key={`${repo.root}\u0000${folder}`}>
                  <label
                    className="team-repo"
                    title={revealInvisible(where === "" ? repo.name : where)}
                  >
                    <input
                      type="checkbox"
                      checked={linked}
                      disabled={props.busy.has(key)}
                      onChange={(event) => {
                        setProblem(null);
                        team
                          .linkRepo(props.projectId, repo.root, folder, event.currentTarget.checked)
                          .then(setProblem)
                          .catch(() => undefined);
                      }}
                    />
                    <span className="team-repo-name">
                      <Revealed text={repo.name} />
                    </span>
                    {where !== "" && (
                      <span className="team-repo-folder">
                        <Revealed text={where} />
                      </span>
                    )}
                  </label>
                </li>
              );
            })}
          </ul>
        )}
        {relink && <p className="team-help">{t("team.activity.relink")}</p>}
        {legacy.length > 0 && (
          <ul className="team-legacy-list">
            {legacy.map((repo) => (
              <li key={repo.root}>
                <button
                  type="button"
                  className="link-button"
                  title={repo.root}
                  disabled={props.busy.has(linkKey(props.projectId, repo.root, null))}
                  onClick={() => {
                    setProblem(null);
                    team
                      .linkRepo(props.projectId, repo.root, null, false)
                      .then(setProblem)
                      .catch(() => undefined);
                  }}
                >
                  {t("team.activity.removeLegacy", { name: revealInvisible(repo.name) })}
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="team-help">{t("team.activity.reposHelp")}</p>
        {problem !== null && (
          <p className="team-help error-text" role="alert">
            {teamErrorText(problem, t)}
          </p>
        )}
      </section>
      <section className="team-people" aria-labelledby={peopleId}>
        <h4 id={peopleId} className="team-activity-title">
          {t("team.activity.people")}
        </h4>
        {props.activity.divergent.map((device) => (
          <p
            key={`${device.member}\u0000${device.device}`}
            className="team-help error-text"
            role="status"
            title={`${device.member} · ${device.device}`}
          >
            {divergentText(device, t)}
          </p>
        ))}
        {props.activity.peers.length === 0 ? (
          <p className="team-help">{t("team.activity.nobody")}</p>
        ) : (
          <ul className="team-peer-list">
            {props.activity.peers.map((peer) => (
              <Peer key={`${peer.member}\u0000${peer.device}`} peer={peer} />
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function Peer({ peer }: { peer: PeerView }) {
  const t = useT();
  const name = personName(peer, t);
  // The name is whatever the teammate's device sends; the member id is the verified log's, so
  // it shows quietly next to any name a device chose itself.
  const named = !peer.own && peer.name.trim() !== "";
  return (
    <li className="team-peer">
      <div className="team-peer-head">
        <span className="team-peer-name" title={peer.member}>
          <Revealed text={name} />
        </span>
        {named && (
          <span className="team-peer-id" title={peer.member}>
            #{shortId(peer.member)}
          </span>
        )}
        {peer.presence === "away" && (
          <span className="team-peer-away">{t("team.activity.away")}</span>
        )}
      </div>
      {peer.sessions.length === 0 ? (
        <p className="team-help">{t("team.activity.noSessions")}</p>
      ) : (
        <ul className="team-session-list">
          {peer.sessions.map((session) => (
            <PeerSessionRow key={session.key} session={session} />
          ))}
        </ul>
      )}
    </li>
  );
}

function PeerSessionRow({ session }: { session: PeerSession }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [listed, setListed] = useState(FILE_PAGE);
  const count = session.files.length + session.moreFiles;
  const label = [session.agent, session.title].filter((part) => part.trim() !== "").join(" · ");
  return (
    <li className={`team-session state-${session.state}`}>
      <div className="team-session-head">
        <span className="team-session-mark" aria-hidden="true" />
        <span className="team-session-label" title={revealInvisible(label)}>
          <Revealed text={label} />
        </span>
        <span className="team-session-state">{sessionStateText(session.state, t)}</span>
      </div>
      {count > 0 && (
        <div className="team-session-files">
          <button
            type="button"
            className="claims-toggle"
            aria-expanded={open}
            onClick={() => {
              setOpen((value) => !value);
            }}
          >
            <span>{t("team.activity.files", { count })}</span>
          </button>
          {open && (
            <ul className="claims-files">
              {session.files.slice(0, listed).map((path) => (
                <li key={path} title={revealInvisible(path)}>
                  <span className="claims-path">
                    <Revealed text={path} />
                  </span>
                </li>
              ))}
            </ul>
          )}
          {open && session.files.length > listed && (
            <button
              type="button"
              className="link-button change-more files"
              onClick={() => {
                setListed((value) => value + FILE_PAGE);
              }}
            >
              {t("changes.moreFiles", { count: session.files.length - listed })}
            </button>
          )}
        </div>
      )}
    </li>
  );
}
