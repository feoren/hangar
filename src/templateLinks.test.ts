// Workspace templates that link their sessions (ADR 0014): the stored form, old and new, and how
// a start turns into one link request.

import { describe, expect, it } from "vitest";

import {
  EMPTY_LINKS,
  templateGroup,
  templateLinkRequest,
  templateMembers,
  type LinkGroup,
  type LinkState,
} from "./links";
import { parseTemplates, planTemplate, storeTemplate, type WorkspaceTemplate } from "./templates";

const claude = { profileId: "claude", folder: "C:\\work\\app", mode: "private" as const };
const codex = { profileId: "codex", folder: "C:\\work\\web", mode: "private" as const };

describe("templates with a link group", () => {
  it("reads an older template as one that links nothing", () => {
    const [old] = parseTemplates([{ id: "a", name: "Old", sessions: [claude, codex] }]);
    expect(old).toEqual({ id: "a", name: "Old", sessions: [claude, codex] });
    expect(old?.link).toBeUndefined();
    expect(old?.sessions[0]?.memberName).toBeUndefined();
  });

  it("keeps the link, ask first, names and roles of a new one", () => {
    const [crew] = parseTemplates([
      {
        id: "b",
        name: "Crew",
        link: true,
        askFirst: true,
        sessions: [
          { ...claude, memberName: "boss", role: "lead" },
          { ...codex, memberName: "web", role: "member" },
        ],
      },
    ]);
    expect(crew).toEqual({
      id: "b",
      name: "Crew",
      link: true,
      askFirst: true,
      sessions: [
        { ...claude, memberName: "boss", role: "lead" },
        { ...codex, memberName: "web", role: "member" },
      ],
    });
  });

  it("drops what the backend would refuse: bad or repeated names, a second lead", () => {
    const [crew] = parseTemplates([
      {
        id: "c",
        name: "Crew",
        link: "yes",
        askFirst: true,
        sessions: [
          { ...claude, memberName: "Boss", role: "lead" },
          { ...codex, memberName: "web", role: "lead" },
          { ...codex, memberName: "web", role: "chief" },
          { ...codex, memberName: "lead" },
        ],
      },
    ]);
    expect(crew?.link).toBeUndefined();
    // Without the link, ask first means nothing and is not kept.
    expect(crew?.askFirst).toBeUndefined();
    expect(crew?.sessions).toEqual([
      { ...claude, role: "lead" },
      { ...codex, memberName: "web" },
      codex,
      codex,
    ]);
  });

  it("stores the link only when asked to", () => {
    const sessions = [{ ...claude, memberName: "boss", role: "lead" as const }, codex];
    const [plain] = storeTemplate([], "Plain", sessions);
    expect(plain?.link).toBeUndefined();
    const stored = storeTemplate([], "Linked", sessions, { link: true, askFirst: false });
    const [linked] = stored;
    expect(linked?.link).toBe(true);
    expect(linked?.askFirst).toBeUndefined();
    expect(linked?.sessions[0]).toEqual({ ...claude, memberName: "boss", role: "lead" });
    // Saving again under the same name without the link takes it away.
    const replaced = storeTemplate(stored, "Linked", sessions, {
      link: false,
      askFirst: true,
    });
    expect(replaced[0]?.link).toBeUndefined();
    expect(replaced[0]?.askFirst).toBeUndefined();
  });

  it("carries names and roles into the planned starts", () => {
    const template: WorkspaceTemplate = {
      id: "t",
      name: "T",
      link: true,
      sessions: [{ ...claude, memberName: "boss", role: "lead" }, codex],
    };
    const profiles = [
      { id: "claude", teamCapable: true, cwd: null },
      { id: "codex", teamCapable: true, cwd: null },
    ];
    const plan = planTemplate(template, profiles, () => false);
    expect(plan.starts.map((start) => [start.memberName, start.role])).toEqual([
      ["boss", "lead"],
      [undefined, undefined],
    ]);
  });
});

describe("templateLinkRequest", () => {
  it("links the started agent sessions with their names and lead", () => {
    const request = templateLinkRequest(
      [
        { id: 7, memberName: "boss", role: "lead" },
        { id: 8 },
        // A shell: no send client, left out.
        { id: 9, memberName: "sh", role: "member" },
      ],
      new Set([7, 8]),
    );
    expect(request).toEqual({ ids: [7, 8], names: [{ session: 7, name: "boss" }], lead: 7 });
  });

  it("asks for nothing with fewer than two agent sessions", () => {
    expect(templateLinkRequest([{ id: 1 }, { id: 2 }], new Set([1]))).toBeNull();
    expect(templateLinkRequest([], new Set())).toBeNull();
  });

  it("keeps the first of a name or a lead given twice", () => {
    const request = templateLinkRequest(
      [
        { id: 1, memberName: "web", role: "lead" },
        { id: 2, memberName: "web", role: "lead" },
        { id: 3, memberName: "All" },
      ],
      new Set([1, 2, 3]),
    );
    expect(request).toEqual({ ids: [1, 2, 3], names: [{ session: 1, name: "web" }], lead: 1 });
  });
});

describe("templateMembers", () => {
  const group = (id: number, sessions: number[], askFirst = false): LinkGroup => ({
    id,
    color: 0,
    askFirst,
    braked: false,
    members: sessions.map((session, index) => ({
      session,
      name: `m${String(session)}`,
      role: index === 0 ? "lead" : "member",
    })),
  });
  const state = (groups: LinkGroup[]): LinkState => ({ ...EMPTY_LINKS, groups });

  it("takes the group with most of the chosen sessions", () => {
    const links = state([group(1, [1, 2]), group(2, [3, 4, 5], true)]);
    const members = templateMembers(links, [1, 2, 3, 4, 5]);
    expect([...members]).toEqual([
      [3, { memberName: "m3", role: "lead" }],
      [4, { memberName: "m4", role: "member" }],
      [5, { memberName: "m5", role: "member" }],
    ]);
    expect(templateGroup(links, [1, 2, 3, 4, 5])?.askFirst).toBe(true);
    // Only the chosen members of it.
    expect([...templateMembers(links, [1, 2, 3]).keys()]).toEqual([1, 2]);
  });

  it("gives none when no two chosen sessions share a group", () => {
    const links = state([group(1, [1, 2])]);
    expect(templateMembers(links, [1, 3]).size).toBe(0);
    expect(templateGroup(links, [1, 3])).toBeUndefined();
    expect(templateMembers(EMPTY_LINKS, [1, 2]).size).toBe(0);
  });
});
