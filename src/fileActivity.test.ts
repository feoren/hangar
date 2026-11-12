import { describe, expect, it } from "vitest";

import type { Claim, ClaimsSnapshot, SessionClaims } from "./claims";
import {
  EMPTY_LOG,
  KEEP_MS,
  MAX_EVENTS,
  MAX_PROJECTS,
  MAX_SEEN,
  MAX_STORED_CHARS,
  PLAY_STEPS,
  appendEvents,
  heatLevel,
  heatOf,
  holderKey,
  localObservations,
  observe,
  otherWorktrees,
  parseLog,
  projectKeyOf,
  prune,
  serializeLog,
  sinceOf,
  stepTime,
  teamObservations,
  windowStart,
  type ActivityEvent,
  type EventLog,
  type LocalSession,
  type SeenEntry,
} from "./fileActivity";
import { EMPTY_ACTIVITY, type PeerView, type TeamActivity } from "./team";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

const claim = (path: string, start = 1, end = 1): Claim => ({
  path,
  kind: "modified",
  wholeFile: false,
  ranges: [{ start, end }],
  area: null,
});

function session(
  sessionId: number,
  root: string | null,
  claims: Claim[],
  state: SessionClaims["state"] = { kind: "ready" },
): SessionClaims {
  return {
    sessionId,
    state,
    root,
    base: null,
    sharedWith: [],
    claims,
    truncated: false,
    level: "none",
    overlapCount: 0,
    overlapsTruncated: false,
  };
}

const snapshot = (...sessions: SessionClaims[]): ClaimsSnapshot => ({
  enabled: true,
  sessions,
  overlaps: [],
});

const local = (id: number, repo: string | null = null): LocalSession => ({
  id,
  title: `Claude ${String(id)}`,
  program: "Claude Code",
  repo,
});

const event = (path: string, at: number, team = false): ActivityEvent => ({ path, at, team });

describe("projects and holders", () => {
  it("puts a Hangar worktree under its repository and spells folders as keys", () => {
    expect(projectKeyOf("C:\\src\\Shop", "C:/wt/shop-x")).toBe("c:/src/shop");
    expect(projectKeyOf(null, "C:/src/Shop/")).toBe("c:/src/shop");
    expect(projectKeyOf(null, null)).toBeNull();
    expect(holderKey({ kind: "worktree", root: "C:\\wt\\X" })).toBe("w:c:/wt/x");
    expect(holderKey({ kind: "team", member: "m", device: "d", session: "s" })).toBe("t:m\nd\ns");
  });

  it("finds the other worktrees of the active session's repository", () => {
    const claims = snapshot(
      session(1, "C:/src/shop", [claim("a.ts")]),
      session(2, "C:/src/shop", [claim("a.ts")]),
      session(3, "C:/wt/shop-x", [claim("b.ts")]),
      session(4, "C:/wt/shop-x", [claim("b.ts")]),
      session(5, "C:/wt/shop-y", [], { kind: "pending" }),
      session(6, "C:/other", [claim("c.ts")]),
      session(7, "C:/wt/shop-z", []),
    );
    const sessions = [
      local(1),
      local(2),
      local(3, "C:\\src\\shop"),
      local(4, "C:\\src\\shop"),
      local(5, "C:\\src\\shop"),
      local(6),
      local(7, "C:\\src\\shop"),
    ];
    const others = otherWorktrees(claims, sessions, 1);
    expect(others).toEqual([{ root: "C:/wt/shop-x", sessions: [3, 4], claims: [claim("b.ts")] }]);
    // Seen from the worktree, the main checkout is the other one.
    expect(otherWorktrees(claims, sessions, 3).map((o) => [o.root, o.sessions])).toEqual([
      ["C:/src/shop", [1, 2]],
    ]);
    expect(otherWorktrees(claims, sessions, 6)).toEqual([]);
    expect(otherWorktrees(claims, sessions, 99)).toEqual([]);
  });
});

describe("observing", () => {
  it("counts a new hold and a change of its lines, not a repeat", () => {
    const sessions = [local(1)];
    const first = localObservations(snapshot(session(1, "/r", [claim("a.ts")])), sessions);
    const one = observe(new Map(), "w:", first.observed, first.keep, 1000);
    expect(one.events).toEqual([{ project: "/r", event: event("a.ts", 1000) }]);

    const same = localObservations(snapshot(session(1, "/r", [claim("a.ts")])), sessions);
    const two = observe(one.seen, "w:", same.observed, same.keep, 2000);
    expect(two.events).toEqual([]);

    const more = localObservations(snapshot(session(1, "/r", [claim("a.ts", 1, 4)])), sessions);
    const three = observe(two.seen, "w:", more.observed, more.keep, 3000);
    expect(three.events).toEqual([{ project: "/r", event: event("a.ts", 3000) }]);
    // The hold began with the first sighting.
    expect(sinceOf(three.seen, "/r", { kind: "worktree", root: "/r" }, "a.ts")).toBe(1000);
  });

  it("counts sessions sharing a worktree once and forgets holds that ended", () => {
    const sessions = [local(1), local(2)];
    const both = localObservations(
      snapshot(session(1, "/r", [claim("a.ts")]), session(2, "/r", [claim("a.ts")])),
      sessions,
    );
    const one = observe(new Map(), "w:", both.observed, both.keep, 1000);
    expect(one.events).toHaveLength(1);
    const gone = observe(one.seen, "w:", [], new Set(), 2000);
    expect(gone.seen.size).toBe(0);
  });

  it("keeps the holds of a session still reading its changes", () => {
    const sessions = [local(1)];
    const ready = localObservations(snapshot(session(1, "/r", [claim("a.ts")])), sessions);
    const one = observe(new Map(), "w:", ready.observed, ready.keep, 1000);
    const pending = localObservations(
      snapshot(session(1, "/r", [], { kind: "pending" })),
      sessions,
    );
    const two = observe(one.seen, "w:", pending.observed, pending.keep, 2000);
    expect(two.seen.size).toBe(1);
    const back = observe(two.seen, "w:", ready.observed, ready.keep, 3000);
    expect(back.events).toEqual([]);
  });

  it("leaves the other kind of holder alone", () => {
    const teamHold: SeenEntry = {
      project: "/r",
      holder: "t:m\nd\ns",
      path: "x.ts",
      signature: "",
      since: 5,
    };
    const seen = new Map([["/r\u0000t:m\nd\ns\u0000x.ts", teamHold]]);
    const result = observe(seen, "w:", [], new Set(), 1000);
    expect([...result.seen.values()]).toEqual([teamHold]);
  });

  it("observes teammates' files under the project of the session beside them", () => {
    const repo = "a".repeat(40);
    const ann: PeerView = {
      member: "ann",
      device: "laptop",
      name: "Ann",
      own: false,
      presence: "active",
      sessions: [
        {
          key: "k1",
          agent: "Codex",
          title: "",
          state: "working",
          repo,
          files: ["src/x.ts"],
          moreFiles: 0,
        },
      ],
    };
    const activity: TeamActivity = {
      ...EMPTY_ACTIVITY,
      peers: [ann],
      sessions: [3],
      sessionRepos: [{ sessionId: 3, repo }],
    };
    const observed = teamObservations(
      new Map([["p", activity]]),
      snapshot(session(3, "C:/wt/shop-x", [])),
      [local(3, "C:/src/shop")],
    );
    expect(observed).toEqual([
      { project: "c:/src/shop", holder: "t:ann\nlaptop\nk1", path: "src/x.ts", signature: "" },
    ]);
    const result = observe(new Map(), "t:", observed, new Set(), 1000);
    expect(result.events).toEqual([
      { project: "c:/src/shop", event: { path: "src/x.ts", at: 1000, team: true } },
    ]);
  });
});

describe("the log", () => {
  const now = 10 * KEEP_MS;

  it("returns the same log when nothing happened", () => {
    expect(appendEvents(EMPTY_LOG, [], EMPTY_LOG.seen, now)).toBe(EMPTY_LOG);
  });

  it("drops old events and caps events, projects and holds", () => {
    const old = event("a.ts", now - KEEP_MS - 1);
    const many = Array.from({ length: MAX_EVENTS + 5 }, (_, index) => event("b.ts", now - index));
    const log: EventLog = {
      projects: new Map([["/p", [old, ...many.reverse()]]]),
      seen: new Map(),
    };
    const pruned = prune(log, now);
    const events = pruned.projects.get("/p") ?? [];
    expect(events).toHaveLength(MAX_EVENTS);
    expect(events.every((item) => item.path === "b.ts")).toBe(true);
    expect(events[events.length - 1]?.at).toBe(now);

    const projects = new Map(
      Array.from({ length: MAX_PROJECTS + 3 }, (_, index) => [
        `/p${String(index)}`,
        [event("x", now - index * MINUTE)],
      ]),
    );
    const seen = new Map(
      Array.from({ length: MAX_SEEN + 2 }, (_, index): [string, SeenEntry] => [
        `k${String(index)}`,
        { project: "/p", holder: "w:/p", path: String(index), signature: "", since: index },
      ]),
    );
    const capped = prune({ projects, seen }, now);
    expect([...capped.projects.keys()]).toHaveLength(MAX_PROJECTS);
    expect(capped.projects.has("/p0")).toBe(true);
    expect(capped.projects.has(`/p${String(MAX_PROJECTS + 2)}`)).toBe(false);
    expect(capped.seen.size).toBe(MAX_SEEN);
    expect(capped.seen.has("k0")).toBe(false);
  });

  it("sorts events back into time order", () => {
    const log = appendEvents(
      { projects: new Map([["/p", [event("a", now - 10)]]]), seen: new Map() },
      [{ project: "/p", event: event("b", now - 20) }],
      new Map(),
      now,
    );
    expect(log.projects.get("/p")?.map((item) => item.path)).toEqual(["b", "a"]);
  });

  it("stores and reads the log, rejecting anything malformed", () => {
    const at = Math.floor(now / 1000) * 1000;
    const log: EventLog = {
      projects: new Map([["c:/src/shop", [event("a.ts", at - 2000), event("a.ts", at, true)]]]),
      seen: new Map([
        [
          "c:/src/shop\u0000w:c:/src/shop\u0000a.ts",
          {
            project: "c:/src/shop",
            holder: "w:c:/src/shop",
            path: "a.ts",
            signature: "modified 1-1",
            since: at,
          },
        ],
      ]),
    };
    const text = serializeLog(log);
    expect(parseLog(text, now)).toEqual(log);
    expect(parseLog("{", now)).toBeNull();
    expect(parseLog(JSON.stringify({ v: 1, projects: {}, seen: {} }), now)).toBeNull();
    expect(parseLog(JSON.stringify({ v: 2, projects: {}, seen: [] }), now)).toBeNull();
    expect(
      parseLog(
        JSON.stringify({ v: 2, projects: { p: { paths: [], events: [[0, 1, 0]] } }, seen: {} }),
        now,
      ),
    ).toBeNull();
    expect(
      parseLog(JSON.stringify({ v: 2, projects: {}, seen: { p: [["x:", "a", "", 1]] } }), now),
    ).toBeNull();
  });

  it("keeps the stored log within its budget, the newest project first", () => {
    const at = Math.floor(now / 1000) * 1000;
    const path = (project: number, index: number) =>
      `p${String(project)}/${String(index).padStart(6, "0")}/`.padEnd(120, "x");
    const projects = new Map(
      Array.from({ length: MAX_PROJECTS }, (_, project): [string, ActivityEvent[]] => [
        `/project/${String(project)}`,
        Array.from({ length: MAX_EVENTS }, (_, index) =>
          // Project 0 is the newest; within a project the last event is the newest.
          event(path(project, index), at - project * HOUR - (MAX_EVENTS - index) * 1000),
        ),
      ]),
    );
    const seen = new Map(
      Array.from({ length: MAX_SEEN }, (_, index): [string, SeenEntry] => [
        `k${String(index)}`,
        {
          project: `/project/${String(index % MAX_PROJECTS)}`,
          holder: "w:/project",
          path: path(0, index),
          signature: "modified 1-1",
          since: at - index * 1000,
        },
      ]),
    );
    const text = serializeLog({ projects, seen });
    expect(text.length).toBeLessThanOrEqual(MAX_STORED_CHARS);
    const read = parseLog(text, now);
    expect(read?.projects.has("/project/0")).toBe(true);
    expect(read?.projects.has(`/project/${String(MAX_PROJECTS - 1)}`)).toBe(false);
    // The newest event of the newest project survives.
    expect(read?.projects.get("/project/0")?.at(-1)?.path).toBe(path(0, MAX_EVENTS - 1));

    // A single project too large keeps its newest events.
    const one = serializeLog(
      { projects: new Map([["/p", projects.get("/project/0") ?? []]]), seen: new Map() },
      50_000,
    );
    expect(one.length).toBeLessThanOrEqual(50_000);
    expect(parseLog(one, now)?.projects.get("/p")?.at(-1)?.path).toBe(path(0, MAX_EVENTS - 1));
    // A small log is stored whole.
    expect(parseLog(serializeLog({ projects: new Map(), seen: new Map() }), now)).toEqual(
      EMPTY_LOG,
    );
  });
});

describe("heat", () => {
  it("begins the windows an hour ago, at midnight and a week ago", () => {
    const now = new Date(2026, 10, 12, 15, 30).getTime();
    expect(windowStart("hour", now)).toBe(now - HOUR);
    expect(windowStart("today", now)).toBe(new Date(2026, 10, 12).getTime());
    expect(windowStart("week", now)).toBe(now - KEEP_MS);
  });

  it("splits a window into replay steps", () => {
    expect(stepTime(0, 2400, 0)).toBe(0);
    expect(stepTime(0, 2400, 12)).toBe(1200);
    expect(stepTime(0, 2400, PLAY_STEPS)).toBe(2400);
    expect(stepTime(0, 2400, PLAY_STEPS + 5)).toBe(2400);
    expect(stepTime(0, 2400, -1)).toBe(0);
  });

  it("counts changes per file and adds them up per folder within the range", () => {
    const events = [
      event("web/src/A.tsx", 100),
      event("web/src/a.tsx", 200),
      event("web/src/b.tsx", 300),
      event("api/main.rs", 300),
      event("web/readme.md", 400),
      event("web/src/b.tsx", 900),
    ];
    const heat = heatOf(events, 150, 400, "C:\\src\\shop\\web", "C:\\src\\shop");
    expect(heat.files.get("src/a.tsx")).toBe(1);
    expect(heat.files.get("src/b.tsx")).toBe(1);
    expect(heat.files.get("readme.md")).toBe(1);
    expect(heat.folders.get("src")).toBe(2);
    expect(heat.maxFile).toBe(1);
    expect(heat.maxFolder).toBe(2);
    // The file outside the tree's root counts in the total only.
    expect(heat.total).toBe(4);
    expect(heatOf(events, 0, 1000, "/x", null).total).toBe(0);
    expect(heatOf([], 0, 1000, "/r", "/r").maxFile).toBe(0);
  });

  it("grades counts on a square root scale", () => {
    expect(heatLevel(0, 10)).toBe(0);
    expect(heatLevel(3, 0)).toBe(0);
    expect(heatLevel(1, 100)).toBe(1);
    expect(heatLevel(25, 100)).toBe(2);
    expect(heatLevel(50, 100)).toBe(3);
    expect(heatLevel(100, 100)).toBe(4);
    expect(heatLevel(1, 1)).toBe(4);
  });
});
