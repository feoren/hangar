import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  cleanQueuedText,
  discardMessage,
  EMPTY_QUEUE,
  guardedEnd,
  loadLegacyQueue,
  MAX_QUEUED_CHARS,
  parseStoredQueue,
  persistableQueue,
  phaseEnd,
  phaseEnded,
  QUEUE_STORAGE_KEY,
  queuedPreview,
  queueMessage,
  restoredQueue,
  saveMessage,
  storedQueue,
  workStarted,
  type MessageQueue,
} from "./messageQueue";

const KEY = "0b0e7c1e-5d3a-4c1b-9a3e-2f6d8e9a1b2c";

/** Runs one phase end and returns what was sent and the queue after it. */
function end(queue: MessageQueue, how: "done" | "asks" | "other") {
  return phaseEnded(queue, KEY, how);
}

describe("phaseEnd", () => {
  it("is done when a working program went quiet, watched or not", () => {
    expect(phaseEnd("working", "attention", "quiet", "quiet", false)).toBe("done");
    expect(phaseEnd("working", "idle", null, "quiet", false)).toBe("done");
  });

  it("asks on a bell, also when the watched state is only idle, and on a pending decision", () => {
    expect(phaseEnd("working", "attention", "bell", "bell", false)).toBe("asks");
    expect(phaseEnd("working", "idle", null, "bell", false)).toBe("asks");
    expect(phaseEnd("working", "attention", "quiet", "quiet", true)).toBe("asks");
    expect(phaseEnd("working", "attention", "gate", "quiet", false)).toBe("asks");
  });

  it("is other for an exit or a stop by the user", () => {
    expect(phaseEnd("working", "attention", "exit", "exit", false)).toBe("other");
    expect(phaseEnd("working", "idle", null, null, false)).toBe("other");
  });

  it("ends nothing outside a working phase", () => {
    expect(phaseEnd("idle", "working", null, null, false)).toBeNull();
    expect(phaseEnd("working", "working", null, null, false)).toBeNull();
    expect(phaseEnd("attention", "idle", null, "quiet", false)).toBeNull();
  });
});

describe("queue state machine", () => {
  it("queues while working and sends on done exactly once", () => {
    const queued = queueMessage(EMPTY_QUEUE, KEY, "run the tests\n");
    expect(queued.get(KEY)).toEqual({ text: "run the tests", armed: true });
    const first = end(queued, "done");
    expect(first.send).toBe("run the tests");
    expect(first.queue.has(KEY)).toBe(false);
    const second = end(first.queue, "done");
    expect(second.send).toBeNull();
  });

  it("keeps the message on asks and sends it on the done after the answer", () => {
    const queued = queueMessage(EMPTY_QUEUE, KEY, "next step");
    const asked = end(queued, "asks");
    expect(asked.send).toBeNull();
    expect(asked.queue.get(KEY)).toEqual({ text: "next step", armed: true, waiting: true });
    const answered = workStarted(asked.queue, KEY, true);
    expect(answered.get(KEY)?.waiting).toBeUndefined();
    expect(end(answered, "done").send).toBe("next step");
  });

  it("never sends after a restore until work follows the user's input", () => {
    const restored = restoredQueue(new Map([[KEY, "after restart"]]));
    // The resumed program's start-up output: work without input, then quiet.
    const startUp = workStarted(restored, KEY, false);
    const quiet = end(startUp, "done");
    expect(quiet.send).toBeNull();
    expect(quiet.queue.get(KEY)?.armed).toBe(false);
    // The user gives it a task; once that is done, the message goes out.
    const task = workStarted(quiet.queue, KEY, true);
    expect(end(task, "done").send).toBe("after restart");
  });

  it("disarms on an exit, so a restart's start-up screen sends nothing", () => {
    const queued = queueMessage(EMPTY_QUEUE, KEY, "later");
    const exited = end(queued, "other");
    expect(exited.send).toBeNull();
    expect(exited.queue.get(KEY)).toEqual({ text: "later", armed: false });
    expect(end(workStarted(exited.queue, KEY, false), "done").send).toBeNull();
  });

  it("editing replaces the message and arms it", () => {
    const restored = restoredQueue(new Map([[KEY, "old"]]));
    const edited = queueMessage(restored, KEY, "new");
    expect(edited.get(KEY)).toEqual({ text: "new", armed: true });
    expect(edited.size).toBe(1);
  });

  it("cancel and blank text discard it", () => {
    const queued = queueMessage(EMPTY_QUEUE, KEY, "x");
    expect(discardMessage(queued, KEY).has(KEY)).toBe(false);
    expect(queueMessage(queued, KEY, "  \n ").has(KEY)).toBe(false);
    expect(discardMessage(EMPTY_QUEUE, KEY)).toBe(EMPTY_QUEUE);
  });

  it("leaves sessions without a message alone", () => {
    expect(end(EMPTY_QUEUE, "done")).toEqual({ queue: EMPTY_QUEUE, send: null });
    expect(workStarted(EMPTY_QUEUE, KEY, true)).toBe(EMPTY_QUEUE);
  });
});

describe("guardedEnd", () => {
  it("turns a quiet end with a question on screen into asks, and keeps the message", () => {
    expect(guardedEnd("done", true)).toBe("asks");
    expect(guardedEnd("done", false)).toBe("done");
    expect(guardedEnd("other", true)).toBe("other");
    const queued = queueMessage(EMPTY_QUEUE, KEY, "go on");
    const held = end(queued, guardedEnd("done", true));
    expect(held.send).toBeNull();
    expect(held.queue.get(KEY)?.waiting).toBe(true);
  });
});

describe("saveMessage", () => {
  const done = {
    running: true,
    working: false,
    endedBy: "quiet" as const,
    gateWaiting: false,
    showsQuestion: false,
  };

  it("sends at once when the session is done already", () => {
    const saved = saveMessage(EMPTY_QUEUE, KEY, "go on", done);
    expect(saved.send).toBe("go on");
    expect(saved.queue.has(KEY)).toBe(false);
  });

  it("keeps it when the done screen shows a question", () => {
    const saved = saveMessage(EMPTY_QUEUE, KEY, "go on", { ...done, showsQuestion: true });
    expect(saved.send).toBeNull();
    expect(saved.queue.get(KEY)).toEqual({ text: "go on", armed: true, waiting: true });
  });

  it("queues it while working, asking, deciding or ended", () => {
    for (const now of [
      { ...done, working: true },
      { ...done, endedBy: "bell" as const },
      { ...done, gateWaiting: true },
      { ...done, running: false },
      { ...done, endedBy: null },
    ]) {
      const saved = saveMessage(EMPTY_QUEUE, KEY, "later", now);
      expect(saved.send).toBeNull();
      expect(saved.queue.get(KEY)?.text).toBe("later");
    }
  });
});

describe("text and storage", () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: (key: string) => {
        store.delete(key);
      },
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("keeps inner line breaks and cuts very long text", () => {
    expect(cleanQueuedText("a\nb\n\n")).toBe("a\nb");
    expect(cleanQueuedText("x".repeat(MAX_QUEUED_CHARS + 5))).toHaveLength(MAX_QUEUED_CHARS);
  });

  it("parses stored texts and drops malformed entries", () => {
    expect(parseStoredQueue(null).size).toBe(0);
    expect(parseStoredQueue([KEY]).size).toBe(0);
    const stored = parseStoredQueue({ [KEY]: "go", other: 3, blank: " " });
    expect([...stored]).toEqual([[KEY, "go"]]);
  });

  it("stores only the sessions it is told to keep, and only what the backend takes", () => {
    const queue = queueMessage(queueMessage(EMPTY_QUEUE, KEY, "go"), "gone", "old");
    const entries = storedQueue(queue, (key) => key === KEY || key === "gone");
    expect(entries).toEqual({ [KEY]: "go", gone: "old" });
    // Only session keys (UUIDs) reach the backend.
    expect(persistableQueue(entries)).toEqual({ [KEY]: "go" });
    expect(persistableQueue({ [KEY]: " \n " })).toEqual({});
  });

  it("reads what an older version kept in localStorage, for the migration", () => {
    store.set(QUEUE_STORAGE_KEY, JSON.stringify({ [KEY]: "go" }));
    expect([...loadLegacyQueue(localStorage)]).toEqual([[KEY, "go"]]);
  });

  it("survives a broken stored value", () => {
    store.set(QUEUE_STORAGE_KEY, "{not json");
    expect(loadLegacyQueue(localStorage).size).toBe(0);
  });

  it("previews on one line", () => {
    expect(queuedPreview("a\n  b")).toBe("a b");
    expect(queuedPreview("x".repeat(100), 10)).toBe(`${"x".repeat(9)}…`);
  });
});
