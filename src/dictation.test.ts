import { describe, expect, it } from "vitest";

import {
  busySession,
  canStart,
  DEFAULT_DICTATION,
  endStaleRecording,
  failureAfter,
  LatestOnly,
  legacyDictationEnabled,
  parseDictationPrefs,
  parseDictationState,
  parseDictationStatus,
  parseDictationText,
  parseDictationTexts,
  parseLevel,
  pasteForm,
  takeDictationTexts,
  type DictationText,
} from "./dictation";

describe("parseDictationPrefs", () => {
  it("takes the fast model by default", () => {
    expect(parseDictationPrefs(undefined)).toEqual(DEFAULT_DICTATION);
    expect(DEFAULT_DICTATION).toEqual({ model: "base", language: "auto" });
  });

  it("keeps valid values and replaces invalid ones, never the key", () => {
    // Whether the key is on comes from the backend only (mainStore.ts).
    expect(parseDictationPrefs({ enabled: true, model: "small", language: "de" })).toEqual({
      model: "small",
      language: "de",
    });
    expect(parseDictationPrefs({ enabled: "yes", model: "large", language: "fr" })).toEqual(
      DEFAULT_DICTATION,
    );
    expect(parseDictationPrefs([])).toEqual(DEFAULT_DICTATION);
  });
});

describe("parseDictationState", () => {
  it("reads every state the backend sends", () => {
    expect(parseDictationState({ kind: "ready" })).toEqual({ kind: "ready" });
    expect(parseDictationState({ kind: "recording", session: 3 })).toEqual({
      kind: "recording",
      session: 3,
    });
    expect(parseDictationState({ kind: "downloading", model: "small", progress: 1.7 })).toEqual({
      kind: "downloading",
      model: "small",
      progress: 1,
    });
    expect(parseDictationState({ kind: "error", message: "x" })).toEqual({
      kind: "error",
      message: "x",
    });
  });

  it("rejects what it does not know", () => {
    expect(parseDictationState({ kind: "recording" })).toBeNull();
    expect(parseDictationState({ kind: "recording", session: -1 })).toBeNull();
    expect(parseDictationState({ kind: "downloading", model: "tiny", progress: 0 })).toBeNull();
    expect(parseDictationState({ kind: "listening" })).toBeNull();
    expect(parseDictationState(null)).toBeNull();
  });
});

describe("parseDictationStatus", () => {
  it("keeps the valid models", () => {
    expect(
      parseDictationStatus({
        state: { kind: "notInstalled" },
        models: [
          { id: "base", bytes: 59_707_625, installed: false },
          { id: "huge", bytes: 1, installed: true },
        ],
        source: "huggingface.co",
        microphone: false,
      }),
    ).toEqual({
      state: { kind: "notInstalled" },
      models: [{ id: "base", bytes: 59_707_625, installed: false }],
      source: "huggingface.co",
      microphone: false,
    });
    expect(parseDictationStatus({ state: { kind: "ready" } })).toBeNull();
  });

  it("needs to know whether a microphone is there", () => {
    const status = { state: { kind: "ready" }, models: [], source: "huggingface.co" };
    expect(parseDictationStatus({ ...status, microphone: true })?.microphone).toBe(true);
    expect(parseDictationStatus(status)).toBeNull();
    expect(parseDictationStatus({ ...status, microphone: "yes" })).toBeNull();
  });
});

describe("the dictation key before the backend kept it", () => {
  it("reads only a stored true", () => {
    expect(legacyDictationEnabled(JSON.stringify({ dictation: { enabled: true } }))).toBe(true);
    expect(legacyDictationEnabled(JSON.stringify({ dictation: { enabled: "yes" } }))).toBe(false);
    expect(legacyDictationEnabled(JSON.stringify({ dictation: [] }))).toBe(false);
    expect(legacyDictationEnabled("{")).toBe(false);
    expect(legacyDictationEnabled(null)).toBe(false);
  });
});

describe("taking the dictated text", () => {
  it("hands out the valid, non-blank texts, oldest first", async () => {
    const got: DictationText[] = [];
    await takeDictationTexts(
      () =>
        Promise.resolve([
          { session: 1, text: "one" },
          { session: "2", text: "broken" },
          { session: 3, text: "" },
          { session: 4, text: "four" },
        ]),
      (text) => got.push(text),
    );
    expect(got).toEqual([
      { session: 1, text: "one" },
      { session: 4, text: "four" },
    ]);
    expect(parseDictationTexts({ session: 1, text: "x" })).toEqual([]);
  });

  it("stays quiet when the backend refuses", async () => {
    const got: DictationText[] = [];
    await takeDictationTexts(
      () => Promise.reject(new Error("wrongWindow")),
      (text) => got.push(text),
    );
    expect(got).toEqual([]);
  });
});

describe("LatestOnly", () => {
  it("keeps only the newest answer", () => {
    const reads = new LatestOnly();
    const first = reads.next();
    const second = reads.next();
    expect(reads.current(first)).toBe(false);
    expect(reads.current(second)).toBe(true);
  });
});

describe("endStaleRecording", () => {
  it("stops a recording left over from before the window loaded, errors included", async () => {
    let stops = 0;
    const take = () => Promise.resolve([]);
    await endStaleRecording(() => {
      stops += 1;
      return Promise.resolve(null);
    }, take);
    await endStaleRecording(
      () => {
        stops += 1;
        return Promise.reject(new Error("no backend"));
      },
      () => Promise.reject(new Error("no backend")),
    );
    expect(stops).toBe(2);
  });

  it("drops the texts the old window never took before it stops, so the stop's text survives", async () => {
    const calls: string[] = [];
    await endStaleRecording(
      () => {
        calls.push("stop");
        return Promise.resolve(null);
      },
      () => {
        calls.push("take");
        return Promise.resolve([{ session: 1, text: "from before the reload" }]);
      },
    );
    expect(calls).toEqual(["take", "stop"]);
  });
});

describe("parseDictationText and parseLevel", () => {
  it("checks the shapes", () => {
    expect(parseDictationText({ session: 2, text: "hi" })).toEqual({ session: 2, text: "hi" });
    expect(parseDictationText({ session: "2", text: "hi" })).toBeNull();
    expect(parseLevel(0.4)).toBe(0.4);
    expect(parseLevel(9)).toBe(1);
    expect(parseLevel(Number.NaN)).toBe(0);
    expect(parseLevel("loud")).toBe(0);
  });
});

describe("busySession and canStart", () => {
  it("name the session while recording or transcribing", () => {
    expect(busySession({ kind: "recording", session: 4 })).toBe(4);
    expect(busySession({ kind: "transcribing", session: 4 })).toBe(4);
    expect(busySession({ kind: "ready" })).toBeNull();
  });

  it("allow a start when ready or after a failure, not while busy or uninstalled", () => {
    expect(canStart({ kind: "ready" })).toBe(true);
    expect(canStart({ kind: "noMicrophone" })).toBe(true);
    expect(canStart({ kind: "error", message: "" })).toBe(true);
    expect(canStart({ kind: "notInstalled" })).toBe(false);
    expect(canStart({ kind: "recording", session: 1 })).toBe(false);
    expect(canStart({ kind: "downloading", model: "base", progress: 0 })).toBe(false);
  });
});

describe("failureAfter", () => {
  it("names the session whose recording ended in a failure", () => {
    const recording = { kind: "recording", session: 3 } as const;
    const transcribing = { kind: "transcribing", session: 3 } as const;
    expect(failureAfter(transcribing, { kind: "permissionDenied" })).toEqual({
      session: 3,
      failure: "permissionDenied",
    });
    expect(failureAfter(recording, { kind: "noMicrophone" })).toEqual({
      session: 3,
      failure: "noMicrophone",
    });
    expect(failureAfter(transcribing, { kind: "error", message: "x" })).toEqual({
      session: 3,
      failure: "other",
    });
  });

  it("stays quiet for a normal end and for failures outside a recording", () => {
    expect(failureAfter({ kind: "transcribing", session: 3 }, { kind: "ready" })).toBeNull();
    expect(failureAfter({ kind: "ready" }, { kind: "error", message: "x" })).toBeNull();
    expect(
      failureAfter(
        { kind: "downloading", model: "base", progress: 0.5 },
        { kind: "error", message: "" },
      ),
    ).toBeNull();
  });
});

describe("pasteForm", () => {
  it("ends with one space so the next hold continues the sentence", () => {
    expect(pasteForm("Hallo Welt.")).toBe("Hallo Welt. ");
    expect(pasteForm("  run the tests  ")).toBe("run the tests ");
    expect(pasteForm("   ")).toBe("");
    expect(pasteForm("")).toBe("");
  });
});
