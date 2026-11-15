/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  awaitedAgent,
  BUBBLE_MIN_MS,
  BubbleThrottle,
  CREW_COLUMN_MIN_PANE,
  crewForm,
  crewOf,
  crewShown,
  crewState,
  crewWatchWanted,
  gateWaitingSince,
  DONE_LINGER_MS,
  MAX_CREW,
  QUIET_SHOWN_MS,
  waitsForDecision,
} from "./crew";
import type { SubAgent } from "./subAgents";

const NOW = 1_000_000_000;

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

  it("picks the unfinished sub-agent with the newest activity as the awaited one", () => {
    const list = [
      agent("a", { lastActivity: NOW - 5000 }),
      agent("b", { lastActivity: NOW - 1000 }),
      agent("c", { status: "finished", lastActivity: NOW }),
    ];
    expect(awaitedAgent(list, NOW)).toBe("b");
    expect(awaitedAgent([agent("c", { status: "finished" })], NOW)).toBeNull();
    expect(awaitedAgent([], NOW)).toBeNull();
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
  it("keeps start order and marks the awaited one only when the session waits", () => {
    const list = [agent("a"), agent("b", { lastActivity: NOW - 10 })];
    expect(crewOf(list, NOW, null).entries.map((e) => e.state)).toEqual(["working", "working"]);
    // Both wrote before the gate began to wait; the one that wrote last is the awaited one.
    expect(crewOf(list, NOW, NOW).entries.map((e) => e.state)).toEqual(["working", "waiting"]);
  });

  it("does not mark a sub-agent that writes since the gate began to wait", () => {
    const list = [agent("a", { lastActivity: NOW - 1000 })];
    expect(crewOf(list, NOW, NOW - 5000).entries[0]?.state).toBe("working");
  });

  it("marks a sub-agent that has been quiet since the gate began to wait", () => {
    const list = [
      agent("a", { lastActivity: NOW - 20_000 }),
      agent("b", { lastActivity: NOW - 500 }),
    ];
    expect(crewOf(list, NOW, NOW - 5000).entries.map((e) => e.state)).toEqual([
      "waiting",
      "working",
    ]);
  });

  it("takes the oldest pending gate decision of the session as the start of waiting", () => {
    const pending = [
      { sessionId: 1, createdAt: 300 },
      { sessionId: 2, createdAt: 100 },
      { sessionId: 1, createdAt: 200 },
    ];
    expect(gateWaitingSince(pending, 1)).toBe(200);
    expect(gateWaitingSince(pending, 3)).toBeNull();
  });

  it("leaves out finished ones after their linger", () => {
    const list = [agent("a"), agent("b", { status: "finished", lastActivity: NOW - 60_000 })];
    expect(crewOf(list, NOW, null).entries.map((e) => e.agent.id)).toEqual(["a"]);
  });

  it("caps the crew, keeps the waiting and working ones and counts the rest", () => {
    const list = Array.from({ length: MAX_CREW + 3 }, (_, i) =>
      agent(`a${String(i)}`, i < 3 ? { status: "finished", lastActivity: NOW - 100 } : {}),
    );
    const crew = crewOf(list, NOW, null);
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
    const some = crewOf([agent("a")], NOW, null);
    const none = crewOf([], NOW, null);
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
