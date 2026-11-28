// @vitest-environment happy-dom

// The local servers' store in the window: it reads `local_llm_status` once, follows
// `local-llm-changed` and asks for a probe at once when the window gains focus.

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({
  status: vi.fn<() => Promise<unknown>>(),
  refresh: vi.fn<() => Promise<unknown>>(),
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    calls.handlers.set(name, handler);
    return Promise.resolve(() => undefined);
  },
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, localLlmStatus: calls.status, localLlmRefresh: calls.refresh },
  };
});

import { LOCAL_LLM_CHANGED } from "./api";
import type { LocalLlmState } from "./localLlm";
import { useLocalLlm } from "./useLocalLlm";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const OFF = {
  ollama: { running: false, models: [] },
  lmstudio: { running: false, models: [] },
};
const ON = {
  ollama: { running: true, models: [{ id: "qwen3:8b", label: "qwen3:8b", loaded: false }] },
  lmstudio: { running: false, models: [] },
};

const seenStates: (LocalLlmState | null)[] = [];
function Probe() {
  const state = useLocalLlm();
  useEffect(() => {
    seenStates.push(state);
  }, [state]);
  return null;
}

const seen = () => seenStates.at(-1);

let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  calls.status.mockResolvedValue(OFF);
  calls.refresh.mockResolvedValue(OFF);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
});

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("useLocalLlm", () => {
  it("reads the state, follows the event and probes on focus", async () => {
    act(() => {
      root.render(<Probe />);
    });
    await settle();
    expect(calls.status).toHaveBeenCalledOnce();
    expect(seen()).toEqual(OFF);

    act(() => {
      calls.handlers.get(LOCAL_LLM_CHANGED)?.({ payload: ON });
    });
    expect(seen()).toEqual(ON);

    // A malformed payload changes nothing.
    act(() => {
      calls.handlers.get(LOCAL_LLM_CHANGED)?.({ payload: "nonsense" });
    });
    expect(seen()).toEqual(ON);

    act(() => {
      window.dispatchEvent(new Event("focus"));
      // Focus and visibility together still probe once.
      window.dispatchEvent(new Event("focus"));
    });
    await settle();
    expect(calls.refresh).toHaveBeenCalledOnce();
    expect(seen()).toEqual(OFF);
  });
});
