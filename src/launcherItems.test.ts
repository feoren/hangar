import { describe, expect, it, vi } from "vitest";

import type { Profile } from "./api";
import { createTranslator } from "./i18n";
import { buildLauncherItems, type LauncherContext } from "./launcherItems";
import type { SessionView } from "./sessions";

function profile(id: string): Profile {
  return {
    id,
    name: id,
    command: id,
    args: [],
    cwd: null,
    icon: null,
    source: "catalog",
    installed: true,
    homepage: null,
    installHint: null,
  };
}

function session(id: number): SessionView {
  return {
    id,
    profileId: "claude",
    title: `Claude ${String(id)}`,
    cwd: "C:\\src\\hangar",
    status: "running",
    exitCode: null,
    createdAt: 0,
  };
}

function context(patch: Partial<LauncherContext> = {}): LauncherContext {
  return {
    sessions: [],
    programName: () => undefined,
    startable: [profile("claude"), profile("pwsh")],
    places: [
      { path: "C:\\src\\hangar", worktree: false },
      { path: "C:\\src\\notes", worktree: false },
    ],
    noteFolders: [],
    snippets: [{ id: "a", text: "Write tests" }],
    canInsert: false,
    snapLayout: "single",
    canBroadcast: false,
    changesOpen: false,
    t: createTranslator("en"),
    badge: () => null,
    notesIcon: null,
    activate: vi.fn(),
    start: vi.fn(),
    startInWorktree: vi.fn(),
    toggleChanges: vi.fn(),
    openNotes: vi.fn(),
    insertSnippet: vi.fn(),
    setSnapLayout: vi.fn(),
    openSnapPicker: null,
    openBroadcast: vi.fn(),
    subAgents: null,
    ...patch,
  };
}

describe("buildLauncherItems", () => {
  it("makes programs in the current folder primary only while no session runs", () => {
    const empty = buildLauncherItems(context());
    const primaryStarts = empty.filter((item) => item.section === "start" && item.primary === true);
    expect(primaryStarts.map((item) => item.key)).toEqual([
      "start:claude:C:\\src\\hangar",
      "start:pwsh:C:\\src\\hangar",
    ]);

    const busy = buildLauncherItems(context({ sessions: [session(1)] }));
    expect(busy.some((item) => item.section === "start" && item.primary === true)).toBe(false);
    expect(busy.find((item) => item.key === "session:1")?.primary).toBe(true);
  });

  it("offers every program in every place", () => {
    const starts = buildLauncherItems(context()).filter((item) => item.section === "start");
    expect(starts).toHaveLength(4);
  });

  it("disables snippets without a running session and says why", () => {
    const off = buildLauncherItems(context()).find((item) => item.section === "snippets");
    expect(off?.disabled).toBe(true);
    expect(off?.detail).toBe(createTranslator("en")("launcher.noSession"));

    const on = buildLauncherItems(context({ canInsert: true })).find(
      (item) => item.section === "snippets",
    );
    expect(on?.disabled).toBe(false);
    expect(on?.detail).toBeUndefined();
  });

  it("disables broadcast without candidates and marks the current view", () => {
    const items = buildLauncherItems(context({ snapLayout: "mainStack" }));
    expect(items.find((item) => item.key === "broadcast")?.disabled).toBe(true);
    expect(items.find((item) => item.key === "view:mainStack")?.detail).toBe(
      createTranslator("en")("folders.current"),
    );
    expect(items.find((item) => item.key === "view:single")?.detail).toBeUndefined();
  });

  it("offers every snap layout and the picker only with a session", () => {
    const keys = buildLauncherItems(context())
      .filter((item) => item.key.startsWith("view:"))
      .map((item) => item.key);
    expect(keys).toEqual([
      "view:single",
      "view:pair",
      "view:triple",
      "view:mainStack",
      "view:quad",
    ]);
    expect(buildLauncherItems(context()).find((item) => item.key === "snap")?.disabled).toBe(true);
    const openSnapPicker = vi.fn();
    const ctx = context({ openSnapPicker });
    const snap = buildLauncherItems(ctx).find((item) => item.key === "snap");
    expect(snap?.disabled).toBe(false);
    snap?.run();
    expect(openSnapPicker).toHaveBeenCalledOnce();
  });

  it("skips note folders without notes", () => {
    const items = buildLauncherItems(
      context({
        noteFolders: [
          { path: null, folderKey: "general", noteCount: 0, updatedAt: null },
          { path: "C:\\src\\hangar", folderKey: "h", noteCount: 2, updatedAt: null },
        ],
      }),
    );
    expect(items.filter((item) => item.section === "notes").map((item) => item.key)).toEqual([
      "notes:h",
    ]);
  });

  it("offers an own worktree only for git folders, and never as a primary entry", () => {
    const plain = buildLauncherItems(context());
    expect(plain.some((item) => item.key.startsWith("worktree:"))).toBe(false);

    const ctx = context({
      places: [
        { path: "C:\\src\\hangar", worktree: true },
        { path: "C:\\src\\notes", worktree: false },
      ],
    });
    const items = buildLauncherItems(ctx).filter((item) => item.key.startsWith("worktree:"));
    expect(items.map((item) => item.key)).toEqual([
      "worktree:claude:C:\\src\\hangar",
      "worktree:pwsh:C:\\src\\hangar",
    ]);
    expect(items.every((item) => item.primary !== true)).toBe(true);
    items[0]?.run();
    expect(ctx.startInWorktree).toHaveBeenCalledWith(
      expect.objectContaining({ id: "claude" }),
      "C:\\src\\hangar",
    );
  });

  it("names the changes command after the panel state", () => {
    const en = createTranslator("en");
    const closed = buildLauncherItems(context()).find((item) => item.key === "changes");
    expect(closed?.label).toBe(en("changes.show"));
    const open = buildLauncherItems(context({ changesOpen: true })).find(
      (item) => item.key === "changes",
    );
    expect(open?.label).toBe(en("changes.hide"));
  });

  it("runs the matching action", () => {
    const ctx = context({ sessions: [session(7)] });
    buildLauncherItems(ctx)
      .find((item) => item.key === "session:7")
      ?.run();
    expect(ctx.activate).toHaveBeenCalledWith(7);
  });

  it("offers the sub-agent view only for an active session that has sub-agents", () => {
    expect(buildLauncherItems(context()).some((item) => item.key === "subAgents")).toBe(false);
    const toggle = vi.fn();
    const show = buildLauncherItems(context({ subAgents: { shown: false, toggle } })).find(
      (item) => item.key === "subAgents",
    );
    expect(show?.label).toBe(createTranslator("en")("session.showSubAgents"));
    show?.run();
    expect(toggle).toHaveBeenCalledOnce();
    const hide = buildLauncherItems(context({ subAgents: { shown: true, toggle } })).find(
      (item) => item.key === "subAgents",
    );
    expect(hide?.label).toBe(createTranslator("en")("session.hideSubAgents"));
  });
});
