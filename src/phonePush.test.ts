import { describe, expect, it } from "vitest";

import {
  DEFAULT_PUSH_VIEW,
  isNtfyUrl,
  isWeakTopic,
  needsTokenAgain,
  ntfyTopic,
  parsePushView,
  pushErrorKey,
  pushEventFor,
  PushScheduler,
  randomTopic,
  urlOrigin,
  withTopic,
  type PushEvent,
} from "./phonePush";

describe("token and topic rules", () => {
  const stored = { ntfyUrl: "https://ntfy.sh/topic", ntfyToken: true };

  it("asks for the token again on another scheme, host or port", () => {
    expect(needsTokenAgain(stored, "https://ntfy.sh/other", false, false)).toBe(false);
    expect(needsTokenAgain(stored, "https://evil.example/topic", false, false)).toBe(true);
    expect(needsTokenAgain(stored, "https://ntfy.sh:8443/topic", false, false)).toBe(true);
    // Typed again, removed, no token stored or no address: nothing to ask.
    expect(needsTokenAgain(stored, "https://evil.example/topic", true, false)).toBe(false);
    expect(needsTokenAgain(stored, "https://evil.example/topic", false, true)).toBe(false);
    expect(
      needsTokenAgain({ ...stored, ntfyToken: false }, "https://evil.example/t", false, false),
    ).toBe(false);
    expect(needsTokenAgain(stored, "", false, false)).toBe(false);
    expect(urlOrigin("https://ntfy.sh:443/x")).toBe("https://ntfy.sh");
    expect(urlOrigin("nonsense")).toBeNull();
  });

  it("wants a long topic without a token", () => {
    expect(isWeakTopic("https://ntfy.sh/short", false)).toBe(true);
    expect(isWeakTopic("https://ntfy.sh/short", true)).toBe(false);
    expect(isWeakTopic("https://ntfy.sh/0123456789abcdef", false)).toBe(false);
    // An invalid address has its own message.
    expect(isWeakTopic("http://ntfy.sh/short", false)).toBe(false);
  });

  it("generates 128-bit base64url topics", () => {
    expect(randomTopic(new Uint8Array(16).fill(255))).toBe("_____________________w");
    const topic = randomTopic();
    expect(topic).toMatch(/^[A-Za-z0-9_-]{22}$/);
    expect(randomTopic()).not.toBe(topic);
  });

  it("puts a topic into the address", () => {
    expect(withTopic("https://push.example.org/ntfy/old", "new")).toBe(
      "https://push.example.org/ntfy/new",
    );
    expect(withTopic("", "new")).toBe("https://ntfy.sh/new");
    expect(withTopic("http://x/old", "new")).toBe("https://ntfy.sh/new");
    expect(ntfyTopic(withTopic("https://ntfy.sh/", "abc"))).toBe("abc");
  });
});

describe("parsePushView", () => {
  it("is off for anything that is not an object", () => {
    expect(parsePushView(null)).toEqual(DEFAULT_PUSH_VIEW);
    expect(parsePushView("ntfy")).toEqual(DEFAULT_PUSH_VIEW);
    expect(parsePushView([])).toEqual(DEFAULT_PUSH_VIEW);
  });

  it("takes valid values", () => {
    const view = {
      provider: "pushover",
      ntfyUrl: "https://ntfy.sh/topic",
      includeTitle: true,
      events: { waiting: false, done: true, failed: false },
      ntfyToken: false,
      pushoverUser: true,
      pushoverToken: true,
    };
    expect(parsePushView(view)).toEqual(view);
  });

  it("replaces wrong values field by field", () => {
    expect(
      parsePushView({
        provider: "email",
        ntfyUrl: 42,
        events: { waiting: "yes", done: false },
        ntfyToken: "tk_secret",
      }),
    ).toEqual({
      ...DEFAULT_PUSH_VIEW,
      events: { waiting: true, done: false, failed: true },
    });
  });
});

describe("isNtfyUrl", () => {
  it("wants https and a topic at the end", () => {
    expect(isNtfyUrl("https://ntfy.sh/hangar_alerts-1")).toBe(true);
    expect(isNtfyUrl(" https://push.example.org/ntfy/topic ")).toBe(true);
    for (const bad of [
      "",
      "ntfy.sh/topic",
      "http://ntfy.sh/topic",
      "https://ntfy.sh/",
      "https://ntfy.sh/topic/",
      "https://ntfy.sh/topic?x=1",
      "https://ntfy.sh/topic#x",
      "https://me:pw@ntfy.sh/topic",
      "https://ntfy.sh/t%C3%B6pic",
    ]) {
      expect(isNtfyUrl(bad), bad).toBe(false);
    }
  });
});

describe("pushEventFor", () => {
  it("names the state a push tells about", () => {
    expect(pushEventFor("attention", undefined)).toBe("waiting");
    expect(pushEventFor("attention", 0)).toBe("done");
    expect(pushEventFor("attention", 2)).toBe("failed");
    expect(pushEventFor("attention", null)).toBe("failed");
    expect(pushEventFor("working", undefined)).toBeNull();
    expect(pushEventFor("idle", 0)).toBeNull();
  });
});

describe("pushErrorKey", () => {
  it("maps known kinds and falls back otherwise", () => {
    expect(pushErrorKey("unauthorized")).toBe("push.error.unauthorized");
    expect(pushErrorKey("something")).toBe("push.error.unknown");
    expect(pushErrorKey(new Error("x"))).toBe("push.error.unknown");
  });
});

/** Timers driven by hand: `advance` runs what is due. */
function manualTimers() {
  let now = 0;
  let next = 1;
  const due = new Map<number, { at: number; callback: () => void }>();
  return {
    setTimeout(callback: () => void, ms: number) {
      const handle = next++;
      due.set(handle, { at: now + ms, callback });
      return handle;
    },
    clearTimeout(handle: number) {
      due.delete(handle);
    },
    advance(ms: number) {
      now += ms;
      for (const [handle, timer] of [...due]) {
        if (timer.at <= now) {
          due.delete(handle);
          timer.callback();
        }
      }
    },
  };
}

describe("PushScheduler", () => {
  function setup() {
    const timers = manualTimers();
    const sent: [number, PushEvent][] = [];
    const scheduler = new PushScheduler(1000, timers);
    scheduler.setSender((id, event) => {
      sent.push([id, event]);
    });
    return { timers, sent, scheduler };
  }

  it("sends once a state has lasted the delay", () => {
    const { timers, sent, scheduler } = setup();
    scheduler.change(1, "waiting");
    timers.advance(999);
    expect(sent).toEqual([]);
    timers.advance(1);
    expect(sent).toEqual([[1, "waiting"]]);
  });

  it("drops a state that ends before the delay", () => {
    const { timers, sent, scheduler } = setup();
    scheduler.change(1, "waiting");
    timers.advance(500);
    scheduler.change(1, null);
    timers.advance(5000);
    expect(sent).toEqual([]);
  });

  it("sends one push per stretch of the same state", () => {
    const { timers, sent, scheduler } = setup();
    scheduler.change(1, "waiting");
    timers.advance(1000);
    scheduler.change(1, "waiting");
    timers.advance(5000);
    expect(sent).toEqual([[1, "waiting"]]);
    // Waiting ends when the process exits: that is a new state.
    scheduler.change(1, "done");
    timers.advance(1000);
    expect(sent).toEqual([
      [1, "waiting"],
      [1, "done"],
    ]);
  });

  it("restarts the delay for a new state and forgets closed sessions", () => {
    const { timers, sent, scheduler } = setup();
    scheduler.change(1, "waiting");
    scheduler.change(2, "waiting");
    timers.advance(600);
    scheduler.change(1, "failed");
    scheduler.forget(2);
    timers.advance(600);
    expect(sent).toEqual([]);
    timers.advance(400);
    expect(sent).toEqual([[1, "failed"]]);
  });
});
