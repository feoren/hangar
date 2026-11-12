import { describe, expect, it } from "vitest";

import {
  chosenTitle,
  deleteTemplate,
  parseTemplates,
  planTemplate,
  renameProblem,
  renameTemplate,
  storeTemplate,
  TEMPLATE_LIMIT,
  TEMPLATE_NAME_LENGTH,
  TEMPLATE_SESSION_LIMIT,
  templateFolders,
  templateReport,
  templateSession,
  type WorkspaceTemplate,
} from "./templates";

const claude = { profileId: "claude", folder: "C:\\work\\app", mode: "team" as const };
const shell = { profileId: "pwsh", folder: "C:\\work\\app", mode: "private" as const };

describe("parseTemplates", () => {
  it("gives none for anything but a list", () => {
    expect(parseTemplates(undefined)).toEqual([]);
    expect(parseTemplates({ name: "x" })).toEqual([]);
    expect(parseTemplates("[]")).toEqual([]);
  });

  it("keeps valid templates and sessions, drops the rest", () => {
    expect(
      parseTemplates([
        {
          id: "a",
          name: "  Review  ",
          sessions: [
            { ...claude, title: "Reviewer" },
            { profileId: "pwsh", folder: "C:\\work\\app", mode: "weird" },
            { profileId: "", folder: "C:\\x" },
            { profileId: "pwsh", folder: "bad\u0000path" },
            null,
          ],
        },
        { id: "b", name: "review", sessions: [shell] },
        { id: "c", name: "Empty", sessions: [] },
        { id: "d", name: "", sessions: [shell] },
        { id: "e", name: "No list", sessions: "x" },
        42,
      ]),
    ).toEqual([
      {
        id: "a",
        name: "Review",
        sessions: [{ ...claude, title: "Reviewer" }, shell],
      },
    ]);
  });

  it("repairs duplicate or invalid ids and limits sizes", () => {
    const parsed = parseTemplates([
      { id: "same", name: "One", sessions: [shell] },
      { id: "same", name: "Two", sessions: [shell] },
      { id: "Bad ID", name: "Three", sessions: [shell] },
    ]);
    expect(new Set(parsed.map((template) => template.id)).size).toBe(3);
    const many = Array.from({ length: TEMPLATE_LIMIT + 3 }, (_, index) => ({
      name: `T${String(index)}`,
      sessions: Array.from({ length: TEMPLATE_SESSION_LIMIT + 2 }, () => shell),
    }));
    const limited = parseTemplates(many);
    expect(limited).toHaveLength(TEMPLATE_LIMIT);
    expect(limited[0]?.sessions).toHaveLength(TEMPLATE_SESSION_LIMIT);
    const long = parseTemplates([{ name: "n".repeat(200), sessions: [shell] }]);
    expect(long[0]?.name).toHaveLength(TEMPLATE_NAME_LENGTH);
  });
});

describe("store, rename, delete", () => {
  const list: WorkspaceTemplate[] = [{ id: "template-1", name: "Review", sessions: [claude] }];

  it("adds a new template last", () => {
    const next = storeTemplate(list, "Frontend", [shell]);
    expect(next.map((template) => template.name)).toEqual(["Review", "Frontend"]);
    expect(next[1]?.id).toBe("template-2");
  });

  it("replaces a template of the same name in place", () => {
    const next = storeTemplate(list, " review ", [shell]);
    expect(next).toEqual([{ id: "template-1", name: "review", sessions: [shell] }]);
  });

  it("ignores a blank name or no sessions", () => {
    expect(storeTemplate(list, "  ", [shell])).toEqual(list);
    expect(storeTemplate(list, "New", [])).toEqual(list);
  });

  it("renames unless the name is empty or taken", () => {
    const two = storeTemplate(list, "Frontend", [shell]);
    expect(renameProblem(two, "template-1", "")).toBe("empty");
    expect(renameProblem(two, "template-1", "frontend")).toBe("taken");
    expect(renameProblem(two, "template-1", "REVIEW")).toBeNull();
    expect(renameTemplate(two, "template-1", "Backend")[0]?.name).toBe("Backend");
    expect(renameTemplate(two, "template-1", "Frontend")).toEqual(two);
  });

  it("deletes by id", () => {
    expect(deleteTemplate(list, "template-1")).toEqual([]);
    expect(deleteTemplate(list, "nope")).toEqual(list);
  });
});

describe("templateSession", () => {
  const names = ["Claude Code", "claude"];

  it("remembers the repository of a team session and a chosen title", () => {
    expect(
      templateSession(
        {
          profileId: "claude",
          title: "API review",
          cwd: "C:\\work\\app\\.worktrees\\x",
          mode: "team",
          worktree: { repoRoot: "C:\\work\\app", branch: "hangar/x" },
        },
        names,
      ),
    ).toEqual({ ...claude, title: "API review" });
  });

  it("keeps a private session's folder and drops names Hangar gave", () => {
    expect(
      templateSession({ profileId: "claude", title: "Claude Code (2)", cwd: "D:\\p" }, names),
    ).toEqual({ profileId: "claude", folder: "D:\\p", mode: "private" });
    expect(
      chosenTitle(
        {
          profileId: "claude",
          title: "Claude Code · 1a2b3c",
          cwd: "x",
          worktree: { repoRoot: "x", branch: "hangar/claude-1a2b3c" },
        },
        names,
      ),
    ).toBeNull();
  });

  it("leaves update sessions out", () => {
    expect(
      templateSession({ profileId: "claude", title: "x", cwd: "D:\\p", kind: "update" }, names),
    ).toBeNull();
  });
});

describe("planTemplate", () => {
  const profiles = [
    { id: "claude", teamCapable: true, cwd: null },
    { id: "pwsh", teamCapable: false, cwd: null },
  ];
  const template: WorkspaceTemplate = {
    id: "t",
    name: "T",
    sessions: [
      { ...claude, title: "Lead" },
      { ...shell, mode: "team" },
      { profileId: "gone", folder: "C:\\x", mode: "private" },
      { profileId: "claude", folder: "C:\\plain", mode: "team" },
    ],
  };

  it("starts team sessions only where possible and names missing programs", () => {
    const plan = planTemplate(template, profiles, (folder) => folder === "C:\\work\\app");
    expect(plan.skipped).toEqual(["gone"]);
    expect(
      plan.starts.map((start) => [start.profile.id, start.folder, start.mode, start.title]),
    ).toEqual([
      ["claude", "C:\\work\\app", "team", "Lead"],
      ["pwsh", "C:\\work\\app", "private", undefined],
      ["claude", "C:\\plain", "private", undefined],
    ]);
  });

  it("sums up a start: all, some or none", () => {
    expect(templateReport(3, 3, [], null)).toEqual({ kind: "started" });
    expect(templateReport(3, 1, ["gone"], "boom")).toEqual({
      kind: "partial",
      started: 1,
      total: 3,
      lastError: "boom",
      skipped: ["gone"],
    });
    expect(templateReport(2, 0, [], "no git")).toEqual({
      kind: "partial",
      started: 0,
      total: 2,
      lastError: "no git",
      skipped: [],
    });
    expect(templateReport(2, 1, ["gone"], null)).toMatchObject({ kind: "partial", started: 1 });
  });

  it("lists each folder once", () => {
    expect(templateFolders(template)).toEqual(["C:\\work\\app", "C:\\x", "C:\\plain"]);
  });
});
