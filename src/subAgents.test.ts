import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  agentTitle,
  applyLog,
  countsText,
  dismissedIn,
  dismissFinished,
  elapsedMs,
  EMPTY_LOG,
  featuredAgents,
  finishedIds,
  formatElapsed,
  logFor,
  MAX_LOG_LINES,
  parseSubAgentEvent,
  parseSubAgentLog,
  statusCounts,
  subAgentTree,
  TREE_ROWS,
  viewOfSnapshot,
  visibleAgents,
  watchedSession,
  withoutExcerpts,
  withoutLogText,
  type Dismissed,
  type LogBatch,
  type LogEntry,
  type SubAgent,
  type SubAgentLog,
  type SubAgentSnapshot,
  type SubAgentStatus,
} from "./subAgents";

const t = createTranslator("en");

function agent(id: string, status: SubAgentStatus = "running", patch: Partial<SubAgent> = {}) {
  return {
    id,
    description: `Task ${id}`,
    agentType: "Explore",
    spawnDepth: 1,
    status,
    startedAt: 1_000,
    lastActivity: 2_000,
    tool: "Grep",
    excerpt: "Looking at src/",
    ...patch,
  } satisfies SubAgent;
}

function snapshot(agents: SubAgent[], located = true): SubAgentSnapshot {
  return { located, agents };
}

describe("parseSubAgentEvent", () => {
  it("reads a snapshot as the backend sends it", () => {
    const wire = {
      type: "snapshot",
      located: true,
      agents: [
        {
          id: "a1",
          description: "Find the tests",
          agentType: "Explore",
          spawnDepth: 1,
          status: "running",
          startedAt: 1_700_000_000_000,
          lastActivity: 1_700_000_005_000,
          tool: "Grep",
          excerpt: "Searching",
        },
      ],
    };
    expect(parseSubAgentEvent(wire)).toEqual({
      located: true,
      agents: [{ ...wire.agents[0] }],
    });
  });

  it("drops broken entries and duplicates but keeps the rest", () => {
    const parsed = parseSubAgentEvent({
      type: "snapshot",
      located: true,
      agents: [
        { id: "a1", status: "finished" },
        { id: "a1", status: "running" },
        { id: "", status: "running" },
        { id: "a2", status: "lost" },
        null,
        { id: "a3", status: "stale", startedAt: -5, spawnDepth: 1.5, tool: "  ", excerpt: 7 },
      ],
    });
    expect(parsed?.agents.map((a) => a.id)).toEqual(["a1", "a3"]);
    expect(parsed?.agents[0]).toEqual({
      id: "a1",
      description: null,
      agentType: null,
      spawnDepth: null,
      status: "finished",
      startedAt: null,
      lastActivity: null,
      tool: null,
      excerpt: null,
    });
    expect(parsed?.agents[1]).toMatchObject({ startedAt: null, spawnDepth: null, tool: null });
  });

  it("cuts an excerpt that is longer than the backend promises", () => {
    const parsed = parseSubAgentEvent({
      type: "snapshot",
      located: true,
      agents: [{ id: "a", status: "running", excerpt: "x".repeat(1000) }],
    });
    expect(parsed?.agents[0]?.excerpt).toHaveLength(400);
  });

  it("ignores unknown messages", () => {
    expect(parseSubAgentEvent(null)).toBeNull();
    expect(parseSubAgentEvent([])).toBeNull();
    expect(parseSubAgentEvent({ type: "other", located: true, agents: [] })).toBeNull();
    expect(parseSubAgentEvent({ type: "snapshot", located: "yes", agents: [] })).toBeNull();
    expect(parseSubAgentEvent({ type: "snapshot", located: true })).toBeNull();
  });
});

describe("viewOfSnapshot", () => {
  it("replaces the agents with the snapshot's", () => {
    const view = viewOfSnapshot(snapshot([agent("a"), agent("b")]));
    expect(view.located).toBe(true);
    expect(view.agents.map((a) => a.id)).toEqual(["a", "b"]);
    expect(viewOfSnapshot(snapshot([], false))).toEqual({ located: false, agents: [] });
  });
});

describe("featuredAgents", () => {
  it("prefers running sub-agents, newer ones first, and shows them in start order", () => {
    const agents = [
      agent("old-running"),
      agent("done-1", "finished"),
      agent("new-running"),
      agent("done-2", "finished"),
    ];
    expect(featuredAgents(agents, 2).map((a) => a.id)).toEqual(["old-running", "new-running"]);
    expect(
      featuredAgents([agent("x", "finished"), agent("y", "stale")], 2).map((a) => a.id),
    ).toEqual(["x", "y"]);
  });

  it("puts the preferred sub-agent first", () => {
    const agents = [agent("picked", "finished"), agent("a"), agent("b"), agent("c")];
    expect(featuredAgents(agents, 2, "picked").map((a) => a.id)).toEqual(["picked", "c"]);
    expect(featuredAgents(agents, 2, "gone").map((a) => a.id)).toEqual(["b", "c"]);
  });
});

describe("withoutExcerpts", () => {
  it("drops every excerpt and keeps the rest", () => {
    const view = viewOfSnapshot(snapshot([agent("a"), agent("b")]));
    const quiet = withoutExcerpts(view);
    expect(quiet.agents.map((a) => a.excerpt)).toEqual([null, null]);
    expect(quiet.agents.map((a) => a.tool)).toEqual(["Grep", "Grep"]);
    expect(withoutExcerpts(quiet)).toBe(quiet);
  });
});

describe("subAgentTree", () => {
  it("lists every sub-agent in start order while they fit", () => {
    const agents = [agent("a", "finished"), agent("b")];
    expect(subAgentTree(agents)).toEqual({ agents, more: 0 });
    expect(subAgentTree([])).toEqual({ agents: [], more: 0 });
  });

  it("keeps running and newer ones and counts the rest", () => {
    const agents = [
      agent("old-running"),
      agent("f1", "finished"),
      agent("f2", "finished"),
      agent("f3", "stale"),
      agent("f4", "finished"),
    ];
    const tree = subAgentTree(agents, 3);
    expect(tree.agents.map((a) => a.id)).toEqual(["old-running", "f4"]);
    expect(tree.more).toBe(3);
    expect(subAgentTree(agents, TREE_ROWS).more).toBe(0);
  });

  it("keeps a row for the sub-agent the view shows", () => {
    const agents = ["a", "b", "c", "d"].map((id) => agent(id, "finished"));
    expect(subAgentTree(agents, 3, "a").agents.map((a) => a.id)).toEqual(["a", "d"]);
  });
});

describe("parseSubAgentLog", () => {
  it("reads a batch as the backend sends it", () => {
    const wire = {
      type: "log",
      agentId: "a1",
      reset: true,
      entries: [
        { kind: "prompt", text: "Find the parser" },
        { kind: "text", text: "Looking" },
        { kind: "tool", name: "Read", detail: "src/a.ts" },
        { kind: "tool", name: "TodoWrite", detail: null },
        { kind: "result", text: "line one", moreLines: 3, error: false },
        { kind: "result", text: null, moreLines: 0, error: true },
        { kind: "gap" },
      ],
    };
    expect(parseSubAgentLog(wire)).toEqual({
      agentId: "a1",
      reset: true,
      entries: wire.entries,
    });
  });

  it("drops broken entries, cuts long texts and ignores other messages", () => {
    const parsed = parseSubAgentLog({
      type: "log",
      agentId: "a1",
      reset: false,
      entries: [
        { kind: "prompt", text: "   " },
        { kind: "tool", name: 5 },
        { kind: "result", text: "r", moreLines: -2, error: "yes" },
        { kind: "html", text: "<b>x</b>" },
        null,
        { kind: "text", text: "y".repeat(9000) },
      ],
    });
    expect(parsed?.entries).toEqual([
      { kind: "result", text: "r", moreLines: 0, error: false },
      { kind: "text", text: "y".repeat(4000) },
    ]);
    expect(parseSubAgentLog({ type: "snapshot", located: true, agents: [] })).toBeNull();
    expect(parseSubAgentLog({ type: "log", agentId: "", reset: true, entries: [] })).toBeNull();
    expect(parseSubAgentLog({ type: "log", agentId: "a", reset: 1, entries: [] })).toBeNull();
    expect(parseSubAgentEvent({ type: "log", agentId: "a", reset: true, entries: [] })).toBeNull();
  });
});

describe("applyLog", () => {
  const say = (text: string): LogEntry => ({ kind: "text", text });
  const batch = (entries: LogEntry[], reset = false, agentId = "a"): LogBatch => ({
    agentId,
    reset,
    entries,
  });
  const texts = (log: SubAgentLog) =>
    log.lines.map((line) =>
      line.entry.kind === "gap" ? "gap" : "text" in line.entry ? line.entry.text : "",
    );

  it("appends batches of the shown sub-agent and starts over on a reset", () => {
    let log = logFor(EMPTY_LOG, "a");
    expect(log.loaded).toBe(false);
    log = applyLog(log, batch([say("one")], true));
    log = applyLog(log, batch([say("two")]));
    expect(log.loaded).toBe(true);
    expect(texts(log)).toEqual(["one", "two"]);
    const ids = log.lines.map((line) => line.id);
    expect(new Set(ids).size).toBe(2);
    log = applyLog(log, batch([say("fresh")], true));
    expect(texts(log)).toEqual(["fresh"]);
    expect(log.lines[0]?.id).not.toBe(ids[0]);
  });

  it("drops batches of another sub-agent and keeps one gap in a row", () => {
    const log = applyLog(logFor(EMPTY_LOG, "a"), batch([{ kind: "gap" }, say("x")], true));
    expect(applyLog(log, batch([say("other")], true, "b"))).toBe(log);
    expect(texts(applyLog(log, batch([say("y"), { kind: "gap" }, { kind: "gap" }])))).toEqual([
      "gap",
      "x",
      "y",
      "gap",
    ]);
  });

  it("waits for the reset after a switch, so appends of an earlier focus never show", () => {
    // Focus a, then b, then a again: an append of the first focus on a is still in flight.
    const again = logFor(
      logFor(applyLog(logFor(EMPTY_LOG, "a"), batch([say("old")], true)), "b"),
      "a",
    );
    expect(applyLog(again, batch([say("stale")]))).toBe(again);
    const fresh = applyLog(again, batch([say("new")], true));
    expect(texts(fresh)).toEqual(["new"]);
    expect(fresh.loaded).toBe(true);
  });

  it("keeps the newest lines and the task when it grows past the limit", () => {
    const task: LogEntry = { kind: "prompt", text: "task" };
    let log = applyLog(logFor(EMPTY_LOG, "a"), batch([task], true), 5);
    log = applyLog(log, batch(["1", "2", "3", "4", "5", "6"].map(say)), 5);
    expect(texts(log)).toEqual(["task", "gap", "4", "5", "6"]);
    log = applyLog(log, batch([say("7")]), 5);
    expect(texts(log)).toEqual(["task", "gap", "5", "6", "7"]);
    const plain = applyLog(logFor(EMPTY_LOG, "a"), batch(["1", "2", "3", "4"].map(say), true), 3);
    expect(texts(plain)).toEqual(["gap", "3", "4"]);
    expect(MAX_LOG_LINES).toBeGreaterThan(400);
  });

  it("switches sub-agents with an empty log and forgets texts on request", () => {
    const log = applyLog(logFor(EMPTY_LOG, "a"), batch([say("x")], true));
    expect(logFor(log, "a")).toBe(log);
    const other = logFor(log, "b");
    expect(other).toMatchObject({ agentId: "b", loaded: false, lines: [] });
    expect(other.next).toBe(log.next);
    expect(withoutLogText(log).lines).toEqual([]);
    expect(withoutLogText(other)).toBe(other);
  });
});

describe("dismissFinished", () => {
  const agents = [agent("run"), agent("done-1", "finished"), agent("done-2", "finished")];

  it("hides finished sub-agents per watch and never running ones", () => {
    const empty: Dismissed = new Map();
    const once = dismissFinished(empty, "1:a", agents, ["done-1", "run"]);
    expect([...dismissedIn(once, "1:a")]).toEqual(["done-1"]);
    expect(dismissedIn(once, "1:b").size).toBe(0);
    expect(dismissedIn(once, null).size).toBe(0);
    expect(dismissFinished(once, "1:a", agents, ["done-1", "run"])).toBe(once);
    const all = dismissFinished(once, "1:a", agents, finishedIds(agents));
    expect(visibleAgents(agents, dismissedIn(all, "1:a")).map((a) => a.id)).toEqual(["run"]);
    expect(dismissedIn(empty, "1:a").size).toBe(0);
  });

  it("returns the same list when nothing is hidden", () => {
    expect(visibleAgents(agents, new Set())).toBe(agents);
    expect(finishedIds(agents)).toEqual(["done-1", "done-2"]);
  });
});

describe("watchedSession", () => {
  it("watches the open view's session, else the active one while the tree shows", () => {
    expect(watchedSession(3, 5, false)).toBe(3);
    expect(watchedSession(null, 5, true)).toBe(5);
    expect(watchedSession(null, 5, false)).toBeNull();
    expect(watchedSession(null, null, true)).toBeNull();
  });
});

describe("times and names", () => {
  it("counts running time up to now and finished time up to the last activity", () => {
    expect(elapsedMs(agent("a", "running"), 61_000)).toBe(60_000);
    expect(elapsedMs(agent("a", "stale"), 61_000)).toBe(60_000);
    expect(elapsedMs(agent("a", "finished"), 61_000)).toBe(1_000);
    expect(elapsedMs(agent("a", "running", { startedAt: null }), 61_000)).toBeNull();
    expect(elapsedMs(agent("a", "running", { startedAt: 90_000 }), 61_000)).toBe(0);
  });

  it("formats durations the same way in every language", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(42_400)).toBe("0:42");
    expect(formatElapsed(725_000)).toBe("12:05");
    expect(formatElapsed(3_723_000)).toBe("1:02:03");
    expect(formatElapsed(Number.NaN)).toBe("0:00");
    expect(formatElapsed(-5)).toBe("0:00");
  });

  it("names a sub-agent by description, then type, then generically", () => {
    expect(agentTitle(agent("a"), t)).toBe("Task a");
    expect(agentTitle(agent("a", "running", { description: null }), t)).toBe("Explore");
    expect(agentTitle(agent("a", "running", { description: null, agentType: null }), t)).toBe(
      t("subAgents.untitled"),
    );
  });

  it("counts sub-agents per status", () => {
    expect(statusCounts([agent("a"), agent("b", "finished"), agent("c", "stale")])).toEqual({
      running: 1,
      finished: 1,
      stale: 1,
    });
  });

  it("sums up the list so the shown counts add up to the total", () => {
    expect(countsText([agent("a"), agent("b", "finished")], t)).toBe("running: 1 · finished: 1");
    const all = [agent("a"), agent("b", "finished"), agent("c", "stale"), agent("d", "stale")];
    expect(countsText(all, t)).toBe("running: 1 · finished: 1 · no activity: 2");
  });
});
