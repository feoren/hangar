// Entries of quick access (Ctrl+Shift+P). Pure: everything it needs comes in through the context,
// so the rules for which entries are primary (shown without a query) and which are disabled can
// be tested without React.

import type { ReactNode } from "react";

import type { NoteFolder, PathEntry, Profile } from "./api";
import type { MenuGroup, MenuItem } from "./AppMenu";
import { folderName } from "./folders";
import { teamCapable } from "./startMode";
import { shortPath } from "./format";
import type { CommitSummary } from "./gitHistory";
import type { Translate } from "./i18n";
import type { LauncherItem } from "./LauncherDialog";
import type { NoteFolderInfo } from "./notes";
import type { SessionView } from "./sessions";
import { SECTION_NAMES, SETTINGS_SECTIONS, type SettingsSection } from "./settingsSections";
import { snippetLabel, type Snippet } from "./snippets";
import { LAYOUT_IDS, LAYOUT_NAMES, type LayoutId } from "./snapLayouts";

/** A working entry of the app menu, as a command of quick access. */
export interface MenuCommand {
  label: string;
  /** Menu and submenu names above the entry, e.g. ["View"]. */
  path: string[];
  shortcut?: string;
  run: () => void;
}

/**
 * The entries of the app menu that can run right now, in menu order. Disabled entries are left
 * out: quick access shows only what works.
 */
export function menuCommands(groups: readonly MenuGroup[]): MenuCommand[] {
  const commands: MenuCommand[] = [];
  const walk = (items: readonly MenuItem[], path: string[]) => {
    for (const item of items) {
      if (item.type === "separator" || item.disabled === true) continue;
      if (item.type === "submenu") {
        walk(item.items, [...path, item.label]);
        continue;
      }
      commands.push({
        label: item.label,
        path,
        ...(item.shortcut === undefined ? {} : { shortcut: item.shortcut }),
        run: item.run,
      });
    }
  };
  for (const group of groups) walk(group.items, [group.label]);
  return commands;
}

/** Files (no folders) of the active session's path index; found only by searching. */
export function fileItems(
  entries: readonly PathEntry[],
  open: (path: string) => void,
  icon: ReactNode,
): LauncherItem[] {
  return entries
    .filter((entry) => entry.kind === "file")
    .map((entry) => {
      const slash = entry.path.lastIndexOf("/");
      const name = entry.path.slice(slash + 1);
      return {
        key: `file:${entry.path}`,
        section: "files",
        label: name,
        ...(slash < 0 ? {} : { detail: entry.path.slice(0, slash) }),
        icon,
        run: () => {
          open(entry.path);
        },
      };
    });
}

/** Recent commits of the active session's folder; found by subject, id or author. */
export function commitItems(
  commits: readonly CommitSummary[],
  open: (sha: string) => void,
  t: Translate,
  icon: ReactNode,
): LauncherItem[] {
  return commits.map((commit) => ({
    key: `commit:${commit.sha}`,
    section: "commits",
    label: commit.subject === "" ? t("history.noSubject") : commit.subject,
    detail: commit.shortSha,
    keywords: `${commit.sha} ${commit.author}`,
    icon,
    run: () => {
      open(commit.sha);
    },
  }));
}

export interface LauncherContext {
  sessions: readonly SessionView[];
  /** Program name per profile ID, a search keyword for its sessions. */
  programName: (profileId: string) => string | undefined;
  startable: readonly Profile[];
  /**
   * Folders to start in, each once (`startPlaces`): the start target first, then running
   * projects, favorites and recent folders. `worktree`: the folder lies in a git repository.
   */
  places: readonly { path: string; worktree: boolean }[];
  noteFolders: readonly NoteFolderInfo[];
  snippets: readonly Snippet[];
  /** A snippet can go into the visible session (its process runs). */
  canInsert: boolean;
  snapLayout: LayoutId;
  canBroadcast: boolean;
  changesOpen: boolean;
  t: Translate;
  badge: (profileId: string) => ReactNode;
  notesIcon: ReactNode;
  activate: (id: number) => void;
  start: (profile: Profile, folder: string) => void;
  /** A team session (startMode.ts): its own worktree, the agent gate, visible to the team. */
  startAsTeam: (profile: Profile, folder: string) => void;
  toggleChanges: () => void;
  openNotes: (folder: NoteFolder) => void;
  insertSnippet: (snippet: Snippet) => void;
  setSnapLayout: (layout: LayoutId) => void;
  /** Opens the layout picker for the active session; `null` without sessions. */
  openSnapPicker: (() => void) | null;
  openBroadcast: () => void;
  /**
   * Sub-agent view of the active session: `null` when it has no sub-agents and no view is open;
   * `shown` makes the entry hide the view.
   */
  subAgents: { shown: boolean; toggle: () => void } | null;
  /** Opens the Settings dialog on this section. */
  openSettings: (section: SettingsSection) => void;
  /** The app menu: its working entries become commands, so every action can be found here. */
  menu: readonly MenuGroup[];
  /** Labels of menu entries that quick access leaves out (e.g. the one that opens it). */
  skipLabels: readonly string[];
}

export function buildLauncherItems(ctx: LauncherContext): LauncherItem[] {
  const { t } = ctx;
  const items: LauncherItem[] = [];
  for (const session of ctx.sessions) {
    items.push({
      key: `session:${String(session.id)}`,
      section: "sessions",
      label: [session.marker?.emoji, session.title].filter(Boolean).join(" "),
      detail: shortPath(session.cwd),
      keywords: [ctx.programName(session.profileId) ?? "", session.cwd].join(" "),
      primary: true,
      icon: ctx.badge(session.profileId),
      run: () => {
        ctx.activate(session.id);
      },
    });
  }
  // Program in project: the start target first, then running projects, favorites, recent.
  const target = ctx.places[0]?.path ?? null;
  for (const place of ctx.places) {
    for (const profile of ctx.startable) {
      items.push({
        key: `start:${profile.id}:${place.path}`,
        section: "start",
        label: t("launcher.startIn", { program: profile.name, folder: folderName(place.path) }),
        detail: shortPath(place.path),
        keywords: place.path,
        // Without sessions, the programs in the target folder are the obvious next step.
        primary: ctx.sessions.length === 0 && place.path === target,
        icon: ctx.badge(profile.id),
        run: () => {
          ctx.start(profile, place.path);
        },
      });
    }
  }
  // Team sessions only where git allows a worktree and only for programs that can be one.
  for (const place of ctx.places) {
    if (!place.worktree) continue;
    for (const profile of ctx.startable) {
      if (!teamCapable(profile)) continue;
      items.push({
        key: `team:${profile.id}:${place.path}`,
        section: "start",
        label: t("launcher.startAsTeam", {
          program: profile.name,
          folder: folderName(place.path),
        }),
        detail: shortPath(place.path),
        keywords: `${place.path} team worktree git`,
        icon: ctx.badge(profile.id),
        run: () => {
          ctx.startAsTeam(profile, place.path);
        },
      });
    }
  }
  for (const info of ctx.noteFolders) {
    if (info.noteCount === 0) continue;
    const folder: NoteFolder = { path: info.path };
    const label = info.path === null ? t("notes.general") : folderName(info.path);
    items.push({
      key: `notes:${info.folderKey}`,
      section: "notes",
      label: t("launcher.notesIn", { folder: label }),
      ...(info.path === null ? {} : { detail: shortPath(info.path) }),
      icon: ctx.notesIcon,
      run: () => {
        ctx.openNotes(folder);
      },
    });
  }
  for (const snippet of ctx.snippets) {
    items.push({
      key: `snippet:${snippet.id}`,
      section: "snippets",
      label: snippetLabel(snippet),
      keywords: snippet.text.slice(0, 200),
      primary: true,
      disabled: !ctx.canInsert,
      ...(ctx.canInsert ? {} : { detail: t("launcher.noSession") }),
      run: () => {
        ctx.insertSnippet(snippet);
      },
    });
  }
  const openSnapPicker = ctx.openSnapPicker;
  items.push({
    key: "snap",
    section: "commands",
    label: t("snap.choose"),
    keywords: "layout snap split",
    primary: true,
    disabled: openSnapPicker === null,
    run: () => {
      openSnapPicker?.();
    },
  });
  for (const layout of LAYOUT_IDS) {
    items.push({
      key: `view:${layout}`,
      section: "commands",
      label: t("launcher.view", { mode: t(LAYOUT_NAMES[layout]) }),
      ...(layout === ctx.snapLayout ? { detail: t("folders.current") } : {}),
      keywords: "layout snap split",
      run: () => {
        ctx.setSnapLayout(layout);
      },
    });
  }
  const subAgents = ctx.subAgents;
  if (subAgents !== null) {
    items.push({
      key: "subAgents",
      section: "commands",
      label: subAgents.shown ? t("session.hideSubAgents") : t("session.showSubAgents"),
      keywords: "agents subagents task",
      primary: true,
      run: subAgents.toggle,
    });
  }
  items.push({
    key: "changes",
    section: "commands",
    label: ctx.changesOpen ? t("changes.hide") : t("changes.show"),
    keywords: "git diff",
    run: ctx.toggleChanges,
  });
  items.push({
    key: "broadcast",
    section: "commands",
    label: t("menu.broadcast"),
    primary: true,
    disabled: !ctx.canBroadcast,
    run: ctx.openBroadcast,
  });
  for (const section of SETTINGS_SECTIONS) {
    items.push({
      key: `settings:${section}`,
      section: "settings",
      label: t(SECTION_NAMES[section]),
      detail: t("settings.title"),
      run: () => {
        ctx.openSettings(section);
      },
    });
  }
  // Every other menu entry that works right now. Entries the commands above already cover (same
  // label, or a layout already offered as "View: …") appear once.
  const known = new Set([
    ...items.filter((item) => item.section === "commands").map((item) => item.label),
    ...LAYOUT_IDS.map((layout) => t(LAYOUT_NAMES[layout])),
    ...ctx.skipLabels,
  ]);
  for (const command of menuCommands(ctx.menu)) {
    if (known.has(command.label)) continue;
    known.add(command.label);
    const path = command.path.join(" › ");
    items.push({
      key: `menu:${path}:${command.label}`,
      section: "commands",
      label: command.label,
      detail: command.shortcut ?? path,
      keywords: path,
      run: command.run,
    });
  }
  return items;
}
