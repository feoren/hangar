// Main and second line of a session row in the Sessions group. A session still called after its
// program ("Claude Code", "Claude Code (2)") is named after its project folder instead, and the
// program moves to the second line, as it does for a name that only repeats the folder; any other
// name the person gave keeps its place. Pure.

import { folderName } from "./folders";

export interface SessionLabel {
  /** Main line: the project folder, or the session's own name. */
  primary: string;
  /** Second line: the program when the folder leads, otherwise the folder. */
  secondary: string;
  /** The folder leads (the program name is on the second line). */
  byFolder: boolean;
}

export interface LabelInput {
  id: number;
  title: string;
  cwd: string;
  profileId: string;
  kind?: string | undefined;
}

/**
 * Who the status line names as its subject ("Claude Code is working"): the program, or for a
 * numbered twin its numbered title, so two Claude Codes in one folder stay apart. Without a
 * known program, the session's title.
 */
export function statusSubject(
  label: SessionLabel | null,
  programName: string | undefined,
  title: string,
): string {
  if (label?.byFolder === true) return label.secondary;
  return programName ?? title;
}

/**
 * The folder at the start of the detail line; `null` when the folder already is the row's title
 * (the program it replaced there now leads the status line).
 */
export function detailPlace(label: SessionLabel | null, folder: string): string | null {
  if (label === null) return folder;
  return label.byFolder ? null : label.secondary;
}

/**
 * Last folder of a path, for Windows, POSIX and WSL paths alike; separators at the end do not
 * count. A drive or the file system root stays as it is ("C:\", "/"). Empty without a path.
 */
export function projectFolder(cwd: string): string {
  const trimmed = cwd.trim();
  if (trimmed === "") return "";
  return folderName(trimmed);
}

/**
 * Whether `title` is only the name Hangar gave on start: the program name, optionally numbered
 * as `numberedTitle` (sessions.ts) does it ("Claude Code (2)"). Case and outer spaces do not matter.
 */
export function isProgramTitle(title: string, programNames: readonly string[]): boolean {
  const wanted = title.trim().toLocaleLowerCase();
  if (wanted === "") return true;
  return programNames.some((name) => {
    const base = name.trim().toLocaleLowerCase();
    if (base === "") return false;
    if (wanted === base) return true;
    if (!wanted.startsWith(`${base} (`)) return false;
    return /^\(\d+\)$/.test(wanted.slice(base.length + 1));
  });
}

/**
 * Labels for the rows of one list, by session id. `programName` gives a profile's display name
 * (undefined for an unknown profile). Two sessions of the same program in folders of the same
 * name keep their numbered titles on the second line, so they stay apart.
 */
export function sessionLabels(
  sessions: readonly LabelInput[],
  programName: (profileId: string) => string | undefined,
): Map<number, SessionLabel> {
  const labels = new Map<number, SessionLabel>();
  const program = (session: LabelInput) => programName(session.profileId) ?? session.profileId;
  // A name that only repeats the folder ("hangar" in ...\hangar) says nothing the folder line
  // would not say, so the program takes the second line there as well.
  const repeatsFolder = (session: LabelInput) =>
    session.title.trim().toLocaleLowerCase() === projectFolder(session.cwd).toLocaleLowerCase();
  const leadsByFolder = (session: LabelInput) =>
    session.kind !== "update" &&
    projectFolder(session.cwd) !== "" &&
    (repeatsFolder(session) ||
      isProgramTitle(session.title, [program(session), session.profileId]));
  const twins = new Map<string, number>();
  const twinKey = (session: LabelInput) =>
    `${projectFolder(session.cwd).toLocaleLowerCase()}\u0000${program(session).toLocaleLowerCase()}`;
  for (const session of sessions) {
    if (!leadsByFolder(session)) continue;
    const key = twinKey(session);
    twins.set(key, (twins.get(key) ?? 0) + 1);
  }
  for (const session of sessions) {
    const folder = projectFolder(session.cwd);
    if (!leadsByFolder(session)) {
      labels.set(session.id, { primary: session.title, secondary: folder, byFolder: false });
      continue;
    }
    const shared = (twins.get(twinKey(session)) ?? 0) > 1;
    const title = session.title.trim();
    labels.set(session.id, {
      primary: folder,
      secondary: shared && title !== "" && !repeatsFolder(session) ? title : program(session),
      byFolder: true,
    });
  }
  return labels;
}

/**
 * Title of a row inside a project group: its header already names the folder, so the row leads
 * with the program (or the session's own name) instead of repeating the folder.
 */
export function projectRowName(label: SessionLabel | null, title: string): string {
  if (label === null) return title;
  return label.byFolder ? label.secondary : label.primary;
}
