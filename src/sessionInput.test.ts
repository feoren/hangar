import { describe, expect, it } from "vitest";

import {
  BUSY_LIMIT_MS,
  InputStalled,
  inputQueue,
  PASTE_END,
  PASTE_START,
  PIECE_UNITS,
  settleStalled,
  splitInput,
  writeInput,
} from "./sessionInput";

/** A clock that only moves when the code under test waits. */
function fakeTiming() {
  let now = 0;
  return {
    sleep: (ms: number) => {
      now += ms;
      return Promise.resolve();
    },
    now: () => now,
  };
}

function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

describe("splitInput", () => {
  it("keeps short input in one piece", () => {
    expect(splitInput("hangar")).toEqual(["hangar"]);
    expect(splitInput("")).toEqual([""]);
  });

  it("splits a large paste into pieces below the backend's limit", () => {
    const paste = "ä".repeat(PIECE_UNITS * 3 + 5);
    const pieces = splitInput(paste);
    expect(pieces.join("")).toBe(paste);
    expect(pieces).toHaveLength(4);
    for (const piece of pieces) expect(utf8Bytes(piece)).toBeLessThanOrEqual(1024 * 1024);
  });

  it("never cuts a surrogate pair", () => {
    const paste = "a" + "\u{1F600}".repeat(10);
    const pieces = splitInput(paste, 4);
    expect(pieces.join("")).toBe(paste);
    for (const piece of pieces) {
      expect(piece).not.toMatch(/[\uD800-\uDBFF]$/);
      expect(piece).not.toMatch(/^[\uDC00-\uDFFF]/);
    }
  });
});

describe("writeInput", () => {
  it("sends again while the queue is full", async () => {
    const answers = [false, false, true];
    const sent: string[] = [];
    await writeInput(
      (piece) => {
        sent.push(piece);
        return Promise.resolve(answers.shift() ?? true);
      },
      "marker",
      fakeTiming(),
    );
    expect(sent).toEqual(["marker", "marker", "marker"]);
  });

  it("gives up when the session never reads", async () => {
    const timing = fakeTiming();
    await expect(writeInput(() => Promise.resolve(false), "marker", timing)).rejects.toThrow(
      InputStalled,
    );
    expect(timing.now()).toBeGreaterThan(BUSY_LIMIT_MS);
  });

  it("ends a bracketed paste whose end was not sent", async () => {
    const paste = PASTE_START + "x".repeat(PIECE_UNITS * 2) + PASTE_END;
    const sent: string[] = [];
    let accepted = 1;
    const send = (piece: string) => {
      // The program takes the first piece, then stops reading; the end marker gets through.
      if (piece === PASTE_END || accepted > 0) {
        accepted -= 1;
        sent.push(piece);
        return Promise.resolve(true);
      }
      return Promise.resolve(false);
    };
    await expect(writeInput(send, paste, fakeTiming())).rejects.toThrow(InputStalled);
    expect(sent).toHaveLength(2);
    expect(sent[0]?.startsWith(PASTE_START)).toBe(true);
    expect(sent[1]).toBe(PASTE_END);
  });

  it("sends no extra end when the paste end already went out or there was no paste", async () => {
    const sent: string[] = [];
    const firstOnly = () => {
      let first = true;
      return (piece: string) => {
        if (!first) return Promise.resolve(false);
        first = false;
        sent.push(piece);
        return Promise.resolve(true);
      };
    };
    // Plain typing: no paste to end.
    await expect(
      writeInput(firstOnly(), "y".repeat(PIECE_UNITS + 1), fakeTiming()),
    ).rejects.toThrow(InputStalled);
    // A complete paste, then more input that stalls.
    const paste = PASTE_START + "z" + PASTE_END + "w".repeat(PIECE_UNITS);
    await expect(writeInput(firstOnly(), paste, fakeTiming())).rejects.toThrow(InputStalled);
    expect(sent.filter((piece) => piece === PASTE_END)).toEqual([]);
  });

  it("still reports the stall when the end marker is refused too", async () => {
    const tried: string[] = [];
    let first = true;
    await expect(
      writeInput(
        (piece) => {
          tried.push(piece);
          // The first piece with the paste start arrives, then nothing more.
          const taken = first;
          first = false;
          return Promise.resolve(taken);
        },
        PASTE_START + "v".repeat(PIECE_UNITS) + PASTE_END,
        fakeTiming(),
      ),
    ).rejects.toThrow(InputStalled);
    expect(tried.at(-1)).toBe(PASTE_END);
  });

  it("sends no paste end when not even the paste start arrived", async () => {
    const tried: string[] = [];
    await expect(
      writeInput(
        (piece) => {
          tried.push(piece);
          return Promise.resolve(false);
        },
        PASTE_START + "v" + PASTE_END,
        fakeTiming(),
      ),
    ).rejects.toThrow(InputStalled);
    expect(tried.length).toBeGreaterThan(0);
    expect(tried.every((piece) => piece !== PASTE_END)).toBe(true);
  });

  it("sends a large paste completely and in order", async () => {
    const paste = "x".repeat(PIECE_UNITS * 2 + 1);
    const sent: string[] = [];
    await writeInput(
      (piece) => {
        sent.push(piece);
        return Promise.resolve(true);
      },
      paste,
      fakeTiming(),
    );
    expect(sent).toHaveLength(3);
    expect(sent.join("")).toBe(paste);
  });
});

describe("inputQueue", () => {
  it("keeps the order of one session while a piece waits", async () => {
    const sent: string[] = [];
    let busy = 2;
    const write = inputQueue((id, piece) => {
      if (piece === "first" && busy > 0) {
        busy -= 1;
        return Promise.resolve(false);
      }
      sent.push(`${String(id)}:${piece}`);
      return Promise.resolve(true);
    }, fakeTiming());
    await Promise.all([write(1, "first"), write(1, "second")]);
    expect(sent).toEqual(["1:first", "1:second"]);
  });

  it("goes on after a failed write", async () => {
    const sent: string[] = [];
    const write = inputQueue((_id, piece) => {
      if (piece === "broken") return Promise.reject(new Error("not running"));
      sent.push(piece);
      return Promise.resolve(true);
    }, fakeTiming());
    await expect(write(1, "broken")).rejects.toThrow();
    await write(1, "after");
    expect(sent).toEqual(["after"]);
  });

  it("drops the keys queued behind a stalled one instead of sending them with a gap", async () => {
    // The program reads nothing for 40 s: `n` stalls after 30 s and is dropped. The Enter typed
    // right after it must not arrive alone, or it would confirm whatever the prompt defaults to.
    const timing = fakeTiming();
    const sent: string[] = [];
    const write = inputQueue((id, piece) => {
      if (timing.now() < 40_000) return Promise.resolve(false);
      sent.push(`${String(id)}:${piece}`);
      return Promise.resolve(true);
    }, timing);
    const key = write(1, "n");
    const enter = write(1, "\r");
    await expect(key).rejects.toThrow(InputStalled);
    await expect(enter).rejects.toThrow(InputStalled);
    expect(await enter.catch((error: unknown) => error)).toMatchObject({ dropped: true });
    expect(await key.catch((error: unknown) => error)).toMatchObject({ dropped: false });
    expect(sent).toEqual([]);
    // Typed after the stall was known: goes out once the program reads again.
    await write(1, "x");
    expect(sent).toEqual(["1:x"]);
  });

  it("keeps keys typed during a slow paste that arrives in the end", async () => {
    // The program reads slowly, but within the limit: nothing is dropped, nothing reordered.
    const timing = fakeTiming();
    const sent: string[] = [];
    const write = inputQueue((_id, piece) => {
      if (timing.now() < BUSY_LIMIT_MS - 1_000) return Promise.resolve(false);
      sent.push(piece);
      return Promise.resolve(true);
    }, timing);
    const paste = PASTE_START + "p".repeat(PIECE_UNITS + 10) + PASTE_END;
    const writes = [write(1, paste), write(1, "a"), write(1, "b"), write(1, "\r")];
    await Promise.all(writes);
    expect(sent.join("")).toBe(paste + "ab\r");
  });

  it("keeps other sessions' writes when one session stalls", async () => {
    const timing = fakeTiming();
    const sent: string[] = [];
    const write = inputQueue((id, piece) => {
      if (id === 1 && timing.now() < 40_000) return Promise.resolve(false);
      sent.push(`${String(id)}:${piece}`);
      return Promise.resolve(true);
    }, timing);
    const stalled = write(1, "n");
    const other = write(2, "y");
    await expect(stalled).rejects.toThrow(InputStalled);
    await other;
    expect(sent).toEqual(["2:y"]);
  });
});

describe("settleStalled", () => {
  it("rejects when the input stalled", async () => {
    await expect(settleStalled(Promise.reject(new InputStalled()))).rejects.toThrow(InputStalled);
  });

  it("resolves when the write arrived or failed otherwise", async () => {
    await expect(settleStalled(Promise.resolve(true))).resolves.toBeUndefined();
    await expect(settleStalled(Promise.reject(new Error("not running")))).resolves.toBeUndefined();
  });
});
