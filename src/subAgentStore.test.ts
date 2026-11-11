import { afterEach, describe, expect, it, vi } from "vitest";

import {
  errorOf,
  FOCUS_DEBOUNCE_MS,
  FocusSender,
  IDLE_WATCH,
  logOf,
  SubAgentStore,
  viewOf,
  watchKey,
  type SubAgentWatchState,
} from "./subAgentStore";
import { EMPTY_LOG, EMPTY_VIEW } from "./subAgents";

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

  it("collects the log of the shown sub-agent only", () => {
    const store = new SubAgentStore();
    const key = watchKey(1, "a");
    const log = (agentId: string, reset: boolean, text: string) => ({
      type: "log",
      agentId,
      reset,
      entries: [{ kind: "text", text }],
    });
    store.retarget(key, true);
    store.refocus("x");
    store.receive(key, true, log("x", true, "one"));
    store.receive(key, true, log("x", false, "two"));
    store.receive(key, true, log("y", true, "other"));
    store.receive(watchKey(2, "a"), true, log("x", false, "elsewhere"));
    const shown = logOf(store.current(), key);
    expect(shown.agentId).toBe("x");
    expect(shown.lines.map((line) => line.entry)).toEqual([
      { kind: "text", text: "one" },
      { kind: "text", text: "two" },
    ]);
    expect(logOf(store.current(), watchKey(2, "a"))).toBe(EMPTY_LOG);
    // A snapshot leaves the log alone.
    store.receive(key, true, message(["x"]));
    expect(logOf(store.current(), key)).toBe(shown);
    // Another sub-agent starts empty.
    store.refocus("y");
    expect(logOf(store.current(), key)).toMatchObject({ agentId: "y", lines: [] });
  });

  it("forgets the log's texts when excerpts switch off and on another session", () => {
    const store = new SubAgentStore();
    store.retarget("1:a", true);
    store.refocus("x");
    store.receive("1:a", true, {
      type: "log",
      agentId: "x",
      reset: true,
      entries: [{ kind: "prompt", text: "task" }],
    });
    store.retarget("1:a", false);
    expect(store.current().log).toMatchObject({ agentId: "x", lines: [] });
    store.retarget("2:a", false);
    expect(store.current().log).toMatchObject({ agentId: "x", lines: [] });
    store.retarget(null, false);
    expect(store.current()).toBe(IDLE_WATCH);
  });

  it("stops telling a listener that unsubscribed", () => {
    const { store, seen, stop } = watched();
    stop();
    store.retarget("1:a", false);
    expect(seen).toEqual([]);
  });
});

describe("FocusSender", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  function sender() {
    vi.useFakeTimers();
    const sent: [number, string | null][] = [];
    const focus = new FocusSender(FOCUS_DEBOUNCE_MS, (sessionId, id) => {
      sent.push([sessionId, id]);
    });
    return { focus, sent };
  }

  it("sends only the last of quick switches, once they settle", () => {
    const { focus, sent } = sender();
    focus.started(7, null);
    focus.want("a1");
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS - 1);
    focus.want("a2");
    focus.want("a3");
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS - 1);
    expect(sent).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(sent).toEqual([[7, "a3"]]);
    // The watch has it now: the same focus again sends nothing.
    focus.want("a3");
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS);
    expect(sent).toHaveLength(1);
  });

  it("sends nothing without a watch, for its own focus or after it stopped", () => {
    const { focus, sent } = sender();
    focus.want("a1");
    focus.started(7, "a1");
    focus.want("a1");
    focus.want("a2");
    focus.stopped();
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS * 2);
    expect(sent).toEqual([]);
    // A restart that already carries the new focus drops the pending switch.
    focus.started(7, "a1");
    focus.want("a2");
    focus.started(8, "a2");
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS * 2);
    expect(sent).toEqual([]);
  });
});
