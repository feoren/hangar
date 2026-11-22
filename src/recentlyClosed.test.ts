import { describe, expect, it, vi } from "vitest";

import { loadRecentlyClosed, saveRecentlyClosed, type ClosedBackend } from "./mainStore";
import {
  closedEntry,
  KEEP_LIMIT,
  parseRecentlyClosed,
  pushClosed,
  recordsOnClose,
  SHOW_LIMIT,
  shownClosed,
  withoutClosed,
  type ClosedSession,
  type ClosingSession,
} from "./recentlyClosed";

const ID = "0f8fad5b-d9cb-469f-a165-70867728950e";
const OTHER_ID = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

function session(patch: Partial<ClosingSession> = {}): ClosingSession {
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

function entry(patch: Partial<ClosedSession> = {}): ClosedSession {
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

describe("recordsOnClose", () => {
  it("records private sessions of agents and shells", () => {
    expect(recordsOnClose(session())).toBe(true);
    expect(recordsOnClose(session({ kind: undefined, mode: undefined, subAgent: undefined }))).toBe(
      true,
    );
  });

  it("leaves out updates, sub-agent starts, team sessions and own worktrees", () => {
    expect(recordsOnClose(session({ kind: "update" }))).toBe(false);
    expect(recordsOnClose(session({ subAgent: "backend" }))).toBe(false);
    expect(recordsOnClose(session({ mode: "team" }))).toBe(false);
    expect(recordsOnClose(session({ worktree: { path: "C:\\wt" } }))).toBe(false);
  });
});

describe("closedEntry", () => {
  it("keeps program, folder, title, marker and conversation", () => {
    expect(closedEntry(session(), 42)).toEqual({
      profileId: "claude",
      cwd: "C:\\work\\app",
      title: "Claude Code",
      color: "#4a8fe7",
      emoji: "\u{1F680}",
      cliSessionId: ID,
      closedAt: 42,
    });
  });

  it("works without marker and conversation", () => {
    const shell = closedEntry(
      session({ profileId: "pwsh", marker: undefined, cliSessionId: undefined }),
      7,
    );
    expect(shell).toMatchObject({ color: null, emoji: null, cliSessionId: null });
  });

  it("gives nothing for sessions that are not recorded or would not keep", () => {
    expect(closedEntry(session({ kind: "update" }), 1)).toBeNull();
    expect(closedEntry(session({ title: "a\u001b[2J" }), 1)).toBeNull();
    expect(closedEntry(session({ cwd: " " }), 1)).toBeNull();
    expect(closedEntry(session({ title: "x".repeat(257) }), 1)).toBeNull();
    expect(closedEntry(session(), Number.NaN)).toBeNull();
  });
});

describe("pushClosed", () => {
  it("puts the newest first and keeps at most KEEP_LIMIT", () => {
    let list: ClosedSession[] = [];
    for (let index = 0; index < KEEP_LIMIT + 3; index += 1) {
      list = pushClosed(list, entry({ cwd: `/srv/${String(index)}`, closedAt: index }));
    }
    expect(list).toHaveLength(KEEP_LIMIT);
    expect(list[0]?.cwd).toBe(`/srv/${String(KEEP_LIMIT + 2)}`);
  });

  it("keeps one entry per conversation", () => {
    const first = entry({ cwd: "/a", cliSessionId: ID, closedAt: 1 });
    const again = entry({ cwd: "/b", cliSessionId: ID, closedAt: 2 });
    expect(pushClosed([first], again)).toEqual([again]);
  });

  it("keeps one entry per program and folder, also spelled another way", () => {
    const first = entry({ cwd: "C:\\Work\\App\\", closedAt: 1, cliSessionId: OTHER_ID });
    const again = entry({ cwd: "c:/work/app", closedAt: 2, cliSessionId: ID });
    expect(pushClosed([first], again)).toEqual([again]);
    const elsewhere = entry({ profileId: "codex", closedAt: 3 });
    expect(pushClosed([again], elsewhere)).toEqual([elsewhere, again]);
  });

  it("refuses an entry the backend would refuse", () => {
    const list = [entry()];
    expect(pushClosed(list, entry({ profileId: " " }))).toEqual(list);
  });
});

describe("withoutClosed", () => {
  it("takes out exactly that entry", () => {
    const a = entry({ closedAt: 1 });
    const b = entry({ cwd: "/other", closedAt: 2 });
    expect(withoutClosed([b, a], { ...a })).toEqual([b]);
  });
});

describe("shownClosed", () => {
  it("hides what a session holds and shows at most SHOW_LIMIT", () => {
    const held = entry({ cwd: "/held", cliSessionId: ID, closedAt: 99 });
    const rest = Array.from({ length: 7 }, (_, index) =>
      entry({ cwd: `/srv/${String(index)}`, closedAt: index }),
    );
    const open = { profileId: "claude", cwd: "/elsewhere" };
    const shown = shownClosed([held, ...rest], [{ ...open, cliSessionId: ID.toUpperCase() }, open]);
    expect(shown).toHaveLength(SHOW_LIMIT);
    expect(shown).not.toContain(held);
    expect(shownClosed([held], [{ ...open, cliSessionId: null }])).toEqual([held]);
  });

  it("hides an entry without a conversation id while that program runs in its folder", () => {
    // Resuming it would pick the last conversation in the folder: the running one's.
    const codex = entry({ profileId: "codex", cwd: "C:\\work\\app", cliSessionId: null });
    expect(shownClosed([codex], [{ profileId: "codex", cwd: "c:/work/app/" }])).toEqual([]);
    expect(shownClosed([codex], [{ profileId: "claude", cwd: "C:\\work\\app" }])).toEqual([codex]);
    expect(shownClosed([codex], [{ profileId: "codex", cwd: "C:\\work\\other" }])).toEqual([codex]);
  });
});

describe("parseRecentlyClosed", () => {
  it("reads what the backend answers", () => {
    const stored = [
      entry({ color: "#4a8fe7", emoji: "\u{2B50}", cliSessionId: ID }),
      entry({ profileId: "pwsh", cwd: "/home/me" }),
    ];
    expect(parseRecentlyClosed(stored)).toEqual(stored);
    // Missing optional fields read as none.
    expect(
      parseRecentlyClosed([{ profileId: "cmd", cwd: "C:\\", title: "cmd", closedAt: 5 }]),
    ).toEqual([entry({ profileId: "cmd", cwd: "C:\\", title: "cmd", closedAt: 5 })]);
  });

  it("gives an empty list for anything malformed", () => {
    expect(parseRecentlyClosed(null)).toEqual([]);
    expect(parseRecentlyClosed({})).toEqual([]);
    expect(parseRecentlyClosed([entry(), "x"])).toEqual([]);
    expect(parseRecentlyClosed([entry({ cwd: "" })])).toEqual([]);
    expect(parseRecentlyClosed([entry({ color: "red" })])).toEqual([]);
    expect(parseRecentlyClosed([entry({ cliSessionId: ID.toUpperCase() })])).toEqual([]);
    expect(parseRecentlyClosed([entry({ closedAt: -1 })])).toEqual([]);
    expect(parseRecentlyClosed([{ ...entry(), title: 5 }])).toEqual([]);
    const twice = [entry({ cliSessionId: ID }), entry({ cwd: "/b", cliSessionId: ID })];
    expect(parseRecentlyClosed(twice)).toEqual([]);
    const many = Array.from({ length: KEEP_LIMIT + 1 }, (_, index) =>
      entry({ cwd: `/srv/${String(index)}` }),
    );
    expect(parseRecentlyClosed(many)).toEqual([]);
  });
});

describe("loadRecentlyClosed and saveRecentlyClosed", () => {
  function backend(answer: Promise<unknown>): ClosedBackend & { saved: unknown[] } {
    const saved: unknown[] = [];
    return {
      saved,
      loadRecentlyClosed: () => answer,
      saveRecentlyClosed: vi.fn((entries: readonly ClosedSession[]) => {
        saved.push(entries);
        return Promise.resolve(null);
      }),
    };
  }

  it("loads the checked list, and none on an error or for a planted one", async () => {
    const stored = [entry({ cliSessionId: ID })];
    expect(await loadRecentlyClosed(backend(Promise.resolve(stored)))).toEqual(stored);
    expect(await loadRecentlyClosed(backend(Promise.reject(new Error("no backend"))))).toEqual([]);
    expect(await loadRecentlyClosed(backend(Promise.resolve([{ cwd: "/x" }])))).toEqual([]);
  });

  it("saves the list as the backend takes it", async () => {
    const fake = backend(Promise.resolve([]));
    const list = [entry()];
    await saveRecentlyClosed(list, fake);
    expect(fake.saved).toEqual([list]);
  });
});
