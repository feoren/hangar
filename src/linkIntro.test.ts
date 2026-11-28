import { describe, expect, it } from "vitest";

import { introKind, introSummary, type IntroSession } from "./linkIntro";
import { EMPTY_LINKS, type LinkState } from "./links";

const state: LinkState = {
  ...EMPTY_LINKS,
  groups: [
    {
      id: 7,
      color: 0,
      askFirst: false,
      braked: false,
      brake: "none",
      budgetLeft: 20,
      budget: 20,
      pendingIntroductions: [],
      members: [
        { session: 1, name: "hangar", role: "member" },
        { session: 2, name: "website", role: "lead" },
        { session: 3, name: "server", role: "member" },
      ],
    },
  ],
};

const sessions: IntroSession[] = [
  {
    id: 1,
    title: "ich habe so viele Agenten nun installiert",
    cwd: "E:\\hangar",
    profileId: "claude",
  },
  { id: 2, title: "Codex CLI (2)", cwd: "E:\\hangar.feoren.com", profileId: "codex" },
  { id: 3, title: "Fix the relay", cwd: "/srv/hangar-server", profileId: "claude" },
];

const names: Record<string, string> = { claude: "Claude Code", codex: "Codex CLI" };
const programName = (id: string) => names[id];

const INTRO =
  'You are linked with other agents in Hangar. They work in parallel in their own sessions. Your member name is "hangar".';

describe("introKind", () => {
  it("tells the first introduction from the one after a lead change", () => {
    expect(introKind(INTRO)).toBe("linked");
    expect(introKind("The roles in your link group in Hangar changed. The other agents")).toBe(
      "rolesChanged",
    );
    // The backend no longer tells members that others joined; such a text reads as the first.
    expect(introKind('Your link group in Hangar grew: "docs" joined.')).toBe("linked");
  });
});

describe("introSummary", () => {
  it("lists the other members with a readable name, program and folder", () => {
    const summary = introSummary(state, { group: 7, to: 1, text: INTRO }, sessions, programName);
    expect(summary).toEqual({
      kind: "linked",
      leads: false,
      members: [
        {
          session: 2,
          // The project says which session it is, as on its row.
          label: "hangar.feoren.com",
          detail: "hangar.feoren.com · Codex CLI",
          profileId: "codex",
          lead: true,
        },
        {
          session: 3,
          // A name the person gave does not hide the project.
          label: "hangar-server",
          detail: "hangar-server · Claude Code",
          profileId: "claude",
          lead: false,
        },
      ],
    });
  });

  it("says when the session itself leads", () => {
    const summary = introSummary(state, { group: 7, to: 2, text: INTRO }, sessions, programName);
    expect(summary?.leads).toBe(true);
    expect(summary?.members.map((member) => member.session)).toEqual([1, 3]);
  });

  it("keeps titles where two members would otherwise show the same folder", () => {
    const twins: IntroSession[] = [
      ...sessions,
      { id: 4, title: "Codex CLI", cwd: "E:\\hangar.feoren.com", profileId: "codex" },
    ];
    const grown: LinkState = {
      ...state,
      groups: state.groups.map((group) => ({
        ...group,
        members: [...group.members, { session: 4, name: "codex", role: "member" as const }],
      })),
    };
    const summary = introSummary(grown, { group: 7, to: 1, text: INTRO }, twins, programName);
    expect(summary?.members.map((member) => member.label)).toEqual([
      "Codex CLI (2)",
      "hangar-server",
      "Codex CLI",
    ]);
    expect(summary?.members[0]?.detail).toBe("Codex CLI (2) · Codex CLI · hangar.feoren.com");
  });

  it("falls back to the member name for an unknown session and is null without the group", () => {
    const summary = introSummary(state, { group: 7, to: 1, text: INTRO }, [], programName);
    expect(summary?.members[0]).toEqual({
      session: 2,
      label: "website",
      detail: "website",
      profileId: "",
      lead: true,
    });
    expect(introSummary(state, { group: 9, to: 1, text: INTRO }, sessions, programName)).toBeNull();
  });
});
