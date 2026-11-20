import { describe, expect, it } from "vitest";

import {
  CardSends,
  cardNotice,
  cardTime,
  cardPreview,
  discardFromCard,
  latestOnly,
  pruneHidden,
  sendFromCard,
  type CardSend,
} from "./messageCard";
import type { DeliveryResult } from "./partnerMessage";

function flush(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}

const GONE = "[link_message_gone] The message no longer waits";

/** A backend refusal as Tauri's `invoke` delivers it: the error's text, not an `Error`. */
function refuse(text: string): Promise<never> {
  // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- invoke's shape
  return Promise.reject(text);
}

function sendDeps(result: DeliveryResult, over: { close?: () => Promise<void> } = {}) {
  const calls: string[] = [];
  const errors: unknown[] = [];
  return {
    calls,
    errors,
    deps: {
      deliver: () => {
        calls.push("deliver");
        return Promise.resolve(result);
      },
      close:
        over.close ??
        (() => {
          calls.push("close");
          return Promise.resolve();
        }),
      release: () => {
        calls.push("release");
        return Promise.resolve();
      },
      reload: () => calls.push("reload"),
      showError: (error: unknown) => errors.push(error),
    },
  };
}

describe("sendFromCard", () => {
  it("removes a pasted message and reads the list again", async () => {
    const run = sendDeps({ kind: "pasted" });
    expect(await sendFromCard(run.deps)).toEqual({ kind: "pasted" });
    expect(run.calls).toEqual(["deliver", "close", "reload"]);
  });

  it("reads the list again when the removal after the paste fails", async () => {
    // Before, the card relied on the backend's event alone: a removal that failed (the message
    // was gone already) left a stale card and an error for a message that had arrived.
    const gone = sendDeps({ kind: "pasted" }, { close: () => refuse(GONE) });
    expect(await sendFromCard(gone.deps)).toEqual({ kind: "pasted" });
    expect(gone.calls).toEqual(["deliver", "reload"]);
    expect(gone.errors).toEqual([]);
    // Any other failure is named, and the list is read again all the same.
    const broken = sendDeps({ kind: "pasted" }, { close: () => refuse("[io] disk") });
    await sendFromCard(broken.deps);
    expect(broken.errors).toEqual(["[io] disk"]);
    expect(broken.calls).toEqual(["deliver", "reload"]);
  });

  it("frees a message that did not go in, but not one the backend refused", async () => {
    for (const kind of ["gone", "cancelled", "notReady", "pasteFailed"] as const) {
      const run = sendDeps({ kind });
      await sendFromCard(run.deps);
      expect(run.calls).toEqual(["deliver", "release", "reload"]);
    }
    const refused = sendDeps({ kind: "refused", error: "[link_claimed] x" });
    await sendFromCard(refused.deps);
    expect(refused.calls).toEqual(["deliver", "reload"]);
  });

  it("reads the list again even when the delivery throws", async () => {
    const calls: string[] = [];
    await expect(
      sendFromCard({
        deliver: () => Promise.reject(new Error("boom")),
        close: () => Promise.resolve(),
        reload: () => calls.push("reload"),
        showError: () => undefined,
      }),
    ).rejects.toThrow("boom");
    expect(calls).toEqual(["reload"]);
  });
});

describe("cardNotice", () => {
  it("says nothing for a sent, cancelled or vanished message", () => {
    expect(cardNotice({ kind: "pasted" }, false)).toEqual({ kind: "none" });
    expect(cardNotice({ kind: "cancelled" }, false)).toEqual({ kind: "none" });
    // The group was unlinked or the message discarded meanwhile: no error for it.
    expect(cardNotice({ kind: "gone" }, true)).toEqual({ kind: "none" });
    expect(cardNotice({ kind: "refused", error: GONE }, false)).toEqual({ kind: "none" });
    expect(cardNotice({ kind: "refused", error: "[partner_message_gone] x" }, false)).toEqual({
      kind: "none",
    });
  });

  it("names a target that went away or was not ready, and a refusal", () => {
    expect(cardNotice({ kind: "gone" }, false)).toEqual({ kind: "hint", key: "partner.unsent" });
    expect(cardNotice({ kind: "pasteFailed" }, false)).toEqual({
      kind: "hint",
      key: "partner.unsent",
    });
    expect(cardNotice({ kind: "notReady" }, false)).toEqual({
      kind: "hint",
      key: "partner.notReady",
    });
    expect(cardNotice({ kind: "refused", error: "[link_wrong_target] x" }, false)).toEqual({
      kind: "error",
      error: "[link_wrong_target] x",
    });
  });
});

describe("discardFromCard", () => {
  function discard(close: () => Promise<void>) {
    const calls: string[] = [];
    const errors: unknown[] = [];
    const done = discardFromCard({
      hide: () => calls.push("hide"),
      unhide: () => calls.push("unhide"),
      close: () => {
        calls.push("close");
        return close();
      },
      reload: () => calls.push("reload"),
      showError: (error) => errors.push(error),
    });
    return { calls, errors, done };
  }

  it("hides the card at once, before the backend answers", async () => {
    // Before, the card went only once the backend's event arrived; when that failed it stayed.
    let answer: () => void = () => undefined;
    const run = discard(
      () =>
        new Promise<void>((resolve) => {
          answer = resolve;
        }),
    );
    expect(run.calls).toEqual(["hide", "close"]);
    answer();
    await run.done;
    expect(run.calls).toEqual(["hide", "close", "reload"]);
  });

  it("keeps a message hidden that was gone already, and shows one again that stays", async () => {
    const gone = discard(() => refuse(GONE));
    await gone.done;
    expect(gone.calls).toEqual(["hide", "close", "reload"]);
    expect(gone.errors).toEqual([]);
    const kept = discard(() => refuse("[io] disk"));
    await kept.done;
    expect(kept.calls).toEqual(["hide", "close", "unhide", "reload"]);
    expect(kept.errors).toEqual(["[io] disk"]);
  });
});

describe("CardSends", () => {
  function sends() {
    const seen: ReadonlyMap<number, CardSend>[] = [];
    const runs = new CardSends((current) => seen.push(current));
    return { runs, seen, last: () => seen[seen.length - 1] };
  }

  it("runs one send per message, so a second click cannot paste it twice", () => {
    const { runs, last } = sends();
    expect(runs.start(4, true, "web")).toBe(true);
    expect(runs.start(4, false, "web")).toBe(false);
    expect(last()?.get(4)).toEqual({ phase: "waiting", submit: true, target: "web" });
    runs.finish(4);
    expect(last()?.has(4)).toBe(false);
    expect(runs.start(4, false, "web")).toBe(true);
  });

  it("takes a cancel only while the send still waits", () => {
    const { runs, last } = sends();
    runs.start(1, true, "web");
    runs.start(2, true, "web");
    expect(runs.cancel(1)).toBe(true);
    expect(runs.cancelled(1)).toBe(true);
    runs.sending(2);
    expect(last()?.get(2)?.phase).toBe("sending");
    expect(runs.cancel(2)).toBe(false);
    expect(runs.cancelled(2)).toBe(false);
    expect(runs.cancel(9)).toBe(false);
  });
});

describe("pruneHidden", () => {
  it("forgets discarded messages once the backend no longer lists them", () => {
    const hidden = new Set([1, 2]);
    expect(pruneHidden(hidden, [1, 3])).toEqual(new Set([1]));
    // Unchanged, it is the same set, so React renders nothing anew.
    expect(pruneHidden(hidden, [1, 2, 3])).toBe(hidden);
  });
});

describe("latestOnly", () => {
  it("drops an answer that arrives after a newer one", async () => {
    // Two reads of the list can overlap (an event per change); the older answer must not
    // bring back a card the newer one no longer has.
    const answers: ((value: string) => void)[] = [];
    const applied: string[] = [];
    const load = latestOnly(
      () =>
        new Promise<string>((resolve) => {
          answers.push(resolve);
        }),
      (value) => applied.push(value),
      () => undefined,
    );
    load();
    load();
    answers[1]?.("new");
    await flush();
    answers[0]?.("old");
    await flush();
    expect(applied).toEqual(["new"]);
  });

  it("names a failed read", async () => {
    const failed: unknown[] = [];
    const load = latestOnly(
      () => Promise.reject(new Error("no")),
      () => undefined,
      (error) => failed.push(error),
    );
    load();
    await flush();
    expect(failed).toHaveLength(1);
  });
});

describe("cardPreview", () => {
  it("shows a short text whole", () => {
    expect(cardPreview("one\ntwo")).toEqual({ lines: ["one", "two"], cut: false });
  });

  it("cuts a long text after a few lines or characters", () => {
    const many = cardPreview("a\nb\nc\nd\ne\nf");
    expect(many.cut).toBe(true);
    expect(many.lines).toEqual(["a", "b", "c", "d…"]);
    const long = cardPreview("x".repeat(500));
    expect(long.cut).toBe(true);
    expect(Array.from(long.lines.join("")).length).toBeLessThanOrEqual(240);
    expect(long.lines.join("").endsWith("…")).toBe(true);
  });
});

describe("cardTime", () => {
  it("shows the clock time today and the date before", () => {
    const now = new Date(2026, 10, 20, 15, 0).getTime();
    expect(cardTime(new Date(2026, 10, 20, 9, 5).getTime(), now, "de")).toBe("09:05");
    expect(cardTime(new Date(2026, 10, 18, 9, 5).getTime(), now, "de")).toBe("18.11., 09:05");
  });
});
