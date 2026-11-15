import { describe, expect, it } from "vitest";

import {
  BATCH_DELAY_MS,
  BATCH_SIZE,
  CACHE_LIMIT,
  FOUND_TTL_MS,
  MISSING_TTL_MS,
  parsePathKinds,
  PathChecker,
  type CheckerClock,
} from "./pathCheck";

/** A clock the test advances by hand; timers run when their time comes. */
function manualClock(): CheckerClock & { advance: (ms: number) => Promise<void> } {
  let now = 0;
  let timers: { at: number; run: () => void }[] = [];
  return {
    now: () => now,
    later: (run, ms) => {
      timers.push({ at: now + ms, run });
    },
    advance: async (ms) => {
      now += ms;
      const due = timers.filter((timer) => timer.at <= now);
      timers = timers.filter((timer) => timer.at > now);
      for (const timer of due) timer.run();
      // Let the backend's promises settle.
      for (let round = 0; round < 5; round += 1) await Promise.resolve();
    },
  };
}

describe("parsePathKinds", () => {
  it("accepts one entry per path", () => {
    expect(parsePathKinds(["file", null, "dir"], 3)).toEqual(["file", null, "dir"]);
    expect(parsePathKinds(["file"], 2)).toBeNull();
    expect(parsePathKinds(["link"], 1)).toBeNull();
    expect(parsePathKinds({}, 0)).toBeNull();
  });
});

describe("PathChecker", () => {
  it("batches lookups close together into one backend call", async () => {
    const clock = manualClock();
    const calls: string[][] = [];
    const checker = new PathChecker((paths) => {
      calls.push(paths);
      return Promise.resolve(paths.map((path) => (path.endsWith(".md") ? "file" : null)));
    }, clock);
    let done = 0;
    void checker.lookup(["/a.md", "/b"]).then(() => (done += 1));
    void checker.lookup(["/a.md", "/c.md"]).then(() => (done += 1));
    await clock.advance(BATCH_DELAY_MS - 1);
    expect(calls).toEqual([]);
    await clock.advance(1);
    expect(calls).toEqual([["/a.md", "/b", "/c.md"]]);
    expect(done).toBe(2);
    expect(checker.state("/a.md")).toBe("file");
    expect(checker.state("/b")).toBe("missing");
  });

  it("asks nothing for remembered paths and joins paths in flight", async () => {
    const clock = manualClock();
    let calls = 0;
    let answer: (value: unknown) => void = () => undefined;
    const checker = new PathChecker(() => {
      calls += 1;
      return new Promise((resolve) => {
        answer = resolve;
      });
    }, clock);
    const first = checker.lookup(["/x"]);
    await clock.advance(BATCH_DELAY_MS);
    const second = checker.lookup(["/x"]);
    await clock.advance(BATCH_DELAY_MS);
    expect(calls).toBe(1);
    answer(["dir"]);
    await Promise.all([first, second]);
    expect(checker.state("/x")).toBe("dir");
    await checker.lookup(["/x"]);
    expect(calls).toBe(1);
  });

  it("forgets missing paths sooner than found ones", async () => {
    const clock = manualClock();
    const checker = new PathChecker(
      (paths) => Promise.resolve(paths.map((path) => (path === "/found" ? "file" : null))),
      clock,
    );
    void checker.lookup(["/found", "/missing"]);
    await clock.advance(BATCH_DELAY_MS);
    await clock.advance(MISSING_TTL_MS);
    expect(checker.state("/missing")).toBeUndefined();
    expect(checker.state("/found")).toBe("file");
    await clock.advance(FOUND_TTL_MS);
    expect(checker.state("/found")).toBeUndefined();
  });

  it("treats a failed or malformed answer as missing", async () => {
    const clock = manualClock();
    const failing = new PathChecker(() => Promise.reject(new Error("no backend")), clock);
    const looked = failing.lookup(["/a"]);
    await clock.advance(BATCH_DELAY_MS);
    await looked;
    expect(failing.state("/a")).toBe("missing");
    const malformed = new PathChecker(() => Promise.resolve("nonsense"), clock);
    const again = malformed.lookup(["/b"]);
    await clock.advance(BATCH_DELAY_MS);
    await again;
    expect(malformed.state("/b")).toBe("missing");
  });

  it("splits large lookups into batches the backend accepts", async () => {
    const clock = manualClock();
    const sizes: number[] = [];
    const checker = new PathChecker((paths) => {
      sizes.push(paths.length);
      return Promise.resolve(paths.map(() => null));
    }, clock);
    void checker.lookup(Array.from({ length: BATCH_SIZE + 3 }, (_, index) => `/p${String(index)}`));
    await clock.advance(BATCH_DELAY_MS);
    expect(sizes).toEqual([BATCH_SIZE, 3]);
  });

  it("keeps at most the cache limit, dropping the oldest", async () => {
    const clock = manualClock();
    const checker = new PathChecker((paths) => Promise.resolve(paths.map(() => "file")), clock);
    const paths = Array.from({ length: CACHE_LIMIT + 1 }, (_, index) => `/f${String(index)}`);
    void checker.lookup(paths);
    await clock.advance(BATCH_DELAY_MS);
    for (let round = 0; round < 50; round += 1) await Promise.resolve();
    expect(checker.state("/f0")).toBeUndefined();
    expect(checker.state(`/f${String(CACHE_LIMIT)}`)).toBe("file");
  });
});
