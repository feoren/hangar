import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { Project, OverviewSession } from "./projectCards";
import { ProjectCard, ProjectsEmpty, type ProjectRow } from "./ProjectOverview";
import { EMPTY_ACTIVITY, type TeamActivity } from "./team";

const project: Project<OverviewSession> = {
  key: "e:\\work\\hangar",
  root: "E:\\work\\hangar",
  name: "hangar",
  repo: true,
  sessions: [{ id: 1, cwd: "E:\\work\\hangar" }],
  workTrees: [{ path: "E:\\work\\hangar", sessionIds: [1] }],
};

const rows = new Map<number, ProjectRow>([
  [1, { title: "Claude Code", badge: null, edge: "edge-working", status: "arbeitet" }],
]);

const shared: TeamActivity = {
  ...EMPTY_ACTIVITY,
  tasks: [
    {
      task: "t1",
      title: "Login umbauen",
      area: null,
      member: "anna",
      device: "d",
      name: "Anna",
      mine: false,
      own: false,
      since: 0,
      expiresAt: null,
      expiring: false,
      request: null,
    },
  ],
  sessions: [1],
};

function card(
  activities: ReadonlyMap<string, TeamActivity>,
  devServers: ReadonlyMap<number, string>,
): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator("de")}>
      <ProjectCard
        project={project}
        refresh={0}
        rows={rows}
        activeId={1}
        devServers={devServers}
        finished={new Map()}
        activities={activities}
        language="de"
        onActivate={() => undefined}
        onOpenDevServer={() => undefined}
        onShowChanges={() => undefined}
      />
    </I18nContext.Provider>,
  );
}

describe("ProjectCard", () => {
  it("shows name, path and the sessions with their state in words", () => {
    const html = card(new Map(), new Map());
    expect(html).toContain("hangar");
    expect(html).toContain("E:\\work\\hangar");
    expect(html).toContain("Claude Code");
    expect(html).toContain("arbeitet");
    expect(html).toContain('aria-current="true"');
  });

  it("hides the team and dev server blocks when there is nothing to show", () => {
    const html = card(new Map(), new Map());
    expect(html).not.toContain("project-team");
    expect(html).not.toContain("project-servers");
  });

  it("shows the reservations of a shared project", () => {
    const html = card(new Map([["p", shared]]), new Map());
    expect(html).toContain("project-team");
    expect(html).toContain("Login umbauen");
    expect(html).toContain("Anna");
  });

  it("offers the dev server with its address in words", () => {
    const html = card(new Map(), new Map([[1, "http://localhost:5173/"]]));
    expect(html).toContain("localhost:5173");
    expect(html).toContain("http://localhost:5173/ im Browser öffnen");
  });
});

describe("ProjectsEmpty", () => {
  it("shows the empty state without sessions", () => {
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={createTranslator("de")}>
        <ProjectsEmpty onNewSession={() => undefined} />
      </I18nContext.Provider>,
    );
    expect(html).toContain("Noch kein Projekt offen");
    expect(html).toContain("Neue Session");
  });

  it("leaves the start button out when no program can start", () => {
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={createTranslator("de")}>
        <ProjectsEmpty onNewSession={null} />
      </I18nContext.Provider>,
    );
    expect(html).not.toContain("<button");
  });
});
