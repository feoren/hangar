import { describe, expect, it, vi } from "vitest";

import type { Profile } from "./api";
import { createTranslator } from "./i18n";
import type { MenuGroup } from "./AppMenu";
import {
  buildLauncherItems,
  commitItems,
  fileItems,
  menuCommands,
  type LauncherContext,
} from "./launcherItems";
import type { SessionView } from "./sessions";

function profile(id: string, teamCapable = false): Profile {
  return {
    teamCapable,
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
    startable: [profile("claude", true), profile("pwsh")],
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
    startAsTeam: vi.fn(),
    templates: [],
    templateIcon: null,
    startTemplate: vi.fn(),
    toggleChanges: vi.fn(),
    openNotes: vi.fn(),
    insertSnippet: vi.fn(),
    setSnapLayout: vi.fn(),
    openSnapPicker: null,
    openBroadcast: vi.fn(),
    subAgents: null,
    openSettings: vi.fn(),
    menu: [],
    skipLabels: [],
    ...patch,
  };
}

describe("quick access beyond sessions", () => {
  it("offers every settings section", () => {
    const openSettings = vi.fn();
    const items = buildLauncherItems(context({ openSettings }));
    const settings = items.filter((item) => item.section === "settings");
    expect(settings.map((item) => item.key)).toEqual([
      "settings:general",
      "settings:appearance",
      "settings:attention",
      "settings:programs",
      "settings:snippets",
      "settings:account",
      "settings:updates",
    ]);
    settings[1]?.run();
    expect(openSettings).toHaveBeenCalledWith("appearance");
  });

  it("turns working menu entries into commands, once each", () => {
    const run = vi.fn();
    const t = createTranslator("en");
    const menu: MenuGroup[] = [
      {
        label: "File",
        items: [
          { type: "action", label: t("menu.launcher"), run },
          { type: "action", label: "Pick folder", run },
          { type: "separator" },
          { type: "action", label: "Restart", disabled: true, run },
          {
            type: "submenu",
            label: "New session",
            items: [{ type: "action", label: "Codex", run }],
          },
        ],
      },
      {
        label: "View",
        items: [
          { type: "action", label: "Overview of all sessions", shortcut: "Ctrl+Shift+M", run },
          // Already a command of its own and a layout offered as "View: …".
          { type: "action", label: t("changes.show"), run },
          { type: "action", label: "One session", run },
        ],
      },
    ];
    const items = buildLauncherItems(context({ menu, skipLabels: [t("menu.launcher")] }));
    const fromMenu = items.filter((item) => item.key.startsWith("menu:"));
    expect(fromMenu.map((item) => [item.label, item.detail])).toEqual([
      ["Pick folder", "File"],
      ["Codex", "File › New session"],
      ["Overview of all sessions", "Ctrl+Shift+M"],
    ]);
    expect(fromMenu.every((item) => item.section === "commands")).toBe(true);
  });

  it("lists files, not folders, by name with their folder", () => {
    const open = vi.fn();
    const items = fileItems(
      [
        { path: "src", kind: "dir" },
        { path: "src/App.tsx", kind: "file" },
        { path: "README.md", kind: "file" },
      ],
      open,
      null,
    );
    expect(items.map((item) => [item.label, item.detail])).toEqual([
      ["App.tsx", "src"],
      ["README.md", undefined],
    ]);
    expect(items.every((item) => item.primary !== true)).toBe(true);
    items[0]?.run();
    expect(open).toHaveBeenCalledWith("src/App.tsx");
  });

  it("finds commits by subject and id", () => {
    const open = vi.fn();
    const sha = "a".repeat(40);
    const items = commitItems(
      [
        {
          sha,
          shortSha: "aaaaaaa",
          subject: "",
          author: "Ada",
          authoredAt: 0,
          pushed: true,
        },
      ],
      open,
      createTranslator("en"),
      null,
    );
    expect(items[0]?.label).toBe("(no subject)");
    expect(items[0]?.keywords).toContain(sha);
    items[0]?.run();
    expect(open).toHaveBeenCalledWith(sha);
  });

  it("leaves out everything of a disabled submenu", () => {
    const run = vi.fn();
    const commands = menuCommands([
      {
        label: "File",
        items: [
          {
            type: "submenu",
            label: "Export",
            disabled: true,
            items: [{ type: "action", label: "Text", run }],
          },
        ],
      },
    ]);
    expect(commands).toEqual([]);
  });
});

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

  it("offers team sessions only for git folders and team-capable programs, never as primary", () => {
    const plain = buildLauncherItems(context());
    expect(plain.some((item) => item.key.startsWith("team:"))).toBe(false);

    const ctx = context({
      places: [
        { path: "C:\\src\\hangar", worktree: true },
        { path: "C:\\src\\notes", worktree: false },
      ],
    });
    const items = buildLauncherItems(ctx).filter((item) => item.key.startsWith("team:"));
    // pwsh is a plain terminal: no team session for it.
    expect(items.map((item) => item.key)).toEqual(["team:claude:C:\\src\\hangar"]);
    expect(items.every((item) => item.primary !== true)).toBe(true);
    items[0]?.run();
    expect(ctx.startAsTeam).toHaveBeenCalledWith(
      expect.objectContaining({ id: "claude" }),
      "C:\\src\\hangar",
    );
  });

  it("offers each workspace template as a start that runs the whole set", () => {
    const ctx = context({
      templates: [
        {
          id: "template-1",
          name: "Review",
          sessions: [
            { profileId: "claude", folder: "C:\\src\\hangar", mode: "team" },
            { profileId: "pwsh", folder: "C:\\src\\hangar", mode: "private" },
          ],
        },
      ],
    });
    const items = buildLauncherItems(ctx).filter((item) => item.key.startsWith("template:"));
    expect(items).toHaveLength(1);
    expect(items[0]?.section).toBe("start");
    expect(items[0]?.primary).toBe(true);
    expect(items[0]?.label).toContain("Review");
    items[0]?.run();
    expect(ctx.startTemplate).toHaveBeenCalledWith("template-1");
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
