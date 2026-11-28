// The model choice (ADR 0022) on its way to the backend: `create_session` gets it as `model`,
// and `null` without a choice.

import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn<(command: string, args?: unknown) => Promise<unknown>>());

vi.mock("@tauri-apps/api/core", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/core")>();
  // Outside Tauri a real channel cannot register its callback.
  class Channel {
    onmessage: (message: unknown) => void = () => undefined;
  }
  return { ...actual, invoke, Channel };
});

import { api, Channel, NO_START_OPTIONS, type SessionEvent } from "./api";
import { startOptions } from "./startOptions";

const SIZE = { cols: 120, rows: 30 };
const channel = new Channel<SessionEvent>();
const settings = { hangarMd: false, worktreeUpstreamCheck: false };

function argsOf(call: number): Record<string, unknown> {
  const args = invoke.mock.calls[call]?.[1];
  return typeof args === "object" && args !== null ? { ...args } : {};
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({});
});

describe("createSession", () => {
  it("passes the chosen model to create_session", async () => {
    const model = { source: "lmstudio", id: "qwen/qwen3-8b" } as const;
    await api.createSession(
      "claude",
      "C:\\work",
      SIZE,
      channel,
      false,
      null,
      "private",
      startOptions(settings, false, null, model),
    );
    expect(invoke.mock.calls[0]?.[0]).toBe("create_session");
    expect(argsOf(0).model).toEqual(model);
  });

  it("passes null without a choice", async () => {
    await api.createSession("claude", null, SIZE, channel);
    await api.createSession("claude", null, SIZE, channel, true, null, "private", NO_START_OPTIONS);
    expect(argsOf(0).model).toBeNull();
    expect(argsOf(1).model).toBeNull();
  });
});
