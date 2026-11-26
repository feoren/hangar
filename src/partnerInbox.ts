// Waiting messages (ADR 0012, decision point 4): cross-project messages an agent sent with
// `hangar-send`. None is pasted on its own; each waits at its partner project, and every running
// agent session row there shows it with "Paste", "Paste and send" and "Discard", so with several
// sessions the person picks the one. The backend keeps, checks and heads them
// (partner_service.rs); this file reads its answer and finds the rows. Pure.

import type { IntroSummary } from "./linkIntro";
import { projectOfSession, validName, type PartnerBook } from "./partners";
import { shellKind } from "./pathQuote";
import { secretKinds, type SecretKind } from "./secretScan";

/** The backend's `WaitingMessage`. */
export interface WaitingMessage {
  id: number;
  /** The sending project's short name. */
  from: string;
  /** The partner project's short name it waits at. */
  to: string;
  /** Cleaned by the backend, without the header line. */
  text: string;
  /** Unix time in milliseconds when it arrived. */
  at: number;
  /** Kinds of secret the backend found in the text (ADR 0015); left out when none. */
  secrets?: readonly SecretKind[];
  /**
   * The card's heading when it is not "Message from <from>": a link group member's message
   * (ADR 0014) shown with the same card. Set by the window, never by the backend.
   */
  title?: string;
  /**
   * Hangar's introduction of a link group, summed up for people; the card shows this instead of
   * the agent's text. Set by the window, never by the backend.
   */
  intro?: IntroSummary;
  /**
   * Why a message that goes on its own waits (a link message held back by the person's draft in
   * its target). Set by the window, never by the backend.
   */
  note?: string;
}

function waitingMessage(value: unknown): WaitingMessage | null {
  if (typeof value !== "object" || value === null) return null;
  const id: unknown = Reflect.get(value, "id");
  const from: unknown = Reflect.get(value, "from");
  const to: unknown = Reflect.get(value, "to");
  const text: unknown = Reflect.get(value, "text");
  const at: unknown = Reflect.get(value, "at");
  if (typeof id !== "number" || !Number.isSafeInteger(id) || id <= 0) return null;
  if (typeof from !== "string" || typeof to !== "string" || typeof text !== "string") return null;
  if (!validName(from) || !validName(to) || text.trim() === "") return null;
  if (typeof at !== "number" || !Number.isFinite(at)) return null;
  const secrets = secretKinds(Reflect.get(value, "secrets"));
  return secrets.length === 0 ? { id, from, to, text, at } : { id, from, to, text, at, secrets };
}

/** The backend's answer; a malformed entry is left out, anything else malformed reads as none. */
export function parseWaitingMessages(value: unknown): WaitingMessage[] {
  if (!Array.isArray(value)) return [];
  return value.map(waitingMessage).filter((message): message is WaitingMessage => message !== null);
}

/** What decides whether a session row shows waiting messages. */
export interface InboxSession {
  id: number;
  profileId: string;
  cwd: string;
  status: string;
  kind?: string | undefined;
  worktree?: { repoRoot: string } | undefined;
}

/**
 * The waiting messages each row shows: those of the session's project, on every running agent
 * session there (not a shell, where the text would run as commands, and not an update). The
 * backend checks the target again before it hands out the text.
 */
export function inboxBySession(
  messages: readonly WaitingMessage[],
  book: PartnerBook,
  sessions: readonly InboxSession[],
  commandOf: (profileId: string) => string | undefined,
): Map<number, WaitingMessage[]> {
  const rows = new Map<number, WaitingMessage[]>();
  if (messages.length === 0) return rows;
  for (const session of sessions) {
    if (session.status !== "running" || session.kind === "update") continue;
    const command = commandOf(session.profileId);
    if (command === undefined || shellKind(command) !== "agent") continue;
    const project = projectOfSession(book, session);
    if (project === undefined) continue;
    const own = messages.filter((message) => message.to === project.name);
    if (own.length > 0) rows.set(session.id, own);
  }
  return rows;
}

/** The messages no row shows yet: their project has no running agent session. */
export function unshownMessages(
  messages: readonly WaitingMessage[],
  rows: ReadonlyMap<number, readonly WaitingMessage[]>,
): WaitingMessage[] {
  const shown = new Set<number>();
  for (const list of rows.values()) for (const message of list) shown.add(message.id);
  return messages.filter((message) => !shown.has(message.id));
}

/** The message's lines, for showing line by line (the invisible characters marked per line). */
export function messageLines(text: string): string[] {
  return text.split("\n");
}
