import { describe, expect, it, vi } from "vitest";

import type { CommitSummary } from "./gitHistory";
import {
  COMMITS_SHOWN,
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
  type Project,
  type RepoPlace,
} from "./projectCards";
import { EMPTY_ACTIVITY, type PeerView, type TaskView, type TeamActivity } from "./team";

interface TestSession {
  id: number;
  cwd: string;
  kind?: "agent" | "update";
  worktree?: { path: string; repoRoot: string; branch: string };
}

const REPO = "E:\\work\\hangar";
const repoPlace: RepoPlace = { root: REPO, workTree: REPO };

describe("parseRepoPlaces", () => {
  it("keeps well-formed places and nulls, drops the rest", () => {
    const places = parseRepoPlaces({
      "E:\\a": { root: "E:\\a", workTree: "E:\\a" },
      "E:\\b": null,
      "E:\\c": { root: "", workTree: "E:\\c" },
      "E:\\d": { root: "E:\\d" },
      "E:\\e": "E:\\e",
    });
    expect([...places]).toEqual([
      ["E:\\a", { root: "E:\\a", workTree: "E:\\a" }],
      ["E:\\b", null],
    ]);
  });

  it("gives an empty map for anything that is no object", () => {
    expect(parseRepoPlaces(null).size).toBe(0);
    expect(parseRepoPlaces([]).size).toBe(0);
    expect(parseRepoPlaces("x").size).toBe(0);
  });

  it("drops places with invisible characters", () => {
    expect(parseRepoPlaces({ "E:\\a": { root: "E:\\a\u202e", workTree: "E:\\a" } }).size).toBe(0);
  });
});

describe("placePaths", () => {
  it("asks each working folder once, without update sessions or worktree sessions", () => {
    const sessions: TestSession[] = [
      { id: 1, cwd: REPO },
      { id: 2, cwd: REPO },
      { id: 3, cwd: "C:\\Users\\me", kind: "update" },
      {
        id: 4,
        cwd: "E:\\wt\\a",
        worktree: { path: "E:\\wt\\a", repoRoot: REPO, branch: "hangar/a" },
      },
      { id: 5, cwd: "" },
      { id: 6, cwd: "E:\\other" },
    ];
    expect(placePaths(sessions)).toEqual([REPO, "E:\\other"]);
  });
});

describe("groupProjects", () => {
  it("puts folders of one repository and its worktrees on one card", () => {
    const sessions: TestSession[] = [
      { id: 1, cwd: REPO },
      { id: 2, cwd: "E:\\notes" },
      { id: 3, cwd: `${REPO}\\src` },
      {
        id: 4,
        cwd: "E:\\wt\\hangar-1\\fix",
        worktree: {
          path: "E:\\wt\\hangar-1\\fix",
          repoRoot: "e:\\work\\HANGAR",
          branch: "hangar/f",
        },
      },
      { id: 5, cwd: "E:\\linked" },
    ];
    const places = new Map<string, RepoPlace | null>([
      [REPO, repoPlace],
      [`${REPO}\\src`, repoPlace],
      ["E:\\notes", null],
      ["E:\\linked", { root: REPO, workTree: "E:\\linked" }],
    ]);
    const projects = groupProjects(sessions, places);
    expect(projects.map((project) => project.name)).toEqual(["hangar", "notes"]);
    const [hangar, notes] = projects;
    expect(hangar?.root).toBe(REPO);
    expect(hangar?.repo).toBe(true);
    expect(hangar?.sessions.map((session) => session.id)).toEqual([1, 3, 4, 5]);
    expect(hangar?.workTrees).toEqual([
      { path: REPO, sessionIds: [1, 3] },
      { path: "E:\\wt\\hangar-1\\fix", sessionIds: [4] },
      { path: "E:\\linked", sessionIds: [5] },
    ]);
    expect(notes?.repo).toBe(false);
    expect(notes?.workTrees).toEqual([]);
  });

  it("leaves out update sessions and empty folders", () => {
    const sessions: TestSession[] = [
      { id: 1, cwd: "C:\\Users\\me", kind: "update" },
      { id: 2, cwd: " " },
    ];
    expect(groupProjects(sessions, new Map())).toEqual([]);
  });

  it("treats a folder without an answer yet as a plain folder", () => {
    const projects = groupProjects([{ id: 1, cwd: REPO }], new Map());
    expect(projects[0]?.repo).toBe(false);
    expect(projects[0]?.key).toBe("e:\\work\\hangar");
  });
});

function peer(member: string, own: boolean, repo: string, agent: string): PeerView {
  return {
    member,
    device: `${member}-device`,
    name: member,
    presence: "active",
    own,
    sessions: [
      { key: `${member}-1`, repo, agent, title: "", state: "working", files: [], moreFiles: 0 },
    ],
  };
}

function task(id: string, title: string, name: string): TaskView {
  return {
    task: id,
    title,
    area: null,
    member: name,
    device: "d",
    name,
    mine: false,
    own: false,
    since: 0,
    expiresAt: null,
    expiring: false,
    request: null,
  };
}

describe("projectTeam", () => {
  const activity: TeamActivity = {
    ...EMPTY_ACTIVITY,
    peers: [peer("anna", false, "root1", "Claude Code"), peer("me", true, "root1", "Codex")],
    tasks: [task("t1", "Login umbauen", "anna")],
    sessions: [1],
    sessionRepos: [{ sessionId: 1, repo: "root1" }],
  };

  it("is null without a team, so the block stays hidden", () => {
    expect(projectTeam(new Map(), [1])).toBeNull();
    expect(projectTeam(new Map([["p", EMPTY_ACTIVITY]]), [1])).toBeNull();
  });

  it("is null when none of the card's sessions is shared", () => {
    expect(projectTeam(new Map([["p", activity]]), [2])).toBeNull();
  });

  it("names the teammates and the reservations of the shared projects", () => {
    const team = projectTeam(new Map([["p", activity]]), [1, 2]);
    expect(team?.teammates.map((person) => person.name)).toEqual(["anna"]);
    expect(team?.tasks.map((entry) => entry.title)).toEqual(["Login umbauen"]);
  });

  it("lists a reservation once even when two projects carry it", () => {
    const team = projectTeam(
      new Map([
        ["p", activity],
        ["q", { ...activity, peers: [] }],
      ]),
      [1],
    );
    expect(team?.tasks).toHaveLength(1);
  });

  it("shows reservations without teammates", () => {
    const team = projectTeam(new Map([["p", { ...activity, peers: [] }]]), [1]);
    expect(team?.teammates).toEqual([]);
    expect(team?.tasks).toHaveLength(1);
  });
});

describe("projectDevServers", () => {
  it("lists each address once with the first session that printed it", () => {
    const servers = new Map([
      [3, "http://localhost:5173/"],
      [1, "http://localhost:5173/"],
      [2, "http://localhost:3000/"],
      [9, "http://localhost:8080/"],
    ]);
    expect(projectDevServers([1, 2, 3], servers)).toEqual([
      { url: "http://localhost:5173/", sessionId: 1 },
      { url: "http://localhost:3000/", sessionId: 2 },
    ]);
  });

  it("is empty without dev servers, so the block stays hidden", () => {
    expect(projectDevServers([1], new Map())).toEqual([]);
  });
});

describe("projectToken", () => {
  it("changes when one of the card's sessions finishes", () => {
    const before = projectToken([1, 2], new Map([[1, 1]]));
    expect(projectToken([1, 2], new Map([[1, 1]]))).toBe(before);
    expect(projectToken([1, 2], new Map([[1, 2]]))).not.toBe(before);
    expect(
      projectToken(
        [1, 2],
        new Map([
          [1, 1],
          [3, 5],
        ]),
      ),
    ).toBe(before);
  });
});

function commit(sha: string): CommitSummary {
  return {
    sha: sha.repeat(40).slice(0, 40),
    shortSha: sha,
    subject: `subject ${sha}`,
    author: "a",
    authoredAt: 0,
    pushed: true,
  };
}

const project: Project<{ id: number }> = {
  key: "e:\\work\\hangar",
  root: REPO,
  name: "hangar",
  repo: true,
  sessions: [{ id: 1 }, { id: 4 }],
  workTrees: [
    { path: REPO, sessionIds: [1] },
    { path: "E:\\wt\\fix", sessionIds: [4] },
  ],
};

describe("loadProjectGit", () => {
  it("counts the changes per work tree and takes the newest commits of the root", async () => {
    const changedCount = vi.fn((cwd: string) => Promise.resolve(cwd === REPO ? 2 : 3));
    const log = vi.fn(() =>
      Promise.resolve({
        isRepo: true,
        commits: ["a", "b", "c"].map(commit),
        upstream: null,
        aheadCount: 0,
        hasMore: true,
      }),
    );
    const git = await loadProjectGit(project, { changedCount, log });
    expect(log).toHaveBeenCalledWith(REPO, 0, COMMITS_SHOWN);
    expect(git.changes).toEqual([
      { path: REPO, count: 2 },
      { path: "E:\\wt\\fix", count: 3 },
    ]);
    expect(git.commits.map((entry) => entry.shortSha)).toEqual(["a", "b", "c"]);
    expect(totalChanges(git.changes)).toBe(5);
  });

  it("leaves out what git could not answer", async () => {
    const git = await loadProjectGit(project, {
      changedCount: (cwd) =>
        cwd === REPO ? Promise.reject(new Error("no git")) : Promise.resolve(1),
      log: () => Promise.resolve("garbage"),
    });
    expect(git.changes).toEqual([{ path: "E:\\wt\\fix", count: 1 }]);
    expect(git.commits).toEqual([]);
  });

  it("has no count at all when every work tree failed", async () => {
    const git = await loadProjectGit(project, {
      changedCount: () => Promise.reject(new Error("no git")),
      log: () => Promise.reject(new Error("no git")),
    });
    expect(totalChanges(git.changes)).toBeNull();
  });

  it("asks nothing for a plain folder", async () => {
    const changedCount = vi.fn(() => Promise.resolve(1));
    const log = vi.fn(() => Promise.resolve(null));
    const git = await loadProjectGit(
      { ...project, repo: false, workTrees: [] },
      { changedCount, log },
    );
    expect(changedCount).not.toHaveBeenCalled();
    expect(log).not.toHaveBeenCalled();
    expect(git).toEqual({ changes: [], commits: [] });
  });
});

describe("changesTarget", () => {
  it("opens the changes of the first work tree that has some", () => {
    expect(
      changesTarget(project, [
        { path: REPO, count: 0 },
        { path: "E:\\wt\\fix", count: 3 },
      ]),
    ).toBe(4);
  });

  it("is null without changes", () => {
    expect(changesTarget(project, [{ path: REPO, count: 0 }])).toBeNull();
  });
});

describe("createLimiter", () => {
  it("runs at most `max` tasks at once and all of them in the end", async () => {
    const limit = createLimiter(2);
    let running = 0;
    let peak = 0;
    const releases: (() => void)[] = [];
    const task = () =>
      limit(
        () =>
          new Promise<number>((resolve) => {
            running += 1;
            peak = Math.max(peak, running);
            releases.push(() => {
              running -= 1;
              resolve(running);
            });
          }),
      );
    const all = Promise.all([task(), task(), task(), task()]);
    await Promise.resolve();
    expect(running).toBe(2);
    while (releases.length > 0 || running > 0) {
      releases.shift()?.();
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    await all;
    expect(peak).toBe(2);
  });

  it("goes on after a failed task", async () => {
    const limit = createLimiter(1);
    const failed = limit(() => Promise.reject(new Error("x")));
    const next = limit(() => Promise.resolve(7));
    await expect(failed).rejects.toThrow("x");
    await expect(next).resolves.toBe(7);
  });

  it("goes on after a task that throws before it returns a promise", async () => {
    const limit = createLimiter(1);
    const failed = limit((): Promise<number> => {
      throw new Error("sync");
    });
    const next = limit(() => Promise.resolve(8));
    await expect(failed).rejects.toThrow("sync");
    await expect(next).resolves.toBe(8);
  });
});
