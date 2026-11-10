import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  agentTitle,
  countsText,
  applySnapshot,
  elapsedMs,
  EMPTY_VIEW,
  featuredAgents,
  formatElapsed,
  nextViewPeak,
  parseSubAgentEvent,
  planSubAgentPanes,
  statusCounts,
  subAgentLayout,
  subAgentTree,
  TOOL_HISTORY,
  TREE_ROWS,
  watchedSession,
  withoutExcerpts,
  type SubAgent,
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

describe("applySnapshot", () => {
  it("replaces the agents and remembers earlier tool names, newest first", () => {
    let view = applySnapshot(EMPTY_VIEW, snapshot([agent("a", "running", { tool: "Glob" })]));
    expect(view.located).toBe(true);
    expect(view.history.get("a")).toEqual([]);
    view = applySnapshot(view, snapshot([agent("a", "running", { tool: "Grep" })]));
    view = applySnapshot(view, snapshot([agent("a", "running", { tool: "Grep" })]));
    view = applySnapshot(view, snapshot([agent("a", "running", { tool: "Read" })]));
    expect(view.history.get("a")).toEqual(["Grep", "Glob"]);
  });

  it("keeps the history short and forgets agents that are gone", () => {
    let view = EMPTY_VIEW;
    for (const tool of ["T1", "T2", "T3", "T4", "T5", "T6", "T7"]) {
      view = applySnapshot(view, snapshot([agent("a", "running", { tool })]));
    }
    expect(view.history.get("a")).toEqual(["T6", "T5", "T4", "T3"]);
    expect(view.history.get("a")).toHaveLength(TOOL_HISTORY);
    view = applySnapshot(view, snapshot([agent("b")]));
    expect(view.history.has("a")).toBe(false);
  });

  it("remembers the most agents seen, so the layout never shrinks", () => {
    let view = applySnapshot(EMPTY_VIEW, snapshot([agent("a"), agent("b")]));
    view = applySnapshot(view, snapshot([agent("a")]));
    expect(view.peak).toBe(2);
  });
});

describe("planSubAgentPanes", () => {
  const viewWith = (count: number) =>
    applySnapshot(
      EMPTY_VIEW,
      snapshot(Array.from({ length: count }, (_, index) => agent(`a${String(index)}`))),
    );

  it("shows a notice beside the session while there is nothing to show", () => {
    expect(planSubAgentPanes(EMPTY_VIEW, true)).toEqual({
      layout: "pair",
      panes: [{ kind: "notice" }],
    });
    expect(planSubAgentPanes(viewWith(0), false).panes).toEqual([{ kind: "notice" }]);
    expect(planSubAgentPanes(viewWith(2), true).panes).toEqual([{ kind: "notice" }]);
  });

  it("gives up to three sub-agents a pane each", () => {
    expect(planSubAgentPanes(viewWith(1), false)).toEqual({
      layout: "pair",
      panes: [{ kind: "agent", id: "a0" }],
    });
    expect(planSubAgentPanes(viewWith(2), false)).toEqual({
      layout: "mainStack",
      panes: [
        { kind: "agent", id: "a0" },
        { kind: "agent", id: "a1" },
      ],
    });
    expect(planSubAgentPanes(viewWith(3), false).layout).toBe("quad");
  });

  it("shows two featured sub-agents and a list once there are more than zones", () => {
    const plan = planSubAgentPanes(viewWith(6), false);
    expect(plan.layout).toBe("quad");
    expect(plan.panes).toEqual([
      { kind: "agent", id: "a4" },
      { kind: "agent", id: "a5" },
      { kind: "list" },
    ]);
  });

  it("keeps the larger layout when the snapshot gets smaller", () => {
    const view = applySnapshot(viewWith(2), snapshot([agent("a0")]));
    expect(planSubAgentPanes(view, false)).toEqual({
      layout: "mainStack",
      panes: [{ kind: "agent", id: "a0" }, { kind: "empty" }],
    });
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

  it("puts the sub-agent picked in the sidebar first", () => {
    const agents = [agent("picked", "finished"), agent("a"), agent("b"), agent("c")];
    expect(featuredAgents(agents, 2, "picked").map((a) => a.id)).toEqual(["picked", "c"]);
    expect(featuredAgents(agents, 2, "gone").map((a) => a.id)).toEqual(["b", "c"]);
  });

  it("gives the picked sub-agent a pane of its own in the grid", () => {
    const agents = ["a", "b", "c", "d", "e"].map((id) => agent(id));
    const view = applySnapshot(EMPTY_VIEW, snapshot(agents));
    expect(planSubAgentPanes(view, false, "a").panes).toEqual([
      { kind: "agent", id: "a" },
      { kind: "agent", id: "e" },
      { kind: "list" },
    ]);
  });
});

describe("subAgentLayout", () => {
  it("gives the same layout as the full plan, from counts alone", () => {
    for (const count of [0, 1, 2, 3, 4, 7]) {
      for (const peak of [0, 2, 5]) {
        for (const notice of [false, true]) {
          const agents = Array.from({ length: count }, (_, index) => agent(`a${String(index)}`));
          const view = { ...applySnapshot(EMPTY_VIEW, snapshot(agents)), peak };
          expect(subAgentLayout(count, peak, notice)).toBe(planSubAgentPanes(view, notice).layout);
        }
      }
    }
  });
});

describe("withoutExcerpts", () => {
  it("drops every excerpt and keeps the rest", () => {
    const view = applySnapshot(EMPTY_VIEW, snapshot([agent("a"), agent("b")]));
    const quiet = withoutExcerpts(view);
    expect(quiet.agents.map((a) => a.excerpt)).toEqual([null, null]);
    expect(quiet.agents.map((a) => a.tool)).toEqual(["Grep", "Grep"]);
    expect(quiet.history).toBe(view.history);
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
});

describe("nextViewPeak", () => {
  it("grows while the view stays on one conversation and starts over otherwise", () => {
    const closed = { key: null, count: 0 };
    expect(nextViewPeak(closed, null, 7)).toBe(closed);
    const opened = nextViewPeak(closed, "1:a", 4);
    expect(opened).toEqual({ key: "1:a", count: 4 });
    expect(nextViewPeak(opened, "1:a", 2)).toBe(opened);
    expect(nextViewPeak(opened, "1:a", 5)).toEqual({ key: "1:a", count: 5 });
    expect(nextViewPeak(opened, "1:b", 1)).toEqual({ key: "1:b", count: 1 });
    expect(nextViewPeak(opened, null, 4)).toEqual(closed);
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
