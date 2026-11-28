import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { NO_DRIVE, type DriveView } from "./hangarDrive";
import { createTranslator, I18nContext } from "./i18n";
import { EMPTY_BOOK, parsePartnerBook, type PartnerBook } from "./partners";
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

const NO_PARTNER_ACTIONS = {
  onLink: () => Promise.resolve(null),
  onUnlink: () => undefined,
  onPickFolder: () => Promise.resolve(null),
};

function card(
  activities: ReadonlyMap<string, TeamActivity>,
  devServers: ReadonlyMap<number, string>,
  partners: PartnerBook = EMPTY_BOOK,
  drive: DriveView = NO_DRIVE,
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
        drive={drive}
        onBackUp={() => undefined}
        partners={partners}
        partnerActions={NO_PARTNER_ACTIONS}
      />
    </I18nContext.Provider>,
  );
}

describe("ProjectCard", () => {
  it("shows nothing of Hangar Drive while it is not installed", () => {
    const html = card(new Map(), new Map());
    expect(html).not.toContain("Hangar Drive");
    expect(html).not.toContain("project-drive");
  });

  it("offers backing up a project Hangar Drive does not hold yet", () => {
    const html = card(new Map(), new Map(), EMPTY_BOOK, { installed: true, marks: new Map() });
    expect(html).toContain("Mit Hangar Drive sichern …");
    expect(html).toContain("<button");
  });

  it("marks a project Hangar Drive holds, with its state where it does not sync", () => {
    const marked = (state: "active" | "paused" | "stopped" | "other") =>
      card(new Map(), new Map(), EMPTY_BOOK, {
        installed: true,
        marks: new Map([["E:\\work\\hangar", state]]),
      });
    expect(marked("active")).toContain("In Hangar Drive<");
    expect(marked("active")).not.toContain("Mit Hangar Drive sichern");
    expect(marked("other")).toContain("In Hangar Drive<");
    expect(marked("paused")).toContain("In Hangar Drive · pausiert");
    expect(marked("stopped")).toContain("In Hangar Drive · gestoppt");
  });

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

describe("ProjectCard partners", () => {
  const t = createTranslator("de");

  it("offers to add a partner when there is none", () => {
    const html = card(new Map(), new Map());
    expect(html).toContain(t("partners.title"));
    expect(html).toContain(t("partners.add"));
    expect(html).not.toContain("project-partner-list");
  });

  it("lists the partners in both directions, each with a way to end the pair", () => {
    const book = parsePartnerBook({
      projects: [
        { name: "app", folder: "E:\\work\\hangar" },
        { name: "web", folder: "E:\\work\\site" },
      ],
      pairs: [["web", "app"]],
    });
    const html = card(new Map(), new Map(), book);
    expect(html).toContain("project-partner-list");
    expect(html).toContain(">web<");
    expect(html).toContain(t("partners.remove", { name: "web" }));
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
