import { describe, expect, it } from "vitest";

import type { AttentionState } from "./attention";
import { FLASH_KIND, shouldFlash, TaskbarFlasher, type FlashKind } from "./taskbarFlash";

const background = { enabled: true, windowFocused: false };

describe("shouldFlash", () => {
  it("flashes when a background session starts waiting", () => {
    expect(shouldFlash("attention", background)).toBe(true);
  });

  it("does not flash in the foreground, without the setting or for other states", () => {
    expect(shouldFlash("attention", { enabled: true, windowFocused: true })).toBe(false);
    expect(shouldFlash("attention", { enabled: false, windowFocused: false })).toBe(false);
    const others: AttentionState[] = ["idle", "working"];
    for (const state of others) expect(shouldFlash(state, background)).toBe(false);
  });
});

function recorder(fail = false) {
  const calls: (FlashKind | null)[] = [];
  const flasher = new TaskbarFlasher((kind) => {
    calls.push(kind);
    return fail ? Promise.reject(new Error("no Tauri")) : Promise.resolve();
  });
  return { calls, flasher };
}

describe("TaskbarFlasher", () => {
  it("requests the chosen kind and stops on focus", () => {
    const { calls, flasher } = recorder();
    flasher.attentionChanged("attention", background);
    expect(calls).toEqual([FLASH_KIND]);
    expect(flasher.active).toBe(true);
    flasher.stop();
    expect(calls).toEqual([FLASH_KIND, null]);
    expect(flasher.active).toBe(false);
  });

  it("chooses the subtle kind", () => {
    expect(FLASH_KIND).toBe("informational");
  });

  it("stops only while flashing", () => {
    const { calls, flasher } = recorder();
    flasher.stop();
    flasher.attentionChanged("working", background);
    flasher.attentionChanged("attention", { enabled: true, windowFocused: true });
    flasher.stop();
    expect(calls).toEqual([]);
  });

  it("flashes again for every newly waiting session and stops once", () => {
    const { calls, flasher } = recorder();
    flasher.attentionChanged("attention", background);
    flasher.attentionChanged("attention", background);
    flasher.stop();
    flasher.stop();
    expect(calls).toEqual([FLASH_KIND, FLASH_KIND, null]);
  });

  it("does not count as flashing after a failed call", async () => {
    const { calls, flasher } = recorder(true);
    flasher.attentionChanged("attention", background);
    await Promise.resolve();
    await Promise.resolve();
    expect(flasher.active).toBe(false);
    flasher.stop();
    expect(calls).toEqual([FLASH_KIND]);
  });
});
