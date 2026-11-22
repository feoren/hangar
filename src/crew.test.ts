/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  BUBBLE_MIN_MS,
  BubbleThrottle,
  CREW_COLUMN_MIN_PANE,
  crewForm,
  crewOf,
  crewShown,
  crewState,
  crewWatchWanted,
  waitingSubAgents,
  DONE_LINGER_MS,
  MAX_CREW,
  QUIET_SHOWN_MS,
  waitsForDecision,
} from "./crew";
import type { SubAgent } from "./subAgents";

const NOW = 1_000_000_000;

/** No pending decision names a sub-agent. */
const NOBODY: ReadonlySet<string> = new Set();

function agent(id: string, over: Partial<SubAgent> = {}): SubAgent {
  return {
    id,
    description: null,
    agentType: "tester",
    spawnDepth: 1,
    status: "running",
    startedAt: NOW - 60_000,
    lastActivity: NOW - 1000,
    tool: "Read",
    target: "C:\\src\\terminal.ts",
    note: null,
    tokens: null,
    excerpt: null,
    ...over,
  };
}

describe("state of a crew member", () => {
  it("maps the backend status", () => {
    expect(crewState(agent("a"), NOW, false)).toBe("working");
    expect(crewState(agent("a", { status: "finished" }), NOW, false)).toBe("done");
    expect(crewState(agent("a", { status: "stale" }), NOW, false)).toBe("quiet");
  });

  it("marks the awaited unfinished one as waiting, never a finished one", () => {
    expect(crewState(agent("a"), NOW, true)).toBe("waiting");
    expect(crewState(agent("a", { status: "stale" }), NOW, true)).toBe("waiting");
    expect(crewState(agent("a", { status: "finished" }), NOW, true)).toBe("done");
  });

  it("lets a finished one go after its linger and drops old quiet ones", () => {
    const fresh = agent("a", { status: "finished", lastActivity: NOW - DONE_LINGER_MS });
    const old = agent("a", { status: "finished", lastActivity: NOW - DONE_LINGER_MS - 1 });
    expect(crewState(fresh, NOW, false)).toBe("done");
    expect(crewState(old, NOW, false)).toBeNull();
    const history = agent("a", { status: "stale", lastActivity: NOW - QUIET_SHOWN_MS - 1 });
    expect(crewState(history, NOW, false)).toBeNull();
  });

  it("waits for a decision only for a gate while the session needs you, never for a bell", () => {
    expect(waitsForDecision("attention", "gate")).toBe(true);
    expect(waitsForDecision("attention", "bell")).toBe(false);
    expect(waitsForDecision("attention", "quiet")).toBe(false);
    expect(waitsForDecision("working", "gate")).toBe(false);
    expect(waitsForDecision(undefined, null)).toBe(false);
  });
});

describe("the crew of a session", () => {
  it("keeps start order and marks only a sub-agent a decision names", () => {
    const list = [agent("a"), agent("b", { lastActivity: NOW - 10 })];
    expect(crewOf(list, NOW, NOBODY).entries.map((e) => e.state)).toEqual(["working", "working"]);
    expect(crewOf(list, NOW, new Set(["b"])).entries.map((e) => e.state)).toEqual([
      "working",
      "waiting",
    ]);
  });

  it("collects the sub-agents the session's decisions name", () => {
    const pending = [
      { sessionId: 1, agents: [] },
      { sessionId: 1, agents: [{ id: "b" }, { id: "c" }] },
      { sessionId: 2, agents: [{ id: "x" }] },
    ];
    expect(waitingSubAgents(pending, 1)).toEqual(new Set(["b", "c"]));
    expect(waitingSubAgents(pending, 3)).toEqual(new Set());
  });

  it("marks nobody for a decision that names nobody, not even the sub-agent quiet since", () => {
    // "a" wrote last before the decision and has been quiet since: once guessed as the awaited
    // one, now the main agent's decision, which no figure stands for.
    const list = [
      agent("a", { lastActivity: NOW - 20_000 }),
      agent("b", { lastActivity: NOW - 500 }),
      agent("c", { status: "stale", lastActivity: NOW - 90_000 }),
    ];
    const pending = [{ sessionId: 1, agents: [] }];
    expect(crewOf(list, NOW, waitingSubAgents(pending, 1)).entries.map((e) => e.state)).toEqual([
      "working",
      "working",
      "quiet",
    ]);
  });

  it("marks exactly the named sub-agents, however they wrote", () => {
    const list = [
      agent("a", { lastActivity: NOW - 20_000 }),
      agent("b", { lastActivity: NOW - 100 }),
      agent("c", { status: "stale", lastActivity: NOW - 90_000 }),
      agent("d", { status: "finished", lastActivity: NOW - 100 }),
    ];
    const pending = [{ sessionId: 1, agents: [{ id: "b" }, { id: "c" }, { id: "d" }] }];
    // Several can wait at once; a finished one stays done.
    expect(crewOf(list, NOW, waitingSubAgents(pending, 1)).entries.map((e) => e.state)).toEqual([
      "working",
      "waiting",
      "waiting",
      "done",
    ]);
    // A name the watch does not know marks nobody.
    expect(crewOf(list, NOW, new Set(["gone"])).entries.some((e) => e.state === "waiting")).toBe(
      false,
    );
  });

  it("leaves out finished ones after their linger", () => {
    const list = [agent("a"), agent("b", { status: "finished", lastActivity: NOW - 60_000 })];
    expect(crewOf(list, NOW, NOBODY).entries.map((e) => e.agent.id)).toEqual(["a"]);
  });

  it("caps the crew, keeps the waiting and working ones and counts the rest", () => {
    const list = Array.from({ length: MAX_CREW + 3 }, (_, i) =>
      agent(`a${String(i)}`, i < 3 ? { status: "finished", lastActivity: NOW - 100 } : {}),
    );
    const crew = crewOf(list, NOW, NOBODY);
    expect(crew.entries).toHaveLength(MAX_CREW);
    expect(crew.more).toBe(3);
    expect(crew.entries.every((e) => e.state === "working")).toBe(true);
    // Still in start order.
    const ids = crew.entries.map((e) => e.agent.id);
    expect(ids).toEqual([...ids].sort());
  });
});

describe("visibility", () => {
  it("shows only when wanted and somebody stands in it", () => {
    const some = crewOf([agent("a")], NOW, NOBODY);
    const none = crewOf([], NOW, NOBODY);
    expect(crewShown(true, some)).toBe(true);
    expect(crewShown(true, none)).toBe(false);
    expect(crewShown(false, some)).toBe(false);
  });

  it("watches only for a session with sub-agents and the switch on", () => {
    expect(crewWatchWanted(true, true)).toBe(true);
    expect(crewWatchWanted(false, true)).toBe(false);
    expect(crewWatchWanted(true, false)).toBe(false);
  });

  it("uses the strip below the column's minimum width and the column from it", () => {
    expect(crewForm(CREW_COLUMN_MIN_PANE - 1)).toBe("strip");
    expect(crewForm(CREW_COLUMN_MIN_PANE)).toBe("column");
    expect(crewForm(320)).toBe("strip");
    expect(crewForm(null)).toBe("column");
  });
});

describe("bubble throttle", () => {
  const want = (text: string, state: "working" | "waiting" | "done" = "working") =>
    new Map([["a", { text, state }]]);

  it("shows the first text at once and holds back changes inside the minimum gap", () => {
    const throttle = new BubbleThrottle();
    expect(throttle.update(want("one"), 0).get("a")).toBe("one");
    expect(throttle.update(want("two"), 400).get("a")).toBe("one");
    expect(throttle.update(want("three"), BUBBLE_MIN_MS - 1).get("a")).toBe("one");
  });

  it("then shows the newest wanted text, not an intermediate one", () => {
    const throttle = new BubbleThrottle();
    throttle.update(want("one"), 0);
    throttle.update(want("two"), 300);
    throttle.update(want("three"), 600);
    expect(throttle.update(want("three"), BUBBLE_MIN_MS).get("a")).toBe("three");
  });

  it("restarts the gap after a change", () => {
    const throttle = new BubbleThrottle();
    throttle.update(want("one"), 0);
    throttle.update(want("two"), BUBBLE_MIN_MS);
    expect(throttle.update(want("three"), BUBBLE_MIN_MS + 100).get("a")).toBe("two");
    expect(throttle.update(want("three"), 2 * BUBBLE_MIN_MS).get("a")).toBe("three");
  });

  it("shows a change of state at once", () => {
    const throttle = new BubbleThrottle();
    throttle.update(want("Reading x"), 0);
    expect(throttle.update(want("I need your OK", "waiting"), 100).get("a")).toBe("I need your OK");
    expect(throttle.update(want("Done", "done"), 150).get("a")).toBe("Done");
  });

  it("is idempotent for the same time and forgets figures that left", () => {
    const throttle = new BubbleThrottle();
    throttle.update(want("one"), 0);
    expect(throttle.update(want("two"), 1500).get("a")).toBe("two");
    expect(throttle.update(want("two"), 1500).get("a")).toBe("two");
    expect(throttle.update(new Map(), 1600).size).toBe(0);
    expect(throttle.update(want("fresh"), 1700).get("a")).toBe("fresh");
  });
});

describe("colors of the plinths", () => {
  const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

  it("come from tokens that both themes define", () => {
    for (const token of ["--accent", "--warn", "--ok", "--muted"]) {
      expect(css).toContain(`${token}:`);
      expect(css).toContain(`.crew-item`);
    }
    expect(css).toMatch(/\.crew-item\.state-waiting \{[^}]*--warn/);
    expect(css).toMatch(/\.crew-item\.state-done \{[^}]*--ok/);
    expect(css).toMatch(/\.crew-item\.state-quiet \{[^}]*--muted/);
    // The theme blocks define these tokens for light and dark.
    const light = css.slice(css.indexOf(':root[data-theme="light"]'));
    expect(light).toMatch(/--ok:/);
    expect(light).toMatch(/--warn:/);
    expect(light).toMatch(/--accent:/);
  });

  it("change only colors, with a transition", () => {
    const plinth = /\.crew-plinth \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(plinth).toMatch(/transition:\s*background-color[^;]*,\s*border-color/);
    expect(plinth).not.toMatch(/animation|transform/);
  });
});
