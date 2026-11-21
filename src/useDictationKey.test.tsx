// @vitest-environment happy-dom

// Without a model, holding the dictation key opens the setup on release, and only when the hold
// stayed a hold: right Ctrl held a little long before Ctrl+C must not open the settings.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EMPTY_DICTATION_STATUS } from "./dictation";
import { useDictationKey } from "./useDictation";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));
vi.mock("./api", () => ({
  api: {
    dictationStart: vi.fn(() => Promise.resolve()),
    dictationStop: vi.fn(() => Promise.resolve()),
    dictationTakeText: vi.fn(() => Promise.resolve(null)),
    dictationStatus: vi.fn(() => Promise.resolve(null)),
  },
  DICTATION_LEVEL: "dictation-level",
  DICTATION_STATUS: "dictation-status",
  DICTATION_TEXT: "dictation-text",
  errorText: () => "other",
}));

function Probe({ onSetup }: { onSetup: () => void }) {
  useDictationKey({
    enabled: true,
    prefs: { model: "base", language: "auto", key: "controlRight" },
    status: EMPTY_DICTATION_STATUS,
    target: () => 1,
    blocked: () => false,
    onSetup,
    onText: () => undefined,
    onProblem: () => undefined,
  });
  return null;
}

const key = (type: "keydown" | "keyup", init: KeyboardEventInit) => {
  act(() => {
    window.dispatchEvent(new KeyboardEvent(type, init));
  });
};
const rightCtrl = { key: "Control", code: "ControlRight", ctrlKey: true };

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

let root: Root;
const render = (element: React.ReactElement) => {
  act(() => {
    root.render(element);
  });
};

describe("useDictationKey without a model", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => {
    act(() => {
      root.unmount();
    });
    vi.useRealTimers();
  });

  it("opens the setup when right Ctrl is let go after a hold", () => {
    const onSetup = vi.fn();
    render(<Probe onSetup={onSetup} />);
    key("keydown", rightCtrl);
    act(() => {
      vi.advanceTimersByTime(400);
    });
    expect(onSetup).not.toHaveBeenCalled();
    key("keyup", { ...rightCtrl, ctrlKey: false });
    expect(onSetup).toHaveBeenCalledTimes(1);
  });

  it("keeps the setup closed when the hold became Ctrl+C", () => {
    const onSetup = vi.fn();
    render(<Probe onSetup={onSetup} />);
    key("keydown", rightCtrl);
    act(() => {
      vi.advanceTimersByTime(400);
    });
    key("keydown", { key: "c", code: "KeyC", ctrlKey: true });
    key("keyup", { key: "c", code: "KeyC", ctrlKey: true });
    key("keyup", { ...rightCtrl, ctrlKey: false });
    expect(onSetup).not.toHaveBeenCalled();
  });

  it("keeps the setup closed when the window loses the focus", () => {
    const onSetup = vi.fn();
    render(<Probe onSetup={onSetup} />);
    key("keydown", rightCtrl);
    act(() => {
      vi.advanceTimersByTime(400);
      window.dispatchEvent(new Event("blur"));
    });
    expect(onSetup).not.toHaveBeenCalled();
  });
});
