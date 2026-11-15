// Session handover: a new session of another agent in the same folder gets a prompt with the
// last request, the changed files and the project notes, pasted without Enter. Pure: the caller
// reads the terminal, git and the notes; this file decides who can take over, in which mode, and
// builds the text within fixed bounds.

import type { Profile, SessionMode, Worktree } from "./api";
import type { AttentionState } from "./attention";
import type { Translate } from "./i18n";
import { shellKind } from "./pathQuote";
import { teamCapable } from "./startMode";

/** The whole prompt stays below this, so it is a paste and never a wall of text. */
export const HANDOVER_MAX_CHARS = 8000;
/** The last prompt is quoted up to this many characters. */
export const PROMPT_MAX_CHARS = 1500;
/** Changed files listed by name; more are counted. */
export const FILES_MAX = 40;
/** Notes listed, each with an excerpt of up to NOTE_EXCERPT_MAX characters. */
export const NOTES_MAX = 5;
export const NOTE_EXCERPT_MAX = 160;
/** How long the new agent gets to start before the text is pasted anyway. */
export const READY_TIMEOUT_MS = 30_000;
/** Output must have paused this long: the agent's prompt is drawn and waits for input. */
export const READY_QUIET_MS = 1000;
const READY_POLL_MS = 250;

export interface HandoverNote {
  title: string;
  excerpt: string;
}

export interface HandoverInput {
  /** The program that worked so far ("Claude Code"). */
  from: string;
  folder: string;
  lastPrompt: string | null;
  /** Changed paths, as git lists them. */
  files: readonly string[];
  notes: readonly HandoverNote[];
}

// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const CONTROLS = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g;

function clean(text: string): string {
  return text.replace(/\r\n?/g, "\n").replace(CONTROLS, "");
}

function oneLine(text: string): string {
  return clean(text).replace(/\s+/g, " ").trim();
}

function cut(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length > max
    ? `${chars
        .slice(0, max - 1)
        .join("")
        .trimEnd()}…`
    : text;
}

/**
 * A submitted line that counts as the user's last request: not empty, not a slash command and
 * not a one-character answer to a menu or a yes/no question ("1", "y"). Capped at
 * PROMPT_MAX_CHARS, which is all the handover quotes.
 */
export function handoverRequest(line: string): string | null {
  const text = oneLine(line);
  if (text.length < 2 || text.startsWith("/")) return null;
  return cut(text, PROMPT_MAX_CHARS);
}

/** The prompt for the new agent; see the bounds above. */
export function buildHandover(input: HandoverInput, t: Translate): string {
  const head = t("handover.intro", { from: oneLine(input.from), folder: oneLine(input.folder) });
  const instruction = t("handover.instruction");
  const prompt =
    input.lastPrompt === null ? "" : cut(clean(input.lastPrompt).trim(), PROMPT_MAX_CHARS);
  const promptPart =
    prompt === ""
      ? null
      : `${t("handover.lastPrompt")}\n${prompt
          .split("\n")
          .map((line) => `> ${line}`)
          .join("\n")}`;
  const files = input.files.map(oneLine).filter((path) => path !== "");
  const notes = input.notes
    .slice(0, NOTES_MAX)
    .map((note) => ({
      title: oneLine(note.title),
      excerpt: cut(oneLine(note.excerpt), NOTE_EXCERPT_MAX),
    }))
    .filter((note) => note.title !== "");

  const filesPart = (limit: number) => {
    if (files.length === 0) return null;
    const shown = files.slice(0, limit).map((path) => `- ${path}`);
    const rest = files.length - Math.min(limit, files.length);
    if (rest > 0) shown.push(`- ${t("handover.moreFiles", { count: rest })}`);
    return `${t("handover.files", { count: files.length })}\n${shown.join("\n")}`;
  };
  const notesPart = (excerpts: boolean) =>
    notes.length === 0
      ? null
      : `${t("handover.notes")}\n${notes
          .map((note) =>
            excerpts && note.excerpt !== ""
              ? `- ${note.title}: ${note.excerpt}`
              : `- ${note.title}`,
          )
          .join("\n")}`;
  const join = (parts: (string | null)[]) =>
    parts.filter((part): part is string => part !== null).join("\n\n");

  // Shrinks step by step until it fits: note excerpts first, then the file list.
  let limit = Math.min(FILES_MAX, files.length);
  let excerpts = true;
  for (;;) {
    const text = join([head, promptPart, filesPart(limit), notesPart(excerpts), instruction]);
    if (text.length <= HANDOVER_MAX_CHARS) return text;
    if (excerpts) excerpts = false;
    else if (limit > 0) limit = Math.max(0, limit - 10);
    else return cut(text, HANDOVER_MAX_CHARS);
  }
}

/** The request for a summary that goes into the old session. */
export function summaryRequest(t: Translate): string {
  return t("handover.summaryRequest");
}

/**
 * Installed agents that can take over: not shells (a prompt would run as a command there) and
 * not the session's own program.
 */
export function handoverTargets(profiles: readonly Profile[], currentProfileId: string): Profile[] {
  return profiles.filter(
    (profile) =>
      profile.installed &&
      profile.id !== currentProfileId &&
      shellKind(profile.command) === "agent",
  );
}

/** Whether a session can be handed over: an agent session, not a program update. */
export function canHandOver(
  session: { kind?: string | undefined },
  command: string | undefined,
): boolean {
  return session.kind !== "update" && command !== undefined && shellKind(command) === "agent";
}

/**
 * Where and how the new session starts: always the old session's folder, so uncommitted changes
 * are there. A team session stays a team session in the same worktree where the new program can
 * be one, else it runs private there. The worktree goes along either way, so closing one of the
 * two sessions never removes it under the other.
 */
export function handoverStart(
  session: { cwd: string; mode?: SessionMode | undefined; worktree?: Worktree | undefined },
  profile: Profile,
): { folder: string; mode: SessionMode; worktree?: Worktree } {
  const worktree = session.worktree;
  if (worktree === undefined) return { folder: session.cwd, mode: "private" };
  const mode = session.mode === "team" && teamCapable(profile) ? "team" : "private";
  return { folder: worktree.path, mode, worktree };
}

/** A summary can be asked for while the old agent runs and is not busy. */
export function canRequestSummary(
  session: { status: string },
  attention: AttentionState | undefined,
): boolean {
  return session.status !== "exited" && attention !== "working";
}

export interface ReadyProbe {
  /** The program turned on bracketed paste: its input line is up. */
  bracketedPaste: () => boolean;
  /** Milliseconds since the last output, `null` before any. */
  quietFor: () => number | null;
  /** The session is gone; nothing is pasted. */
  gone: () => boolean;
}

/**
 * Waits until the new agent is ready for input: bracketed paste on and output paused for
 * READY_QUIET_MS. Resolves `true` then or after the timeout (the paste then asks first if it
 * would run lines), `false` when the session went away.
 */
export async function waitUntilReady(
  probe: ReadyProbe,
  wait: (ms: number) => Promise<void>,
  timeoutMs = READY_TIMEOUT_MS,
): Promise<boolean> {
  for (let waited = 0; waited < timeoutMs; waited += READY_POLL_MS) {
    if (probe.gone()) return false;
    const quiet = probe.quietFor();
    if (probe.bracketedPaste() && quiet !== null && quiet >= READY_QUIET_MS) return true;
    await wait(READY_POLL_MS);
  }
  return !probe.gone();
}
