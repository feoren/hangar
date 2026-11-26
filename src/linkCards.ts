// What the sidebar sums up about a link group's messages (ADR 0014): which of a session's cards
// stand on their own and which a newer introduction replaces, what the message stack says, the
// messages still on their way per group, sending a stack one message after another, and what the
// group's label offers in its menu. Pure; the backend keeps the messages and checks every step.

import type { SecretKind } from "./secretScan";
import { leadOf, type LinkGroup, type LinkMessage, type LinkState } from "./links";

/** From this many cards on, one session's cards are summed up in a message stack. */
export const STACK_FROM = 2;

/** The cards one session shows, and the introductions a newer one replaces. */
export interface CardStack {
  /** Oldest first; of a group's introductions only the newest. */
  cards: LinkMessage[];
  /** By the id of a shown introduction: the older ones of its group, discarded along with it. */
  superseded: ReadonlyMap<number, readonly number[]>;
}

/**
 * Sums up the cards that wait below one session. Every introduction lists the whole group, the
 * roles and how to write to the others (links.rs `introduction`), so only the newest of a group
 * says anything: the older ones are not shown and go when it is sent or discarded.
 */
export function stackCards(list: readonly LinkMessage[]): CardStack {
  const newest = new Map<number, LinkMessage>();
  for (const entry of list) {
    if (entry.fromName !== null) continue;
    const known = newest.get(entry.group);
    if (
      known === undefined ||
      entry.at > known.at ||
      (entry.at === known.at && entry.id > known.id)
    )
      newest.set(entry.group, entry);
  }
  const superseded = new Map<number, number[]>();
  const cards = list.filter((entry) => {
    if (entry.fromName !== null) return true;
    const kept = newest.get(entry.group);
    if (kept === undefined || kept.id === entry.id) return true;
    const older = superseded.get(kept.id) ?? [];
    older.push(entry.id);
    superseded.set(kept.id, older);
    return false;
  });
  return { cards, superseded };
}

/** Every kind of secret the cards seem to hold, each once, in order of appearance (ADR 0015). */
export function stackSecrets(cards: readonly { secrets?: readonly SecretKind[] }[]): SecretKind[] {
  const kinds: SecretKind[] = [];
  for (const card of cards) {
    for (const kind of card.secrets ?? []) if (!kinds.includes(kind)) kinds.push(kind);
  }
  return kinds;
}

/** One message of a group that goes on its own and has not arrived yet. */
export interface PendingDelivery {
  id: number;
  /** The sender's member name; `null` for Hangar's introduction. */
  fromName: string | null;
  /** The target's member name. */
  toName: string;
  text: string;
}

/**
 * The messages of each group still on their way on their own, oldest first: those the backend
 * lets go without a click, and those whose automatic paste runs in this window right now
 * (`delivering`; a slow paste can outlast the backend's claim). A message that waits for a click
 * is a card, not counted here.
 */
export function pendingByGroup(
  state: LinkState,
  delivering: ReadonlySet<number> = new Set(),
): Map<number, PendingDelivery[]> {
  const pending = new Map<number, PendingDelivery[]>();
  for (const entry of state.messages) {
    if (!entry.auto && !delivering.has(entry.id)) continue;
    const group = state.groups.find((candidate) => candidate.id === entry.group);
    const target = group?.members.find((member) => member.session === entry.to);
    if (group === undefined || target === undefined) continue;
    const list = pending.get(group.id) ?? [];
    list.push({ id: entry.id, fromName: entry.fromName, toName: target.name, text: entry.text });
    pending.set(group.id, list);
  }
  return pending;
}

/**
 * Sends `ids` one after another, each only after the one before it went in: every message is a
 * prompt of its own, so the next waits until the target paused again. Stops at the first that did
 * not go in (cancelled, not ready, refused) and leaves the rest as cards. How many went in.
 */
export async function sendInTurn(
  ids: readonly number[],
  send: (id: number) => Promise<boolean>,
): Promise<number> {
  let sent = 0;
  for (const id of ids) {
    if (!(await send(id))) break;
    sent += 1;
  }
  return sent;
}

/** One entry of the menu behind a group's label in the sidebar. */
export type GroupMenuEntry =
  /** The brake or the budget holds messages: says so and offers to let it go on. */
  | { kind: "paused" }
  /** One choice of the lead; `session: null` is "No lead". */
  | { kind: "lead"; session: number | null; current: boolean }
  | { kind: "edit" }
  | { kind: "history" }
  | { kind: "unlink" };

/**
 * What the group's label offers, top to bottom: the brake's state while it holds, the lead as a
 * choice among the members, then the entries the session menu has for a member (edit, history,
 * unlink). While "Ask before pasting" is on, every message waits anyway; the backend then reports
 * no brake.
 */
export function groupMenuEntries(group: LinkGroup): GroupMenuEntry[] {
  const lead = leadOf(group);
  return [
    ...(group.braked ? [{ kind: "paused" } as const] : []),
    { kind: "lead", session: null, current: lead === null },
    ...group.members.map((member): GroupMenuEntry => ({
      kind: "lead",
      session: member.session,
      current: member.session === lead,
    })),
    { kind: "edit" },
    { kind: "history" },
    { kind: "unlink" },
  ];
}

/** What `edit_link_group` gets for group `group` with lead `lead`; names and toggle stay. */
export interface GroupEdit {
  names: { session: number; name: string }[];
  askFirst: boolean;
  lead: number | null;
}

/**
 * Lets the group go on: an edit that changes nothing, which the backend counts as the person
 * stepping in, so the budget starts again (links.rs `Brake::person_acted`). The rate's brake still
 * needs its quiet minute.
 */
export function resumeEdit(group: LinkGroup): GroupEdit {
  return { names: [], askFirst: group.askFirst, lead: leadOf(group) };
}

/**
 * What the window says after "Allow to go on", read from the group as the backend reports it
 * afterwards: going on, or still too fast for the rate's brake, which only a quiet minute
 * releases. Nothing when the group is gone meanwhile.
 */
export function resumeNotice(
  group: Pick<LinkGroup, "braked"> | undefined,
): "links.resumed" | "links.resumeLater" | null {
  if (group === undefined) return null;
  return group.braked ? "links.resumeLater" : "links.resumed";
}

/**
 * The introductions waiting for the same session in the same group as `delivered`, an
 * introduction that just went in: they are older (by time, then id) and say less, so they go.
 * Nothing for a member's message, and never a member's message. One a click is pasting right
 * now (`claimed`) stays: closing it would pull it from under its own paste.
 */
export function olderIntroductions(
  messages: readonly LinkMessage[],
  delivered: Pick<LinkMessage, "id" | "group" | "to" | "at" | "fromName">,
): number[] {
  if (delivered.fromName !== null) return [];
  return messages
    .filter(
      (entry) =>
        entry.fromName === null &&
        !entry.claimed &&
        entry.id !== delivered.id &&
        entry.group === delivered.group &&
        entry.to === delivered.to &&
        (entry.at < delivered.at || (entry.at === delivered.at && entry.id < delivered.id)),
    )
    .map((entry) => entry.id);
}

/** Makes `lead` the group's lead (`null`: none); `null` when it leads already. */
export function leadEdit(group: LinkGroup, lead: number | null): GroupEdit | null {
  if (leadOf(group) === lead) return null;
  if (lead !== null && !group.members.some((member) => member.session === lead)) return null;
  return { names: [], askFirst: group.askFirst, lead };
}
