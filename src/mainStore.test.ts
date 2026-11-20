import { describe, expect, it } from "vitest";

import {
  defaultMainPrefs,
  legacyMainPrefs,
  loadMainPrefs,
  loadProtectedSettings,
  loadQueuedMessages,
  loadSecuritySwitches,
  loadStoredTemplates,
  parseProtected,
  parseMainPrefs,
  parseSwitches,
  saveMainPrefs,
  saveProtectedSettings,
  saveQueuedMessages,
  saveStoredTemplates,
  type MainBackend,
  type MainPrefs,
} from "./mainStore";
import { QUEUE_STORAGE_KEY } from "./messageQueue";
import { SETTINGS_STORAGE_KEY, type ProtectedSettings, type SecuritySwitches } from "./settings";
import { defaultSnippets } from "./snippets";
import { TEMPLATES_STORAGE_KEY, type WorkspaceTemplate } from "./templates";

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

/** The preferences as a Hangar from before dictation stored them. */
function withoutDictation(prefs: MainPrefs): Record<string, unknown> {
  return Object.fromEntries(Object.entries(prefs).filter(([key]) => key !== "dictationEnabled"));
}

function backend(
  switches: unknown,
  queue: unknown,
  settings: unknown = null,
  templates: unknown = null,
  prefs: unknown = null,
) {
  const saved: {
    switches: SecuritySwitches[];
    queues: Record<string, string>[];
    settings: ProtectedSettings[];
    templates: (readonly WorkspaceTemplate[])[];
    prefs: MainPrefs[];
  } = {
    switches: [],
    queues: [],
    settings: [],
    templates: [],
    prefs: [],
  };
  const fake: MainBackend = {
    protectedSettings: () => Promise.resolve(settings),
    saveProtectedSettings: (value) => {
      saved.settings.push(value);
      return Promise.resolve(null);
    },
    mainPrefs: () => Promise.resolve(prefs),
    saveMainPrefs: (value) => {
      saved.prefs.push(value);
      return Promise.resolve(null);
    },
    loadTemplates: () => Promise.resolve(templates),
    saveTemplates: (value) => {
      saved.templates.push(value);
      return Promise.resolve(null);
    },
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

const PROTECTED: ProtectedSettings = {
  warnMultilinePaste: true,
  autoUpdate: true,
  hangarMd: true,
  snippets: [{ id: "mine", text: "Fix the build" }],
};

describe("protected settings", () => {
  it("reads only complete answers", () => {
    expect(parseProtected(PROTECTED, "en")).toEqual(PROTECTED);
    expect(parseProtected(null, "en")).toBeNull();
    expect(parseProtected({ ...PROTECTED, autoUpdate: "no" }, "en")).toBeNull();
    expect(parseProtected({ ...PROTECTED, snippets: undefined }, "en")).toBeNull();
  });

  it("migrates the old values once and removes them from localStorage", async () => {
    const storage = memoryStorage({
      [SETTINGS_STORAGE_KEY]: JSON.stringify({
        language: "de",
        warnMultilinePaste: false,
        autoUpdate: false,
        hangarMd: false,
        snippets: [],
      }),
    });
    const { fake, saved } = backend(null, null);
    const legacy = { warnMultilinePaste: false, autoUpdate: false, hangarMd: false, snippets: [] };
    expect(await loadProtectedSettings(legacy, "de", fake, storage)).toEqual(legacy);
    expect(saved.settings).toEqual([legacy]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({ language: "de" });
  });

  it("stores the defaults when there was nothing to migrate", async () => {
    // Otherwise the backend stays "never stored" and a value a viewer writes to the old place
    // later would be migrated on the next start.
    const { fake, saved } = backend(null, null);
    const legacy = { ...PROTECTED, snippets: defaultSnippets("en") };
    expect(await loadProtectedSettings(legacy, "en", fake, memoryStorage())).toEqual(legacy);
    expect(saved.settings).toEqual([legacy]);
  });

  it("takes the backend's values once stored and ignores what a viewer wrote", async () => {
    const planted = { warnMultilinePaste: false, snippets: [{ id: "x", text: "HANGAR-MARKER" }] };
    const storage = memoryStorage({ [SETTINGS_STORAGE_KEY]: JSON.stringify(planted) });
    const { fake, saved } = backend(null, null, PROTECTED);
    const legacy = { ...PROTECTED, warnMultilinePaste: false, snippets: planted.snippets };
    expect(await loadProtectedSettings(legacy, "en", fake, storage)).toEqual(PROTECTED);
    expect(saved.settings).toEqual([]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({});
  });

  it("keeps the warning and updates on without a backend", async () => {
    const fake: MainBackend = {
      ...backend(null, null).fake,
      protectedSettings: () => Promise.reject(new Error("no backend")),
    };
    const off = { warnMultilinePaste: false, autoUpdate: false, hangarMd: false, snippets: [] };
    const settings = await loadProtectedSettings(off, "en", fake, null);
    expect(settings.warnMultilinePaste).toBe(true);
    expect(settings.autoUpdate).toBe(true);
    expect(settings.snippets).toEqual(defaultSnippets("en"));
  });

  it("saves only what the backend takes", async () => {
    const { fake, saved } = backend(null, null);
    await saveProtectedSettings(
      { ...PROTECTED, snippets: [...PROTECTED.snippets, { id: "new", text: "  " }] },
      fake,
    );
    expect(saved.settings).toEqual([PROTECTED]);
  });
});

const TEMPLATE: WorkspaceTemplate = {
  id: "template-1",
  name: "Morning",
  sessions: [{ profileId: "claude", folder: "/home/user/repo", mode: "private" }],
};

describe("workspace templates", () => {
  it("migrates the old list once, then only the backend counts", async () => {
    const storage = memoryStorage({ [TEMPLATES_STORAGE_KEY]: JSON.stringify([TEMPLATE]) });
    const { fake, saved } = backend(null, null);
    expect(await loadStoredTemplates(fake, storage)).toEqual([TEMPLATE]);
    expect(saved.templates).toEqual([[TEMPLATE]]);
    expect(storage.store.has(TEMPLATES_STORAGE_KEY)).toBe(false);
  });

  it("never reads templates from localStorage once the backend has stored them", async () => {
    const planted = { ...TEMPLATE, name: "Planted" };
    const storage = memoryStorage({ [TEMPLATES_STORAGE_KEY]: JSON.stringify([planted]) });
    const { fake, saved } = backend(null, null, null, [TEMPLATE]);
    expect(await loadStoredTemplates(fake, storage)).toEqual([TEMPLATE]);
    expect(saved.templates).toEqual([]);
    expect(storage.store.has(TEMPLATES_STORAGE_KEY)).toBe(false);
  });

  it("stores an empty list when there was nothing to migrate", async () => {
    const { fake, saved } = backend(null, null);
    expect(await loadStoredTemplates(fake, memoryStorage())).toEqual([]);
    expect(saved.templates).toEqual([[]]);
  });

  it("has no templates without a backend", async () => {
    const fake: MainBackend = {
      ...backend(null, null).fake,
      loadTemplates: () => Promise.reject(new Error("no backend")),
    };
    const storage = memoryStorage({ [TEMPLATES_STORAGE_KEY]: JSON.stringify([TEMPLATE]) });
    expect(await loadStoredTemplates(fake, storage)).toEqual([]);
  });

  it("saves only what the backend takes", async () => {
    const { fake, saved } = backend(null, null);
    const broken = { ...TEMPLATE, id: "t-2", name: "Broken", sessions: [] };
    await saveStoredTemplates([TEMPLATE, broken], fake);
    expect(saved.templates).toEqual([[TEMPLATE]]);
  });
});

describe("start and notice preferences", () => {
  const MINE: MainPrefs = {
    startMode: "team",
    lastProgram: "claude",
    favorites: ["C:\\Work"],
    recent: ["/home/a/repo"],
    restoreSessions: false,
    attentionEnabled: true,
    taskbarFlash: false,
    notifyDesktop: true,
    chime: false,
    quietHours: { enabled: true, from: 1320, to: 420 },
    dailyBudget: 12.5,
    budgetEarlyPercent: 80,
    morningReview: true,
    // What the old key "hangar.costs.warnedDay" held: that version warned only at the budget.
    budgetWarned: { day: "2026-11-14", level: "reached" },
    morningOfferedDay: null,
    dictationEnabled: false,
  };
  /** What a viewer of the same origin could write to the old places after the migration. */
  const PLANTED = {
    "hangar.startMode": "private",
    "hangar.lastProgram": "planted",
    "hangar.folders": JSON.stringify({ favorites: [], recent: ["C:\\planted"] }),
    "hangar.lastCwd": "C:\\planted",
    "hangar.costs.warnedDay": "2999-12-31",
    [SETTINGS_STORAGE_KEY]: JSON.stringify({
      language: "de",
      restoreSessions: false,
      attentionEnabled: false,
      notifyDesktop: false,
      quietHours: { enabled: true, from: 0, to: 1439 },
      dailyBudget: 100_000,
    }),
  };

  it("reads only complete answers", () => {
    expect(parseMainPrefs(MINE)).toEqual(MINE);
    expect(parseMainPrefs(null)).toBeNull();
    expect(parseMainPrefs({ ...MINE, startMode: "root" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, lastProgram: "a\u0007b" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, chime: "no" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, dailyBudget: 0 })).toBeNull();
    expect(parseMainPrefs({ ...MINE, dailyBudget: "5" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, budgetWarned: "2026-13-01" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, budgetWarned: { day: "2026-11-14" } })).toBeNull();
    expect(
      parseMainPrefs({ ...MINE, budgetWarned: { day: "2026-11-14", level: "over" } }),
    ).toBeNull();
    expect(parseMainPrefs({ ...MINE, budgetEarlyPercent: 100 })).toBeNull();
    expect(parseMainPrefs({ ...MINE, budgetEarlyPercent: 12.5 })).toBeNull();
    expect(parseMainPrefs({ ...MINE, morningReview: "yes" })).toBeNull();
    expect(parseMainPrefs({ ...MINE, morningOfferedDay: "yesterday" })).toBeNull();
    expect(
      parseMainPrefs({
        ...MINE,
        dailyBudget: null,
        budgetWarned: null,
        budgetEarlyPercent: null,
        morningReview: false,
        morningOfferedDay: "2026-11-19",
      }),
    ).toEqual({
      ...MINE,
      dailyBudget: null,
      budgetWarned: null,
      budgetEarlyPercent: null,
      morningReview: false,
      morningOfferedDay: "2026-11-19",
    });
    // An answer of an older backend: the early warning and the morning summary at their defaults.
    const later = ["budgetEarlyPercent", "morningReview", "morningOfferedDay"];
    const beforeEarly = Object.fromEntries(
      Object.entries(MINE).filter(([key]) => !later.includes(key)),
    );
    expect(parseMainPrefs(beforeEarly)).toEqual(MINE);
    expect(parseMainPrefs({ ...MINE, dictationEnabled: "yes" })).toBeNull();
    // A file from before dictation: the choice is left open for the migration.
    const older = withoutDictation(MINE);
    expect(parseMainPrefs(older)?.dictationEnabled).toBeNull();
    expect(parseMainPrefs({ ...MINE, dictationEnabled: null })?.dictationEnabled).toBeNull();
    // Folders and quiet hours are read as the menu and the settings read them.
    expect(parseMainPrefs({ ...MINE, recent: ["/r", "a\nb"] })?.recent).toEqual(["/r"]);
  });

  it("migrates the old values once and removes them from localStorage", async () => {
    const storage = memoryStorage({
      "hangar.startMode": "team",
      "hangar.lastProgram": "claude",
      "hangar.lastCwd": "/home/a/repo",
      "hangar.folders": JSON.stringify({ favorites: ["C:\\Work"], recent: [] }),
      "hangar.costs.warnedDay": "2026-11-14",
      [SETTINGS_STORAGE_KEY]: JSON.stringify({
        language: "de",
        restoreSessions: false,
        taskbarFlash: false,
        chime: false,
        dailyBudget: 12.5,
      }),
    });
    const legacy = legacyMainPrefs(storage, storage.getItem(SETTINGS_STORAGE_KEY));
    expect(legacy).toEqual({ ...MINE, quietHours: defaultMainPrefs().quietHours });
    const { fake, saved } = backend(null, null);
    expect(await loadMainPrefs(legacy, fake, storage)).toEqual(legacy);
    expect(saved.prefs).toEqual([legacy]);
    for (const key of [
      "hangar.startMode",
      "hangar.lastProgram",
      "hangar.folders",
      "hangar.lastCwd",
      "hangar.costs.warnedDay",
    ]) {
      expect(storage.store.has(key), key).toBe(false);
    }
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({ language: "de" });
  });

  it("stores the defaults when there was nothing to migrate", async () => {
    const { fake, saved } = backend(null, null);
    const legacy = legacyMainPrefs(memoryStorage(), null);
    expect(legacy).toEqual(defaultMainPrefs());
    expect(await loadMainPrefs(legacy, fake, memoryStorage())).toEqual(defaultMainPrefs());
    expect(saved.prefs).toEqual([defaultMainPrefs()]);
  });

  it("takes the backend's values once stored and ignores what a viewer wrote", async () => {
    const storage = memoryStorage(PLANTED);
    const { fake, saved } = backend(null, null, null, null, MINE);
    const legacy = legacyMainPrefs(storage, storage.getItem(SETTINGS_STORAGE_KEY));
    expect(await loadMainPrefs(legacy, fake, storage)).toEqual(MINE);
    expect(saved.prefs).toEqual([]);
    expect(storage.store.has("hangar.folders")).toBe(false);
    expect(storage.store.has("hangar.lastCwd")).toBe(false);
    expect(storage.store.has("hangar.costs.warnedDay")).toBe(false);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({ language: "de" });
  });

  it("starts private with every notice on without a backend", async () => {
    const fake: MainBackend = {
      ...backend(null, null).fake,
      mainPrefs: () => Promise.reject(new Error("no backend")),
    };
    const storage = memoryStorage(PLANTED);
    const legacy = legacyMainPrefs(storage, storage.getItem(SETTINGS_STORAGE_KEY));
    expect(await loadMainPrefs(legacy, fake, storage)).toEqual(defaultMainPrefs());
    expect(defaultMainPrefs().startMode).toBe("private");
    expect(defaultMainPrefs().restoreSessions).toBe(true);
    expect(defaultMainPrefs().notifyDesktop).toBe(true);
  });

  it("keeps the old values when the backend cannot store them, for the next start", async () => {
    const old = {
      "hangar.startMode": "team",
      "hangar.lastProgram": "claude",
      "hangar.folders": JSON.stringify({ favorites: ["C:\\Work"], recent: ["/home/a/repo"] }),
    };
    const storage = memoryStorage(old);
    const fake: MainBackend = {
      ...backend(null, null).fake,
      saveMainPrefs: () => Promise.reject(new Error("disk full")),
    };
    const legacy = legacyMainPrefs(storage, null);
    // This run still uses what was migrated.
    expect(await loadMainPrefs(legacy, fake, storage)).toEqual(legacy);
    expect(legacy.startMode).toBe("team");
    for (const [key, value] of Object.entries(old)) {
      expect(storage.store.get(key), key).toBe(value);
    }
  });

  it("saves only what the backend takes", async () => {
    const { fake, saved } = backend(null, null);
    await saveMainPrefs(
      { ...MINE, lastProgram: " ", recent: ["/r", "", "/r"], startMode: "team" },
      fake,
    );
    expect(saved.prefs).toEqual([{ ...MINE, lastProgram: null, recent: ["/r"] }]);
    await saveMainPrefs(
      {
        ...MINE,
        dailyBudget: 0.001,
        budgetWarned: { day: "yesterday", level: "early" },
        budgetEarlyPercent: 0,
        morningOfferedDay: "2026-02-31x",
      },
      fake,
    );
    expect(saved.prefs[1]).toEqual({
      ...MINE,
      dailyBudget: null,
      budgetWarned: null,
      budgetEarlyPercent: null,
      morningOfferedDay: null,
    });
  });

  it("takes the old dictation key over once, also into a file from before dictation", async () => {
    const text = JSON.stringify({
      language: "de",
      dictation: { enabled: true, model: "small", language: "de" },
    });
    // Nothing stored yet: with the rest of the preferences.
    let storage = memoryStorage({ [SETTINGS_STORAGE_KEY]: text });
    let legacy = legacyMainPrefs(storage, text);
    expect(legacy.dictationEnabled).toBe(true);
    let { fake, saved } = backend(null, null);
    expect((await loadMainPrefs(legacy, fake, storage)).dictationEnabled).toBe(true);
    expect(saved.prefs.map((prefs) => prefs.dictationEnabled)).toEqual([true]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({
      language: "de",
      dictation: { model: "small", language: "de" },
    });
    // A file from before dictation: the old key is added to what the backend holds.
    storage = memoryStorage({ [SETTINGS_STORAGE_KEY]: text });
    legacy = legacyMainPrefs(storage, text);
    const older = withoutDictation(MINE);
    ({ fake, saved } = backend(null, null, null, null, older));
    expect(await loadMainPrefs(legacy, fake, storage)).toEqual({ ...MINE, dictationEnabled: true });
    expect(saved.prefs).toEqual([{ ...MINE, dictationEnabled: true }]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({
      language: "de",
      dictation: { model: "small", language: "de" },
    });
  });

  it("never turns the dictation key on from what a viewer planted later", async () => {
    const text = JSON.stringify({ language: "de", dictation: { enabled: true } });
    const storage = memoryStorage({ [SETTINGS_STORAGE_KEY]: text });
    const { fake, saved } = backend(null, null, null, null, MINE);
    const legacy = legacyMainPrefs(storage, text);
    expect((await loadMainPrefs(legacy, fake, storage)).dictationEnabled).toBe(false);
    expect(saved.prefs).toEqual([]);
    expect(JSON.parse(storage.store.get(SETTINGS_STORAGE_KEY) ?? "")).toEqual({
      language: "de",
      dictation: {},
    });
  });
});
