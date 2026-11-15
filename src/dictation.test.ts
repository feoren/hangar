import { describe, expect, it } from "vitest";

import {
  busySession,
  canStart,
  DEFAULT_DICTATION,
  parseDictationPrefs,
  parseDictationState,
  parseDictationStatus,
  parseDictationText,
  parseLevel,
} from "./dictation";

describe("parseDictationPrefs", () => {
  it("is off with the fast model by default", () => {
    expect(parseDictationPrefs(undefined)).toEqual(DEFAULT_DICTATION);
    expect(DEFAULT_DICTATION).toEqual({ enabled: false, model: "base", language: "auto" });
  });

  it("keeps valid values and replaces invalid ones", () => {
    expect(parseDictationPrefs({ enabled: true, model: "small", language: "de" })).toEqual({
      enabled: true,
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
      }),
    ).toEqual({
      state: { kind: "notInstalled" },
      models: [{ id: "base", bytes: 59_707_625, installed: false }],
      source: "huggingface.co",
    });
    expect(parseDictationStatus({ state: { kind: "ready" } })).toBeNull();
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
