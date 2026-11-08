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
  parseSubAgentEvent,
  planSubAgentPanes,
  statusCounts,
  TOOL_HISTORY,
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
