import { useState } from "react";

import { accountErrorText, projectMeta, roleText, type AccountProblem } from "./account";
import { useT } from "./i18n";
import { account, loadProjects, type AccountSnapshot } from "./useAccount";

/**
 * Body of the sidebar's Team group: the account's projects with role, plan, seats (per account
 * of the owner) and pending invitations, and "Invite people" for owners and admins. Shown only
 * while signed in.
 */
export function TeamProjects({ snapshot }: { snapshot: AccountSnapshot }) {
  const t = useT();
  const [problem, setProblem] = useState<AccountProblem | null>(null);
  const { projects, projectsError, loadingProjects } = snapshot;

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

  if (projectsError !== null) {
    return (
      <p className="hint group-hint team-hint" role="status">
        {projectsError.kind === "sessionEnded"
          ? t("account.sessionEnded")
          : accountErrorText(projectsError, t)}{" "}
        {projectsError.kind !== "sessionEnded" && reload}
      </p>
    );
  }
  if (projects === null) {
    return <p className="hint group-hint team-hint">{t("team.loading")}</p>;
  }
  if (projects.length === 0) {
    return (
      <p className="hint group-hint team-hint">
        {t("team.empty")} {reload}
      </p>
    );
  }
  return (
    <>
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
      {problem !== null && (
        <p className="hint group-hint error-text" role="alert">
          {accountErrorText(problem, t)}
        </p>
      )}
    </>
  );
}
