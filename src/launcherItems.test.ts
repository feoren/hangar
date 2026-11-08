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
    cwd: "C:\\src\\hangar",
    folders: { favorites: ["C:\\src\\notes", "c:\\src\\hangar\\\\"], recent: ["C:\\src\\notes"] },
    noteFolders: [],
    snippets: [{ id: "a", text: "Write tests" }],
    canInsert: false,
    splitMode: "single",
    canBroadcast: false,
    t: createTranslator("en"),
    badge: () => null,
    notesIcon: null,
    activate: vi.fn(),
    start: vi.fn(),
    openNotes: vi.fn(),
    insertSnippet: vi.fn(),
    setSplitMode: vi.fn(),
    openBroadcast: vi.fn(),
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

  it("lists every folder once, ignoring case and trailing separators", () => {
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
    const items = buildLauncherItems(context({ splitMode: "grid" }));
    expect(items.find((item) => item.key === "broadcast")?.disabled).toBe(true);
    expect(items.find((item) => item.key === "view:grid")?.detail).toBe(
      createTranslator("en")("folders.current"),
    );
    expect(items.find((item) => item.key === "view:single")?.detail).toBeUndefined();
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

  it("runs the matching action", () => {
    const ctx = context({ sessions: [session(7)] });
    buildLauncherItems(ctx)
      .find((item) => item.key === "session:7")
      ?.run();
    expect(ctx.activate).toHaveBeenCalledWith(7);
  });
});
