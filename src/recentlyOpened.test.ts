import { describe, expect, it, vi } from "vitest";

import { loadRecentSessions, saveRecentSessions, type RecentBackend } from "./mainStore";
import {
  isRecorded,
  KEEP_LIMIT,
  markSeeded,
  needsSeed,
  parseRecentSessions,
  pushRecent,
  recentEntry,
  SEEDED_STORAGE_KEY,
  seedReady,
  seedRecent,
  SHOW_LIMIT,
  shownRecent,
  UNKNOWN_TIME,
  withoutRecent,
  type RecentSession,
  type SessionSource,
} from "./recentlyOpened";

const ID = "0f8fad5b-d9cb-469f-a165-70867728950e";
const OTHER_ID = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

function session(patch: Partial<SessionSource> = {}): SessionSource {
  return {
    profileId: "claude",
    cwd: "C:\\work\\app",
    title: "Claude Code",
    kind: "agent",
    mode: "private",
    subAgent: null,
    cliSessionId: ID,
    marker: { color: "#4A8FE7", emoji: "\u{1F680}" },
    ...patch,
  };
}

function entry(patch: Partial<RecentSession> = {}): RecentSession {
  return {
    profileId: "claude",
    cwd: "C:\\work\\app",
    title: "Claude Code",
    color: null,
    emoji: null,
    cliSessionId: null,
    closedAt: 1,
    ...patch,
  };
}

describe("isRecorded", () => {
  it("records private sessions of agents and shells", () => {
    expect(isRecorded(session())).toBe(true);
    expect(isRecorded(session({ kind: undefined, mode: undefined, subAgent: undefined }))).toBe(
      true,
    );
  });

  it("leaves out updates, sub-agent starts, team sessions and own worktrees", () => {
    expect(isRecorded(session({ kind: "update" }))).toBe(false);
    expect(isRecorded(session({ subAgent: "backend" }))).toBe(false);
    expect(isRecorded(session({ mode: "team" }))).toBe(false);
    expect(isRecorded(session({ worktree: { path: "C:\\wt" } }))).toBe(false);
  });
});

describe("recentEntry", () => {
  it("keeps program, folder, title, marker and conversation", () => {
    expect(recentEntry(session(), 42)).toEqual({
      profileId: "claude",
      cwd: "C:\\work\\app",
      title: "Claude Code",
      color: "#4a8fe7",
      emoji: "\u{1F680}",
      cliSessionId: ID,
      closedAt: 42,
    });
  });

  it("keeps the model choice, and writes no field without one", () => {
    const model = { source: "ollama", id: "qwen3:8b" } as const;
    expect(recentEntry(session({ model }), 42)?.model).toEqual(model);
    expect(recentEntry(session({ model: null }), 42)).not.toHaveProperty("model");
    expect(recentEntry(session({ model: { source: "ollama", id: "-x" } }), 42)).not.toHaveProperty(
      "model",
    );
    expect(parseRecentSessions([{ ...entry(), model }])[0]?.model).toEqual(model);
    // A broken choice only costs the model.
    const broken = parseRecentSessions([{ ...entry(), model: { source: "cloud", id: "x" } }]);
    expect(broken).toHaveLength(1);
    expect(broken[0]).not.toHaveProperty("model");
  });

  it("works without marker and conversation, as a fresh start tells it", () => {
    const shell = recentEntry(
      { profileId: "pwsh", cwd: "C:\\work", title: "PowerShell 7", cliSessionId: undefined },
      7,
    );
    expect(shell).toMatchObject({ color: null, emoji: null, cliSessionId: null, closedAt: 7 });
  });

  it("gives nothing for sessions that are not recorded or would not keep", () => {
    expect(recentEntry(session({ kind: "update" }), 1)).toBeNull();
    expect(recentEntry(session({ title: "a\u001b[2J" }), 1)).toBeNull();
    expect(recentEntry(session({ cwd: " " }), 1)).toBeNull();
    expect(recentEntry(session({ title: "x".repeat(257) }), 1)).toBeNull();
    expect(recentEntry(session(), Number.NaN)).toBeNull();
  });
});

describe("pushRecent", () => {
  it("puts the newest first and keeps at most KEEP_LIMIT", () => {
    let list: RecentSession[] = [];
    for (let index = 0; index < KEEP_LIMIT + 3; index += 1) {
      list = pushRecent(list, entry({ cwd: `/srv/${String(index)}`, closedAt: index }));
    }
    expect(list).toHaveLength(KEEP_LIMIT);
    expect(list[0]?.cwd).toBe(`/srv/${String(KEEP_LIMIT + 2)}`);
  });

  it("updates the start's entry on close with the final title and conversation", () => {
    const started = recentEntry(session({ cliSessionId: null, marker: undefined }), 10);
    const closed = recentEntry(session({ title: "Shop checkout" }), 20);
    if (started === null || closed === null) throw new Error("entries expected");
    const other = entry({ cwd: "/other", closedAt: 15 });
    expect(pushRecent(pushRecent([started], other), closed)).toEqual([closed, other]);
  });

  it("keeps one entry per conversation", () => {
    const first = entry({ cwd: "/a", cliSessionId: ID, closedAt: 1 });
    const again = entry({ cwd: "/b", cliSessionId: ID, closedAt: 2 });
    expect(pushRecent([first], again)).toEqual([again]);
  });

  it("keeps one entry per program and folder, also spelled another way", () => {
    const first = entry({ cwd: "C:\\Work\\App\\", closedAt: 1, cliSessionId: OTHER_ID });
    const again = entry({ cwd: "c:/work/app", closedAt: 2, cliSessionId: ID });
    expect(pushRecent([first], again)).toEqual([again]);
    const elsewhere = entry({ profileId: "codex", closedAt: 3 });
    expect(pushRecent([again], elsewhere)).toEqual([elsewhere, again]);
  });

  it("refuses an entry the backend would refuse", () => {
    const list = [entry()];
    expect(pushRecent(list, entry({ profileId: " " }))).toEqual(list);
  });
});

describe("withoutRecent", () => {
  it("takes out exactly that entry", () => {
    const a = entry({ closedAt: 1 });
    const b = entry({ cwd: "/other", closedAt: 2 });
    expect(withoutRecent([b, a], { ...a })).toEqual([b]);
  });
});

describe("shownRecent", () => {
  it("hides what a session holds and shows at most SHOW_LIMIT", () => {
    const held = entry({ cwd: "/held", cliSessionId: ID, closedAt: 99 });
    const rest = Array.from({ length: 7 }, (_, index) =>
      entry({ cwd: `/srv/${String(index)}`, closedAt: index }),
    );
    const open = { profileId: "claude", cwd: "/elsewhere" };
    const shown = shownRecent([held, ...rest], [{ ...open, cliSessionId: ID.toUpperCase() }, open]);
    expect(shown).toHaveLength(SHOW_LIMIT);
    expect(shown).not.toContain(held);
    expect(shownRecent([held], [{ ...open, cliSessionId: null }])).toEqual([held]);
  });

  it("hides an entry while its program runs in its folder, with or without a conversation", () => {
    const codex = entry({ profileId: "codex", cwd: "C:\\work\\app", cliSessionId: null });
    expect(shownRecent([codex], [{ profileId: "codex", cwd: "c:/work/app/" }])).toEqual([]);
    expect(shownRecent([codex], [{ profileId: "claude", cwd: "C:\\work\\app" }])).toEqual([codex]);
    expect(shownRecent([codex], [{ profileId: "codex", cwd: "C:\\work\\other" }])).toEqual([codex]);
    // Another conversation of the same program in the same folder is open: offered once closed.
    const claude = entry({ cliSessionId: ID });
    const running = { profileId: "claude", cwd: "C:\\work\\app", cliSessionId: OTHER_ID };
    expect(shownRecent([claude], [running])).toEqual([]);
  });
});

describe("seedRecent", () => {
  it("offers the recent folders, newest first, with the last program, up to SHOW_LIMIT", () => {
    const folders = Array.from({ length: 7 }, (_, index) => `C:\\work\\p${String(index)}`);
    const seed = seedRecent(folders, "claude");
    expect(seed).toHaveLength(SHOW_LIMIT);
    expect(seed[0]).toEqual({
      profileId: "claude",
      cwd: "C:\\work\\p0",
      title: "p0",
      color: null,
      emoji: null,
      cliSessionId: null,
      closedAt: UNKNOWN_TIME,
    });
    expect(seed.map((item) => item.title)).toEqual(["p0", "p1", "p2", "p3", "p4"]);
    expect(parseRecentSessions(seed)).toEqual(seed);
  });

  it("gives nothing without a program, and skips what would not keep or comes twice", () => {
    expect(seedRecent(["C:\\work"], undefined)).toEqual([]);
    expect(seedRecent(["C:\\work", "c:/work/", "a\u0001b"], "pwsh").map((e) => e.cwd)).toEqual([
      "C:\\work",
    ]);
  });
});

describe("needsSeed and markSeeded", () => {
  function memory(): Storage {
    const items = new Map<string, string>();
    return {
      get length() {
        return items.size;
      },
      clear: () => {
        items.clear();
      },
      getItem: (key) => items.get(key) ?? null,
      key: () => null,
      removeItem: (key) => {
        items.delete(key);
      },
      setItem: (key, value) => {
        items.set(key, value);
      },
    };
  }

  it("makes the first list once", () => {
    const storage = memory();
    expect(needsSeed(storage)).toBe(true);
    markSeeded(storage);
    expect(storage.getItem(SEEDED_STORAGE_KEY)).toBe("1");
    expect(needsSeed(storage)).toBe(false);
  });

  it("makes none without storage", () => {
    expect(needsSeed(null)).toBe(false);
    const blocked = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(needsSeed(blocked)).toBe(false);
    expect(() => {
      markSeeded(blocked);
    }).not.toThrow();
  });
});

describe("parseRecentSessions", () => {
  it("reads what the backend answers", () => {
    const stored = [
      entry({ color: "#4a8fe7", emoji: "\u{2B50}", cliSessionId: ID }),
      entry({ profileId: "pwsh", cwd: "/home/me" }),
    ];
    expect(parseRecentSessions(stored)).toEqual(stored);
    // Missing optional fields read as none.
    expect(
      parseRecentSessions([{ profileId: "cmd", cwd: "C:\\", title: "cmd", closedAt: 5 }]),
    ).toEqual([entry({ profileId: "cmd", cwd: "C:\\", title: "cmd", closedAt: 5 })]);
  });

  it("gives an empty list for anything malformed", () => {
    expect(parseRecentSessions(null)).toEqual([]);
    expect(parseRecentSessions({})).toEqual([]);
    expect(parseRecentSessions([entry(), "x"])).toEqual([]);
    expect(parseRecentSessions([entry({ cwd: "" })])).toEqual([]);
    expect(parseRecentSessions([entry({ color: "red" })])).toEqual([]);
    expect(parseRecentSessions([entry({ cliSessionId: ID.toUpperCase() })])).toEqual([]);
    expect(parseRecentSessions([entry({ closedAt: -1 })])).toEqual([]);
    expect(parseRecentSessions([{ ...entry(), title: 5 }])).toEqual([]);
    const twice = [entry({ cliSessionId: ID }), entry({ cwd: "/b", cliSessionId: ID })];
    expect(parseRecentSessions(twice)).toEqual([]);
    const many = Array.from({ length: KEEP_LIMIT + 1 }, (_, index) =>
      entry({ cwd: `/srv/${String(index)}` }),
    );
    expect(parseRecentSessions(many)).toEqual([]);
  });
});

describe("loadRecentSessions and saveRecentSessions", () => {
  function backend(answer: Promise<unknown>): RecentBackend & { saved: unknown[] } {
    const saved: unknown[] = [];
    return {
      saved,
      loadRecentlyClosed: () => answer,
      saveRecentlyClosed: vi.fn((entries: readonly RecentSession[]) => {
        saved.push(entries);
        return Promise.resolve(null);
      }),
    };
  }

  it("loads the checked list, and none on an error or for a planted one", async () => {
    const stored = [entry({ cliSessionId: ID })];
    expect(await loadRecentSessions(backend(Promise.resolve(stored)))).toEqual(stored);
    expect(await loadRecentSessions(backend(Promise.reject(new Error("no backend"))))).toEqual([]);
    expect(await loadRecentSessions(backend(Promise.resolve([{ cwd: "/x" }])))).toEqual([]);
  });

  it("saves the list as the backend takes it", async () => {
    const fake = backend(Promise.resolve([]));
    const list = [entry()];
    await saveRecentSessions(list, fake);
    expect(fake.saved).toEqual([list]);
  });
});

describe("seedReady", () => {
  // Before the protected settings arrive, the program a start would use may be one the person
  // hid; the first list is made only once, so it must wait for them as well.
  it("waits for the preferences, the programs and the protected settings", () => {
    const all = { prefsReady: true, profilesReady: true, protectedReady: true };
    expect(seedReady(all)).toBe(true);
    expect(seedReady({ ...all, protectedReady: false })).toBe(false);
    expect(seedReady({ ...all, prefsReady: false })).toBe(false);
    expect(seedReady({ ...all, profilesReady: false })).toBe(false);
  });
});
