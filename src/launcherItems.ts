// Entries of quick access (Ctrl+Shift+P). Pure: everything it needs comes in through the context,
// so the rules for which entries are primary (shown without a query) and which are disabled can
// be tested without React.

import type { ReactNode } from "react";

import type { NoteFolder, Profile } from "./api";
import { folderKey, folderName, type FolderPrefs } from "./folders";
import { shortPath } from "./format";
import type { MessageKey, Translate } from "./i18n";
import type { LauncherItem } from "./LauncherDialog";
import type { NoteFolderInfo } from "./notes";
import type { SessionView } from "./sessions";
import { snippetLabel, type Snippet } from "./snippets";
import { SPLIT_MODES, type SplitMode } from "./split";

export const SPLIT_MODE_NAMES: Record<SplitMode, MessageKey> = {
  single: "menu.viewSingle",
  columns: "menu.viewColumns",
  grid: "menu.viewGrid",
};

export interface LauncherContext {
  sessions: readonly SessionView[];
  /** Program name per profile ID, a search keyword for its sessions. */
  programName: (profileId: string) => string | undefined;
  startable: readonly Profile[];
  /** "Starts in" folder of new sessions. */
  cwd: string | null;
  folders: FolderPrefs;
  noteFolders: readonly NoteFolderInfo[];
  snippets: readonly Snippet[];
  /** A snippet can go into the visible session (its process runs). */
  canInsert: boolean;
  splitMode: SplitMode;
  canBroadcast: boolean;
  t: Translate;
  badge: (profileId: string) => ReactNode;
  notesIcon: ReactNode;
  activate: (id: number) => void;
  start: (profile: Profile, folder: string) => void;
  openNotes: (folder: NoteFolder) => void;
  insertSnippet: (snippet: Snippet) => void;
  setSplitMode: (mode: SplitMode) => void;
  openBroadcast: () => void;
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
  // Program in folder: the "Starts in" folder first, then favorites and recent folders.
  const seen = new Set<string>();
  const places = [ctx.cwd, ...ctx.folders.favorites, ...ctx.folders.recent].filter(
    (path): path is string => {
      if (path === null || seen.has(folderKey(path))) return false;
      seen.add(folderKey(path));
      return true;
    },
  );
  for (const place of places) {
    for (const profile of ctx.startable) {
      items.push({
        key: `start:${profile.id}:${place}`,
        section: "start",
        label: t("launcher.startIn", { program: profile.name, folder: folderName(place) }),
        detail: shortPath(place),
        keywords: place,
        // Without sessions, the programs in the current folder are the obvious next step.
        primary: ctx.sessions.length === 0 && place === ctx.cwd,
        icon: ctx.badge(profile.id),
        run: () => {
          ctx.start(profile, place);
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
  for (const mode of SPLIT_MODES) {
    items.push({
      key: `view:${mode}`,
      section: "commands",
      label: t("launcher.view", { mode: t(SPLIT_MODE_NAMES[mode]) }),
      ...(mode === ctx.splitMode ? { detail: t("folders.current") } : {}),
      primary: true,
      run: () => {
        ctx.setSplitMode(mode);
      },
    });
  }
  items.push({
    key: "broadcast",
    section: "commands",
    label: t("menu.broadcast"),
    primary: true,
    disabled: !ctx.canBroadcast,
    run: ctx.openBroadcast,
  });
  return items;
}
