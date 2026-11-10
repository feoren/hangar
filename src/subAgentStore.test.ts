import { describe, expect, it } from "vitest";

import {
  errorOf,
  IDLE_WATCH,
  SubAgentStore,
  viewOf,
  watchKey,
  type SubAgentWatchState,
} from "./subAgentStore";
import { EMPTY_VIEW } from "./subAgents";

function message(ids: string[], excerpt: string | null = "Looking at src/") {
  return {
    type: "snapshot",
    located: true,
    agents: ids.map((id) => ({ id, status: "running", tool: "Grep", excerpt })),
  };
}

/** A store and every state it reported to its listener. */
function watched() {
  const store = new SubAgentStore();
  const seen: SubAgentWatchState[] = [];
  const stop = store.subscribe(() => {
    seen.push(store.current());
  });
  return { store, seen, stop };
}

describe("SubAgentStore", () => {
  it("starts idle and reads as empty for every key", () => {
    const store = new SubAgentStore();
    expect(store.current()).toBe(IDLE_WATCH);
    expect(viewOf(store.current(), "1:a")).toBe(EMPTY_VIEW);
    expect(errorOf(store.current(), "1:a")).toBeNull();
  });

  it("applies snapshots of the current watch and tells its listeners", () => {
    const { store, seen } = watched();
    const key = watchKey(1, "a");
    store.retarget(key, true);
    store.receive(key, true, message(["x", "y"]));
    expect(seen).toHaveLength(2);
    expect(viewOf(store.current(), key).agents.map((a) => a.id)).toEqual(["x", "y"]);
    expect(viewOf(store.current(), watchKey(2, "a"))).toBe(EMPTY_VIEW);
  });

  it("drops messages of another watch, with other excerpts, or unknown ones", () => {
    const { store, seen } = watched();
    const key = watchKey(1, "a");
    store.retarget(key, false);
    store.receive(watchKey(1, "b"), false, message(["x"]));
    store.receive(key, true, message(["x"]));
    store.receive(key, false, { type: "other" });
    expect(seen).toHaveLength(1);
    expect(viewOf(store.current(), key).agents).toEqual([]);
  });

  it("starts empty for another session or conversation", () => {
    const store = new SubAgentStore();
    store.retarget("1:a", false);
    store.receive("1:a", false, message(["x"]));
    store.retarget("1:b", false);
    expect(store.current().view).toBe(EMPTY_VIEW);
    store.retarget(null, false);
    expect(store.current()).toBe(IDLE_WATCH);
  });

  it("keeps the agents when only excerpts switch and drops excerpts at once", () => {
    const store = new SubAgentStore();
    store.retarget("1:a", true);
    store.receive("1:a", true, message(["x"]));
    store.retarget("1:a", false);
    const view = viewOf(store.current(), "1:a");
    expect(view.agents.map((a) => [a.id, a.excerpt])).toEqual([["x", null]]);
    store.retarget("1:a", true);
    expect(viewOf(store.current(), "1:a")).toBe(view);
  });

  it("does not notify when nothing changed", () => {
    const { store, seen } = watched();
    store.retarget(null, false);
    store.retarget("1:a", false);
    store.retarget("1:a", false);
    store.fail("1:a", "refused");
    store.fail("1:a", "refused");
    store.fail("2:a", "other");
    expect(seen).toHaveLength(2);
    expect(errorOf(store.current(), "1:a")).toBe("refused");
    store.retarget("1:a", true);
    expect(errorOf(store.current(), "1:a")).toBeNull();
  });

  it("stops telling a listener that unsubscribed", () => {
    const { store, seen, stop } = watched();
    stop();
    store.retarget("1:a", false);
    expect(seen).toEqual([]);
  });
});
