// The window's side of a waiting message's card (WaitingMessages.tsx), shared by partner messages
// (ADR 0012) and link messages (ADR 0014): one send per message at a time, visible while it waits
// for a busy target and cancellable until the paste begins; a discard that hides the card at once;
// and the backend's list read again after every action, so no card outlives its message and none
// hangs without saying why. Pure.

import { splitBackendError } from "./backendError";
import { formatShortTime } from "./format";
import type { MessageKey } from "./i18n";
import type { DeliveryResult } from "./partnerMessage";

/** A send that runs from a card. */
export interface CardSend {
  /** `waiting`: for the target to pause and take a paste (cancellable); `sending`: pasting now. */
  phase: "waiting" | "sending";
  /** Enter follows the paste ("Send"); else the text is only pasted. */
  submit: boolean;
  /** The target session's title, shown next to the status where the card is not its row. */
  target: string;
  /** The target session's id. */
  to?: number;
}

/**
 * The sends that run from cards, by message id: a second click on a message whose send runs is
 * refused, so it is never pasted twice; a cancel counts only while the send still waits. Every
 * change is handed to `changed` as a new map, for React.
 */
export class CardSends {
  private readonly runs = new Map<number, CardSend & { cancelled: boolean }>();

  constructor(private readonly changed: (sends: ReadonlyMap<number, CardSend>) => void) {}

  /** Starts the send of message `id`; `false` when one runs already. */
  start(id: number, submit: boolean, target: string, to?: number): boolean {
    if (this.runs.has(id)) return false;
    this.runs.set(id, {
      phase: "waiting",
      submit,
      target,
      ...(to === undefined ? {} : { to }),
      cancelled: false,
    });
    this.publish();
    return true;
  }

  /** The paste of message `id` begins: no cancel from here on. */
  sending(id: number): void {
    const run = this.runs.get(id);
    if (run === undefined || run.phase === "sending") return;
    run.phase = "sending";
    this.publish();
  }

  /** The person cancels the send of message `id`; `false` when it pastes already or runs not. */
  cancel(id: number): boolean {
    const run = this.runs.get(id);
    if (run?.phase !== "waiting") return false;
    run.cancelled = true;
    return true;
  }

  cancelled(id: number): boolean {
    return this.runs.get(id)?.cancelled === true;
  }

  /** The send of message `id` ended, however. */
  finish(id: number): void {
    if (this.runs.delete(id)) this.publish();
  }

  private publish(): void {
    this.changed(
      new Map(
        [...this.runs].map(([id, run]) => [
          id,
          {
            phase: run.phase,
            submit: run.submit,
            target: run.target,
            ...(run.to === undefined ? {} : { to: run.to }),
          },
        ]),
      ),
    );
  }
}

/** The backend no longer has the message: discarded, pasted elsewhere, or its group ended. */
export function messageGone(error: unknown): boolean {
  if (typeof error !== "string") return false;
  const { code } = splitBackendError(error);
  return code === "link_message_gone" || code === "partner_message_gone";
}

export interface CardSendDeps {
  /** The shared delivery (partnerMessage.ts `deliverMessage`) into the target's terminal. */
  deliver: () => Promise<DeliveryResult>;
  /** Removes the pasted message in the backend. */
  close: () => Promise<void>;
  /** Frees a message claimed for the paste that did not go in (link messages); none for others. */
  release?: () => Promise<void>;
  /** Reads the backend's list again. */
  reload: () => void;
  showError: (error: unknown) => void;
}

/**
 * Delivers a card's message, then removes it when it was pasted or frees it when it was not
 * (a refusal claimed nothing), and reads the list again whatever happened: the card goes with its
 * message even when the backend's event is lost or the removal fails because it went already.
 */
export async function sendFromCard(deps: CardSendDeps): Promise<DeliveryResult> {
  try {
    const result = await deps.deliver();
    if (result.kind === "pasted") {
      await deps.close().catch((error: unknown) => {
        if (!messageGone(error)) deps.showError(error);
      });
    } else if (result.kind !== "refused" && deps.release !== undefined) {
      await deps.release().catch(() => undefined);
    }
    return result;
  } finally {
    deps.reload();
  }
}

/** What the window says after a card's send. */
export type CardNotice =
  | { kind: "none" }
  | { kind: "hint"; key: Extract<MessageKey, "partner.notReady" | "partner.unsent"> }
  | { kind: "error"; error: unknown };

/**
 * Nothing for a message that went in, was cancelled or vanished meanwhile (`vanished`: no longer
 * in the backend's list, for example because its group was unlinked); a hint when the target went
 * away or was not ready; the backend's reason for any other refusal.
 */
export function cardNotice(result: DeliveryResult, vanished: boolean): CardNotice {
  switch (result.kind) {
    case "pasted":
    case "cancelled":
      return { kind: "none" };
    case "gone":
      return vanished ? { kind: "none" } : { kind: "hint", key: "partner.unsent" };
    case "pasteFailed":
      return { kind: "hint", key: "partner.unsent" };
    case "notReady":
      return { kind: "hint", key: "partner.notReady" };
    case "refused":
      return messageGone(result.error) ? { kind: "none" } : { kind: "error", error: result.error };
  }
}

export interface CardDiscardDeps {
  /** Hides the card now. */
  hide: () => void;
  /** Shows it again: the backend kept the message. */
  unhide: () => void;
  close: () => Promise<void>;
  reload: () => void;
  showError: (error: unknown) => void;
}

/**
 * Discards a card's message: the card goes at once, the backend removes the message, and the list
 * is read again. A message that was gone already stays hidden without an error; one the backend
 * kept comes back with the reason.
 */
export async function discardFromCard(deps: CardDiscardDeps): Promise<void> {
  deps.hide();
  try {
    await deps.close();
  } catch (error) {
    if (!messageGone(error)) {
      deps.unhide();
      deps.showError(error);
    }
  } finally {
    deps.reload();
  }
}

/** `hidden` without the ids the backend no longer lists; the same set when none went. */
export function pruneHidden(
  hidden: ReadonlySet<number>,
  listed: Iterable<number>,
): ReadonlySet<number> {
  const present = new Set(listed);
  const kept = [...hidden].filter((id) => present.has(id));
  return kept.length === hidden.size ? hidden : new Set(kept);
}

/**
 * A reader of a list whose reads can overlap: only an answer newer than the last one applied
 * counts, so a slow, older answer never brings back what a newer one dropped.
 */
export function latestOnly<T>(
  read: () => Promise<T>,
  apply: (value: T) => void,
  fail: (error: unknown) => void,
): () => void {
  let asked = 0;
  let applied = 0;
  return () => {
    const ticket = ++asked;
    read()
      .then((value) => {
        if (ticket < applied) return;
        applied = ticket;
        apply(value);
      })
      .catch(fail);
  };
}

/** When a message arrived, for its card: the clock time today, else the short date with it. */
export function cardTime(at: number, now: number, language: string): string {
  const then = new Date(at);
  if (then.toDateString() === new Date(now).toDateString()) {
    return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(then);
  }
  return formatShortTime(then, language);
}

/** Lines a closed card shows. */
export const PREVIEW_LINES = 4;
/** Characters a closed card shows. */
export const PREVIEW_CHARS = 240;

/** What a closed card shows of `text`: at most a few lines and characters, cut with "…". */
export function cardPreview(text: string): { lines: string[]; cut: boolean } {
  const lines: string[] = [];
  let left = PREVIEW_CHARS;
  let cut = false;
  for (const line of text.split("\n")) {
    if (lines.length === PREVIEW_LINES || left <= 0) {
      cut = true;
      break;
    }
    const chars = Array.from(line);
    if (chars.length > left) {
      // One character of room stays for the "…".
      lines.push(chars.slice(0, left - 1).join(""));
      cut = true;
      break;
    }
    lines.push(line);
    left -= chars.length;
  }
  if (cut) {
    const last = lines.length - 1;
    lines[last] = `${(lines[last] ?? "").trimEnd()}…`;
  }
  return { lines, cut };
}
