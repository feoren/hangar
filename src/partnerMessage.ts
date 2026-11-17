// Cross-project messages (ADR 0012), the window's part of the delivery: where a message goes
// (exactly one running agent session of the partner project, a choice among several, or a new
// session with the last program used there) and when (the target paused, then ready for a
// paste like the handover, `waitUntilReady`). What may be sent and the pasted text itself come
// from the backend (partners.rs: partnership, target, control characters, length). Pure.

import { HANDOVER_MAX_CHARS } from "./handover";
import { shellKind } from "./pathQuote";

/** A message is capped like the handover prompt (backend: `MAX_MESSAGE_CHARS`). */
export const MESSAGE_MAX_CHARS = HANDOVER_MAX_CHARS;
/** How often a held message looks whether its target paused. */
export const HOLD_POLL_MS = 500;

/** A session of the partner project as the backend lists it (`PartnerSession`). */
export interface PartnerSession {
  id: number;
  profileId: string;
  running: boolean;
}

/** The backend's `PartnerTargets`. */
export interface PartnerTargets {
  /** The sending project's short name. */
  sender: string;
  partner: { name: string; folder: string };
  /** Oldest first. */
  sessions: PartnerSession[];
}

function partnerSession(value: unknown): PartnerSession | null {
  if (typeof value !== "object" || value === null) return null;
  const id: unknown = Reflect.get(value, "id");
  const profileId: unknown = Reflect.get(value, "profileId");
  const running: unknown = Reflect.get(value, "running");
  if (typeof id !== "number" || !Number.isSafeInteger(id) || id < 0) return null;
  if (typeof profileId !== "string" || typeof running !== "boolean") return null;
  return { id, profileId, running };
}

/** The backend's answer, `null` when malformed. */
export function parsePartnerTargets(value: unknown): PartnerTargets | null {
  if (typeof value !== "object" || value === null) return null;
  const sender: unknown = Reflect.get(value, "sender");
  const partner: unknown = Reflect.get(value, "partner");
  const sessions: unknown = Reflect.get(value, "sessions");
  if (typeof sender !== "string" || typeof partner !== "object" || partner === null) return null;
  const name: unknown = Reflect.get(partner, "name");
  const folder: unknown = Reflect.get(partner, "folder");
  if (typeof name !== "string" || typeof folder !== "string" || !Array.isArray(sessions)) {
    return null;
  }
  const parsed = sessions.map(partnerSession);
  if (parsed.some((session) => session === null)) return null;
  return {
    sender,
    partner: { name, folder },
    sessions: parsed.filter((session): session is PartnerSession => session !== null),
  };
}

/**
 * Where a message goes: `one` running agent session; `choose` among several (the person
 * picks); `start` a new session with `profileId` in the partner's folder, `null` when no
 * program can start there.
 */
export type Delivery =
  | { kind: "one"; id: number }
  | { kind: "choose"; ids: number[] }
  | { kind: "start"; profileId: string | null };

/**
 * Only agents receive a message: in a shell the text would run as a command. Without a running
 * one, the program that worked there last starts (the newest session, ended ones too), else the
 * one Hangar would start next anyway (`fallbackProfileId`). Programs that cannot start are
 * skipped (`startable`).
 */
export function deliveryPlan(
  sessions: readonly PartnerSession[],
  commandOf: (profileId: string) => string | undefined,
  startable: ReadonlySet<string>,
  fallbackProfileId: string | null,
): Delivery {
  const agent = (profileId: string) => {
    const command = commandOf(profileId);
    return command !== undefined && shellKind(command) === "agent";
  };
  const running = sessions.filter((session) => session.running && agent(session.profileId));
  const [first] = running;
  if (first !== undefined && running.length === 1) return { kind: "one", id: first.id };
  if (running.length > 1) return { kind: "choose", ids: running.map((session) => session.id) };
  const last = [...sessions]
    .reverse()
    .find((session) => agent(session.profileId) && startable.has(session.profileId));
  if (last !== undefined) return { kind: "start", profileId: last.profileId };
  const fallback =
    fallbackProfileId !== null && agent(fallbackProfileId) && startable.has(fallbackProfileId)
      ? fallbackProfileId
      : null;
  return { kind: "start", profileId: fallback };
}

/** Text the dialog can send: not blank, not longer than MESSAGE_MAX_CHARS. */
export function canSendMessage(text: string): boolean {
  return text.trim() !== "" && Array.from(text).length <= MESSAGE_MAX_CHARS;
}

export interface HoldProbe {
  /** The target's program is working on a task. */
  working: () => boolean;
  /** The target session is gone; the message is dropped. */
  gone: () => boolean;
}

/**
 * Holds a message while its target works and resolves `paused` once it stopped, or `gone`
 * when the session went away. There is no time limit: the message waits as long as the task
 * runs, like a queued message.
 */
export async function holdUntilPaused(
  probe: HoldProbe,
  wait: (ms: number) => Promise<void>,
): Promise<"paused" | "gone"> {
  for (;;) {
    if (probe.gone()) return "gone";
    if (!probe.working()) return "paused";
    await wait(HOLD_POLL_MS);
  }
}

/** How a delivery ended; everything but `pasted` leaves the text undelivered. */
export type DeliveryResult =
  | { kind: "pasted" }
  | { kind: "gone" }
  | { kind: "notReady" }
  | { kind: "refused"; error: unknown }
  | { kind: "pasteFailed" };

/** What a delivery needs from the window and the backend (faked in tests). */
export interface DeliveryDeps extends HoldProbe {
  /** The held message is shown to the person once, when the target is working. */
  onHeld: () => void;
  /** Waits until the target can take a paste (`waitUntilReady` of the handover). */
  ready: () => Promise<"ready" | "notReady" | "gone">;
  /**
   * Bracketed paste is on right now. Checked again just before the paste: the backend's round
   * trip lies between `ready` and the paste, and without it every line break would submit a line.
   */
  bracketedPaste: () => boolean;
  /** The backend checks, cleans, heads and logs the text; rejects when it refuses. */
  prepare: () => Promise<string>;
  /** Pastes as one bracketed paste; rejects when the program did not take it. */
  paste: (text: string) => Promise<void>;
  /** Presses Enter. */
  submit: () => void;
  wait: (ms: number) => Promise<void>;
}

/**
 * The delivery to a running target, in order: hold while it works, wait until it can take a
 * paste, have the backend prepare the text, paste it, and press Enter only when `submit`. Every
 * step re-checks that the target still exists. Shared by the manual send and, later, by messages
 * an agent sent (ADR 0012 part 2).
 */
export async function deliverMessage(
  deps: DeliveryDeps,
  submit: boolean,
  enterDelayMs: number,
): Promise<DeliveryResult> {
  if (deps.working()) deps.onHeld();
  if ((await holdUntilPaused(deps, deps.wait)) === "gone") return { kind: "gone" };
  const ready = await deps.ready();
  if (ready !== "ready") return { kind: ready };
  let text: string;
  try {
    text = await deps.prepare();
  } catch (error) {
    return { kind: "refused", error };
  }
  if (deps.gone()) return { kind: "gone" };
  if (!deps.bracketedPaste()) return { kind: "notReady" };
  try {
    await deps.paste(text);
  } catch {
    return { kind: "pasteFailed" };
  }
  if (submit) {
    await deps.wait(enterDelayMs);
    if (!deps.gone()) deps.submit();
  }
  return { kind: "pasted" };
}
