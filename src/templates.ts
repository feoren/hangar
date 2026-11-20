// Workspace templates: a named set of sessions (program, project folder, private or team, optional
// title) that starts again with one click. The backend keeps them for the main window alone
// (mainStore.ts, `main_store.rs`): a template starts programs in folders, and the panel and file
// viewer windows share localStorage. An older version kept them in localStorage; that list is
// migrated once. A broken value is read as far as it is valid, the rest is dropped. A template
// may also link its agent sessions once they run (ADR 0014), with the member names and the lead
// it keeps; older templates link nothing.

import { folderKey } from "./folders";
import { parseRole, validMemberName, type LinkRole } from "./links";
import { isProgramTitle } from "./sessionLabel";
import { teamCapable, type StartMode, type TeamProfile } from "./startMode";

/** Where an older version kept the templates; read once for the migration, then removed. */
export const TEMPLATES_STORAGE_KEY = "hangar.templates";

export const TEMPLATE_LIMIT = 30;
export const TEMPLATE_SESSION_LIMIT = 16;
export const TEMPLATE_NAME_LENGTH = 60;
const TITLE_LENGTH = 80;
const MAX_PATH_LENGTH = 4096;

export interface TemplateSession {
  profileId: string;
  /** Project folder; for a team session the repository, never its worktree. */
  folder: string;
  mode: StartMode;
  /** A title the user chose; without one the session is named as on any start. */
  title?: string;
  /** Its name in the template's link group; without one it comes from the title. */
  memberName?: string;
  /** Its role in the template's link group; without one it is a plain member. */
  role?: LinkRole;
}

export interface WorkspaceTemplate {
  /** Stable within the list, for React keys and menus. */
  id: string;
  name: string;
  sessions: TemplateSession[];
  /** Its agent sessions form a link group once they run. */
  link?: boolean;
  /** That group starts with "Ask before pasting" on. */
  askFirst?: boolean;
}

/** How a saved template links its sessions. */
export interface TemplateLink {
  link: boolean;
  askFirst: boolean;
}

const ID = /^[a-z0-9-]{1,40}$/;

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f]/;

function cleanText(value: unknown, limit: number): string | null {
  if (typeof value !== "string") return null;
  const text = value.replace(/\s+/g, " ").trim();
  if (text === "" || CONTROL.test(text)) return null;
  return Array.from(text).slice(0, limit).join("");
}

function validFolder(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim() !== "" &&
    value.length <= MAX_PATH_LENGTH &&
    !CONTROL.test(value)
  );
}

function parseSession(value: unknown): TemplateSession | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const profileId: unknown = Reflect.get(value, "profileId");
  const folder: unknown = Reflect.get(value, "folder");
  if (typeof profileId !== "string" || profileId.trim() === "" || profileId.length > 200) {
    return null;
  }
  if (!validFolder(folder)) return null;
  const mode: StartMode = Reflect.get(value, "mode") === "team" ? "team" : "private";
  const title = cleanText(Reflect.get(value, "title"), TITLE_LENGTH);
  const memberName: unknown = Reflect.get(value, "memberName");
  const role: unknown = Reflect.get(value, "role");
  return {
    profileId,
    folder,
    mode,
    ...(title === null ? {} : { title }),
    ...(typeof memberName === "string" && validMemberName(memberName) ? { memberName } : {}),
    ...(role === "lead" || role === "member" ? { role: parseRole(role) } : {}),
  };
}

/** A template's sessions with unique member names and at most one lead, as the backend takes
 * them; a second name or lead is dropped. */
function linkable(sessions: readonly TemplateSession[]): TemplateSession[] {
  const names = new Set<string>();
  let lead = false;
  return sessions.map((session) => {
    const { memberName, role, ...rest } = session;
    const keepName = memberName !== undefined && !names.has(memberName);
    if (keepName) names.add(memberName);
    const keepRole = role !== undefined && !(role === "lead" && lead);
    if (role === "lead" && keepRole) lead = true;
    return {
      ...rest,
      ...(keepName ? { memberName } : {}),
      ...(keepRole ? { role } : {}),
    };
  });
}

/** The template's own fields beyond name and sessions: set only where true. */
function linkFields(link: TemplateLink | undefined): Pick<WorkspaceTemplate, "link" | "askFirst"> {
  if (link?.link !== true) return {};
  return { link: true, ...(link.askFirst ? { askFirst: true } : {}) };
}

function newId(taken: ReadonlySet<string>): string {
  for (let number = taken.size + 1; ; number += 1) {
    const candidate = `template-${String(number)}`;
    if (!taken.has(candidate)) return candidate;
  }
}

/** Reads the stored list; anything that is not a list gives none. */
export function parseTemplates(value: unknown): WorkspaceTemplate[] {
  if (!Array.isArray(value)) return [];
  const result: WorkspaceTemplate[] = [];
  const ids = new Set<string>();
  for (const entry of value) {
    if (result.length >= TEMPLATE_LIMIT) break;
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) continue;
    const name = cleanText(Reflect.get(entry, "name"), TEMPLATE_NAME_LENGTH);
    const list: unknown = Reflect.get(entry, "sessions");
    if (name === null || !Array.isArray(list)) continue;
    const sessions = linkable(
      list
        .map(parseSession)
        .filter((session): session is TemplateSession => session !== null)
        .slice(0, TEMPLATE_SESSION_LIMIT),
    );
    if (sessions.length === 0) continue;
    if (result.some((template) => sameName(template.name, name))) continue;
    const id: unknown = Reflect.get(entry, "id");
    const key = typeof id === "string" && ID.test(id) && !ids.has(id) ? id : newId(ids);
    ids.add(key);
    const link = linkFields({
      link: Reflect.get(entry, "link") === true,
      askFirst: Reflect.get(entry, "askFirst") === true,
    });
    result.push({ id: key, name, sessions, ...link });
  }
  return result;
}

/** The list an older version kept in localStorage, for the one migration (mainStore.ts). */
export function legacyTemplates(storage: Pick<Storage, "getItem">): WorkspaceTemplate[] {
  try {
    const text = storage.getItem(TEMPLATES_STORAGE_KEY);
    return text === null ? [] : parseTemplates(JSON.parse(text));
  } catch {
    return [];
  }
}

function sameName(a: string, b: string): boolean {
  return a.trim().toLocaleLowerCase() === b.trim().toLocaleLowerCase();
}

/** The template that already carries this name, if any (names ignore case). */
export function templateNamed(
  templates: readonly WorkspaceTemplate[],
  name: string,
): WorkspaceTemplate | undefined {
  return templates.find((template) => sameName(template.name, name));
}

/**
 * Saves a template under `name`, linking its sessions as `link` says (not at all without it). A
 * template of the same name is replaced in place, so saving again updates it; a new one goes
 * last. Without a valid name or sessions nothing changes.
 */
export function storeTemplate(
  templates: readonly WorkspaceTemplate[],
  name: string,
  sessions: readonly TemplateSession[],
  link?: TemplateLink,
): WorkspaceTemplate[] {
  const clean = cleanText(name, TEMPLATE_NAME_LENGTH);
  const valid = linkable(
    sessions
      .map(parseSession)
      .filter((session): session is TemplateSession => session !== null)
      .slice(0, TEMPLATE_SESSION_LIMIT),
  );
  if (clean === null || valid.length === 0) return [...templates];
  const fields = linkFields(link);
  const existing = templateNamed(templates, clean);
  if (existing !== undefined) {
    return templates.map((template) =>
      template.id === existing.id
        ? { id: template.id, name: clean, sessions: valid, ...fields }
        : template,
    );
  }
  if (templates.length >= TEMPLATE_LIMIT) return [...templates];
  const id = newId(new Set(templates.map((template) => template.id)));
  return [...templates, { id, name: clean, sessions: valid, ...fields }];
}

/** Why a new name cannot be used for template `id`, `null` when it can. */
export function renameProblem(
  templates: readonly WorkspaceTemplate[],
  id: string,
  name: string,
): "empty" | "taken" | null {
  const clean = cleanText(name, TEMPLATE_NAME_LENGTH);
  if (clean === null) return "empty";
  const other = templateNamed(templates, clean);
  return other !== undefined && other.id !== id ? "taken" : null;
}

export function renameTemplate(
  templates: readonly WorkspaceTemplate[],
  id: string,
  name: string,
): WorkspaceTemplate[] {
  const clean = cleanText(name, TEMPLATE_NAME_LENGTH);
  if (clean === null || renameProblem(templates, id, clean) !== null) return [...templates];
  return templates.map((template) =>
    template.id === id ? { ...template, name: clean } : template,
  );
}

export function deleteTemplate(
  templates: readonly WorkspaceTemplate[],
  id: string,
): WorkspaceTemplate[] {
  return templates.filter((template) => template.id !== id);
}

/** What saving needs to know about a running session. */
export interface SessionSource {
  profileId: string;
  title: string;
  cwd: string;
  kind?: string | undefined;
  mode?: StartMode | undefined;
  worktree?: { repoRoot: string; branch: string } | undefined;
}

/**
 * A title worth keeping: not the name Hangar gives on start ("Claude Code", "Claude Code (2)",
 * "Claude Code · 1a2b3c" in a worktree). `null` when the session carries such a name.
 */
export function chosenTitle(
  session: SessionSource,
  programNames: readonly string[],
): string | null {
  const title = cleanText(session.title, TITLE_LENGTH);
  if (title === null || isProgramTitle(title, programNames)) return null;
  const lower = title.toLocaleLowerCase();
  if (
    session.worktree !== undefined &&
    programNames.some(
      (name) => name.trim() !== "" && lower.startsWith(`${name.toLocaleLowerCase()} · `),
    )
  ) {
    return null;
  }
  return title;
}

/**
 * The template entry for a running session: team sessions remember the repository (a new
 * worktree is made on start), private ones their working folder. Update sessions are left out.
 */
export function templateSession(
  session: SessionSource,
  programNames: readonly string[],
): TemplateSession | null {
  if (session.kind === "update") return null;
  const team = session.mode === "team" && session.worktree !== undefined;
  const folder = team && session.worktree ? session.worktree.repoRoot : session.cwd;
  if (!validFolder(folder)) return null;
  const title = chosenTitle(session, programNames);
  return {
    profileId: session.profileId,
    folder,
    mode: team ? "team" : "private",
    ...(title === null ? {} : { title }),
  };
}

/** One planned start of a template. */
export interface PlannedStart<P> {
  profile: P;
  folder: string;
  mode: StartMode;
  title?: string;
  memberName?: string;
  role?: LinkRole;
}

/**
 * How a template starts with the programs at hand: each entry with its program, team only where
 * the program can be a team session and the folder lies in a git repository (`inRepo`), else
 * private. Entries whose program is gone are named in `skipped` (by profile ID).
 */
export function planTemplate<P extends TeamProfile & { id: string }>(
  template: WorkspaceTemplate,
  profiles: readonly P[],
  inRepo: (folder: string) => boolean,
): { starts: PlannedStart<P>[]; skipped: string[] } {
  const starts: PlannedStart<P>[] = [];
  const skipped: string[] = [];
  for (const entry of template.sessions) {
    const profile = profiles.find((candidate) => candidate.id === entry.profileId);
    if (profile === undefined) {
      if (!skipped.includes(entry.profileId)) skipped.push(entry.profileId);
      continue;
    }
    const team = entry.mode === "team" && teamCapable(profile) && inRepo(entry.folder);
    starts.push({
      profile,
      folder: entry.folder,
      mode: team ? "team" : "private",
      ...(entry.title === undefined ? {} : { title: entry.title }),
      ...(entry.memberName === undefined ? {} : { memberName: entry.memberName }),
      ...(entry.role === undefined ? {} : { role: entry.role }),
    });
  }
  return { starts, skipped };
}

/** Distinct project folders of a template, for one git query before starting. */
export function templateFolders(template: WorkspaceTemplate): string[] {
  const seen = new Set<string>();
  const folders: string[] = [];
  for (const entry of template.sessions) {
    const key = folderKey(entry.folder);
    if (seen.has(key)) continue;
    seen.add(key);
    folders.push(entry.folder);
  }
  return folders;
}

/** How a template start went, said once after all its sessions were tried. */
export type TemplateReport =
  | { kind: "started" }
  | {
      kind: "partial";
      started: number;
      total: number;
      /** Text of the last failed start, `null` when only programs were missing. */
      lastError: string | null;
      /** Profile IDs of programs that are gone. */
      skipped: string[];
    };

/**
 * Sums up a template start: `started` sessions run out of the template's `total`; `skipped`
 * programs were missing, `lastError` is the last failure's text. Anything short of all of them is
 * a partial start, also when none started.
 */
export function templateReport(
  total: number,
  started: number,
  skipped: readonly string[],
  lastError: string | null,
): TemplateReport {
  if (started >= total && skipped.length === 0 && lastError === null) return { kind: "started" };
  return { kind: "partial", started, total, lastError, skipped: [...skipped] };
}
