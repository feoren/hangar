import { describe, expect, it } from "vitest";

import {
  loadQueuedMessages,
  loadSecuritySwitches,
  parseSwitches,
  saveQueuedMessages,
  type MainBackend,
} from "./mainStore";
import { QUEUE_STORAGE_KEY } from "./messageQueue";
import { SETTINGS_STORAGE_KEY, type SecuritySwitches } from "./settings";

const KEY = "0b0e7c1e-5d3a-4c1b-9a3e-2f6d8e9a1b2c";
const ON: SecuritySwitches = { claimsEnabled: true, gateEnabled: true, commandGuardEnabled: true };
const OFF: SecuritySwitches = {
  claimsEnabled: false,
  gateEnabled: false,
  commandGuardEnabled: false,
};

function memoryStorage(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  return {
    store,
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
  };
}

function backend(switches: unknown, queue: unknown) {
  const saved: { switches: SecuritySwitches[]; queues: Record<string, string>[] } = {
    switches: [],
    queues: [],
  };
  const fake: MainBackend = {
    securitySwitches: () => Promise.resolve(switches),
    saveSecuritySwitches: (value) => {
      saved.switches.push(value);
      return Promise.resolve(null);
    },
    loadMessageQueue: () => Promise.resolve(queue),
    saveMessageQueue: (entries) => {
      saved.queues.push(entries);
      return Promise.resolve(null);
    },
  };
  return { fake, saved };
}

describe("security switches", () => {
  it("reads only complete answers", () => {
    expect(parseSwitches(OFF)).toEqual(OFF);
    expect(parseSwitches(null)).toBeNull();
    expect(parseSwitches({ claimsEnabled: true, gateEnabled: "no" })).toBeNull();
  });

  it("migrates the old values once and removes them from localStorage", async () => {
    const storage = memoryStorage({
      [SETTINGS_STORAGE_KEY]: JSON.stringify({ language: "de", gateEnabled: false }),
    });
    const { fake, saved } = backend(null, null);
    const legacy = { ...ON, gateEnabled: false };
    expect(await loadSecuritySwitches(legacy, fake, storage)).toEqual(legacy);
    expect(saved.switches).toEqual([legacy]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({ language: "de" });
  });

  it("takes the backend's values once stored and ignores what a viewer wrote", async () => {
    // Written by another window of the same origin after the migration: ignored and removed.
    const storage = memoryStorage({
      [SETTINGS_STORAGE_KEY]: JSON.stringify({ gateEnabled: false, commandGuardEnabled: false }),
    });
    const { fake, saved } = backend(ON, null);
    expect(await loadSecuritySwitches(OFF, fake, storage)).toEqual(ON);
    expect(saved.switches).toEqual([]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({});
  });

  it("keeps everything on without a backend", async () => {
    const fake: MainBackend = {
      ...backend(null, null).fake,
      securitySwitches: () => Promise.reject(new Error("no backend")),
    };
    expect(await loadSecuritySwitches(OFF, fake, null)).toEqual(ON);
  });
});

describe("queued messages", () => {
  it("migrates the old texts once, then only the backend counts", async () => {
    const storage = memoryStorage({
      [QUEUE_STORAGE_KEY]: JSON.stringify({ [KEY]: "go", "not-a-key": "x" }),
    });
    const { fake, saved } = backend(null, null);
    expect([...(await loadQueuedMessages(fake, storage))]).toEqual([[KEY, "go"]]);
    expect(saved.queues).toEqual([{ [KEY]: "go" }]);
    expect(storage.store.has(QUEUE_STORAGE_KEY)).toBe(false);
  });

  it("ignores and removes texts written to localStorage after the migration", async () => {
    const storage = memoryStorage({ [QUEUE_STORAGE_KEY]: JSON.stringify({ [KEY]: "injected" }) });
    const { fake, saved } = backend(null, { [KEY]: "mine" });
    expect([...(await loadQueuedMessages(fake, storage))]).toEqual([[KEY, "mine"]]);
    expect(saved.queues).toEqual([]);
    expect(storage.store.has(QUEUE_STORAGE_KEY)).toBe(false);
  });

  it("stores an empty queue when there was nothing to migrate", async () => {
    // Otherwise the backend stays "never stored" and a text a viewer writes to the old key
    // later would be migrated on the next start.
    const { fake, saved } = backend(null, null);
    expect([...(await loadQueuedMessages(fake, memoryStorage()))]).toEqual([]);
    expect(saved.queues).toEqual([{}]);
  });

  it("saves only what the backend takes", async () => {
    const { fake, saved } = backend(null, null);
    await saveQueuedMessages({ [KEY]: "go\n\n", other: "x" }, fake);
    expect(saved.queues).toEqual([{ [KEY]: "go" }]);
  });
});
