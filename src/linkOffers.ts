// The link groups of the last run that can be linked again (ADR 0014, "Remembered groups"): the
// backend remembers only who formed a group and offers it after a restart; nothing is linked
// before the person confirms. Pure: reading the backend's answer and what the hint says.

import { parseRole, validMemberName, type LinkMember } from "./links";

/** The backend's `Offer` (link_memory.rs). */
export interface RelinkOffer {
  /** Names the offer in `applyLinkOffer` and `discardLinkOffer`. */
  id: number;
  /** The members whose restored session runs now, in the remembered order; at least two. */
  members: LinkMember[];
  /** Member names of the remembered members whose session does not run (yet). */
  missing: string[];
  askFirst: boolean;
}

function id(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function offerMember(value: unknown): LinkMember | null {
  if (typeof value !== "object" || value === null) return null;
  const session = id(Reflect.get(value, "session"));
  const name: unknown = Reflect.get(value, "name");
  if (session === null || typeof name !== "string" || !validMemberName(name)) return null;
  return { session, name, role: parseRole(Reflect.get(value, "role")) };
}

function offer(value: unknown): RelinkOffer | null {
  if (typeof value !== "object" || value === null) return null;
  const offerId = id(Reflect.get(value, "id"));
  const members: unknown = Reflect.get(value, "members");
  const missing: unknown = Reflect.get(value, "missing");
  const askFirst: unknown = Reflect.get(value, "askFirst");
  if (offerId === null || !Array.isArray(members) || typeof askFirst !== "boolean") return null;
  const parsed = members.map(offerMember);
  if (parsed.length < 2 || parsed.some((entry) => entry === null)) return null;
  const kept = parsed.filter((entry): entry is LinkMember => entry !== null);
  if (new Set(kept.map((entry) => entry.session)).size !== kept.length) return null;
  return {
    id: offerId,
    members: kept,
    missing: Array.isArray(missing)
      ? missing.filter((name): name is string => typeof name === "string" && validMemberName(name))
      : [],
    askFirst,
  };
}

/** The backend's answer to `link_offers`; malformed offers are left out. */
export function parseLinkOffers(value: unknown): RelinkOffer[] {
  if (!Array.isArray(value)) return [];
  const offers = value.map(offer).filter((entry): entry is RelinkOffer => entry !== null);
  // One offer per id.
  return offers.filter((entry, index) => offers.findIndex((o) => o.id === entry.id) === index);
}

/**
 * The offer the sidebar shows: one at a time, so the hint stays one small bar; the next one
 * shows once this one is decided. `dismissed`: offers the person decided in this window whose
 * answer the backend has not reflected yet.
 */
export function shownOffer(
  offers: readonly RelinkOffer[],
  dismissed: ReadonlySet<number>,
): RelinkOffer | null {
  return offers.find((entry) => !dismissed.has(entry.id)) ?? null;
}

/**
 * The decided offers still worth hiding: those the backend still lists (its answer is on its
 * way). The same set when nothing changes, so React keeps its state.
 */
export function keepOffered(
  decided: ReadonlySet<number>,
  offers: readonly RelinkOffer[],
): ReadonlySet<number> {
  const kept = [...decided].filter((id) => offers.some((entry) => entry.id === id));
  return kept.length === decided.size ? decided : new Set(kept);
}

/**
 * How the hint names the members: each session's own title where the window knows it, else its
 * member name, joined for one line.
 */
export function offerMemberNames(
  offer: RelinkOffer,
  titleOf: (session: number) => string | undefined,
): string[] {
  return offer.members.map((member) => {
    const title = titleOf(member.session)?.trim();
    return title !== undefined && title !== "" ? title : member.name;
  });
}
