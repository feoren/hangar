// Link groups (ADR 0014), the window's part: reads the backend's state, decides which sessions
// can be linked (running agent sessions that got the send client, in no group yet), how each
// member row is marked, which link messages go on their own and which wait for a click below a
// row, checks member names in the link dialog, reads a group's history and turns the members of
// a running group into what a workspace template keeps. Pure; the backend keeps the groups and
// checks everything again (links.rs, link_service.rs).

import { secretKinds, type SecretKind } from "./secretScan";
import type { AttentionReason, AttentionState } from "./attention";
import { READY_QUIET_MS } from "./handover";

/** Colors of the fixed palette (`--link-0` to `--link-5` in styles.css; backend `PALETTE_SIZE`). */
export const LINK_COLORS = 6;
/** Members of one group (backend `MAX_MEMBERS`). */
export const MAX_LINK_MEMBERS = 8;
/** The loop brake's window and the quiet time that releases it (backend `BRAKE_WINDOW_MS`). */
export const BRAKE_WINDOW_MS = 60_000;
/** Names every other member in `hangar-send all`; no member carries it (backend `ALL`). */
export const ALL_MEMBERS = "all";
/** Names the group's lead in `hangar-send lead`; no member carries it (backend `LEAD`). */
export const LEAD_ALIAS = "lead";

/** A member's part in its group: at most one leads (backend `Role`). */
export type LinkRole = "lead" | "member";

export interface LinkMember {
  session: number;
  name: string;
  role: LinkRole;
}

/** The backend's `GroupView`. */
export interface LinkGroup {
  id: number;
  /** Index into the palette, below LINK_COLORS. */
  color: number;
  askFirst: boolean;
  /** The loop brake holds messages right now. */
  braked: boolean;
  members: LinkMember[];
}

/** The backend's `LinkMessageView`. */
export interface LinkMessage {
  id: number;
  group: number;
  /** The sender's member name; `null` for Hangar's introduction. */
  fromName: string | null;
  to: number;
  /** Cleaned by the backend, without the header line. */
  text: string;
  at: number;
  /** Pasted with Enter once the target waits for input; else it waits for a click. */
  auto: boolean;
  /** A paste of it runs right now. */
  claimed: boolean;
  /** Kinds of secret the backend found (ADR 0015): it never goes on its own; left out when none. */
  secrets?: readonly SecretKind[];
}

/** The backend's `LinkSnapshot`. */
export interface LinkState {
  groups: LinkGroup[];
  /** Sessions whose current process got the send client. */
  sendable: ReadonlySet<number>;
  /** Of those, the ones whose agent runs without asking before tools. */
  unattended: ReadonlySet<number>;
  messages: LinkMessage[];
}

export const EMPTY_LINKS: LinkState = {
  groups: [],
  sendable: new Set(),
  unattended: new Set(),
  messages: [],
};

const NAME = /^[a-z0-9-]{1,32}$/;

/** A member name as the backend takes it: `[a-z0-9-]{1,32}`, neither "all" nor "lead". */
export function validMemberName(name: string): boolean {
  return NAME.test(name) && name !== ALL_MEMBERS && name !== LEAD_ALIAS;
}

/** A role as the backend sends it; anything else reads as a plain member. */
export function parseRole(value: unknown): LinkRole {
  return value === "lead" ? "lead" : "member";
}

function id(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function member(value: unknown): LinkMember | null {
  if (typeof value !== "object" || value === null) return null;
  const session = id(Reflect.get(value, "session"));
  const name: unknown = Reflect.get(value, "name");
  if (session === null || typeof name !== "string" || !validMemberName(name)) return null;
  return { session, name, role: parseRole(Reflect.get(value, "role")) };
}

function group(value: unknown): LinkGroup | null {
  if (typeof value !== "object" || value === null) return null;
  const groupId = id(Reflect.get(value, "id"));
  const color = id(Reflect.get(value, "color"));
  const askFirst: unknown = Reflect.get(value, "askFirst");
  const braked: unknown = Reflect.get(value, "braked");
  const members: unknown = Reflect.get(value, "members");
  if (groupId === null || color === null || color >= LINK_COLORS) return null;
  if (typeof askFirst !== "boolean" || typeof braked !== "boolean") return null;
  if (!Array.isArray(members)) return null;
  const parsed = members.map(member);
  if (parsed.length < 2 || parsed.some((entry) => entry === null)) return null;
  const kept = parsed.filter((entry): entry is LinkMember => entry !== null);
  // At most one lead: a second one reads as a member.
  const lead = kept.findIndex((entry) => entry.role === "lead");
  return {
    id: groupId,
    color,
    askFirst,
    braked,
    members: kept.map((entry, index) =>
      entry.role === "lead" && index !== lead ? { ...entry, role: "member" } : entry,
    ),
  };
}

function message(value: unknown): LinkMessage | null {
  if (typeof value !== "object" || value === null) return null;
  const messageId = id(Reflect.get(value, "id"));
  const groupId = id(Reflect.get(value, "group"));
  const fromName: unknown = Reflect.get(value, "fromName");
  const to = id(Reflect.get(value, "to"));
  const text: unknown = Reflect.get(value, "text");
  const at: unknown = Reflect.get(value, "at");
  const auto: unknown = Reflect.get(value, "auto");
  const claimed: unknown = Reflect.get(value, "claimed");
  if (messageId === null || messageId === 0 || groupId === null || to === null) return null;
  if (fromName !== null && (typeof fromName !== "string" || !validMemberName(fromName))) {
    return null;
  }
  if (typeof text !== "string" || text.trim() === "") return null;
  if (typeof at !== "number" || !Number.isFinite(at) || typeof auto !== "boolean") return null;
  if (typeof claimed !== "boolean") return null;
  const found = { id: messageId, group: groupId, fromName, to, text, at, auto, claimed };
  const secrets = secretKinds(Reflect.get(value, "secrets"));
  // A message with a secret waits for a click, whatever `auto` says.
  return secrets.length === 0 ? found : { ...found, auto: false, secrets };
}

/** The backend's answer; malformed entries are left out, anything else malformed reads as none. */
export function parseLinkState(value: unknown): LinkState {
  if (typeof value !== "object" || value === null) return EMPTY_LINKS;
  const groups: unknown = Reflect.get(value, "groups");
  const sendable: unknown = Reflect.get(value, "sendable");
  const unattended: unknown = Reflect.get(value, "unattended");
  const messages: unknown = Reflect.get(value, "messages");
  const ids = (list: unknown) =>
    new Set(
      Array.isArray(list) ? list.map(id).filter((entry): entry is number => entry !== null) : [],
    );
  return {
    groups: Array.isArray(groups)
      ? groups.map(group).filter((entry): entry is LinkGroup => entry !== null)
      : [],
    sendable: ids(sendable),
    unattended: ids(unattended),
    messages: Array.isArray(messages)
      ? messages.map(message).filter((entry): entry is LinkMessage => entry !== null)
      : [],
  };
}

/** The group session `id` belongs to. */
export function groupOf(state: LinkState, id: number): LinkGroup | undefined {
  return state.groups.find((entry) => entry.members.some((m) => m.session === id));
}

/** The session that leads `group`, `null` when none does. */
export function leadOf(group: LinkGroup): number | null {
  return group.members.find((m) => m.role === "lead")?.session ?? null;
}

/** What decides whether a session can be linked. */
export interface LinkableSession {
  id: number;
  status: string;
  kind?: string | undefined;
}

/**
 * Whether session `session` can be linked now: running, an agent session (no update), its
 * current process got the send client (so no shell and none started before this version), and
 * in no group yet. Only these are offered (only offer what works).
 */
export function canLink(state: LinkState, session: LinkableSession): boolean {
  return (
    session.status === "running" &&
    session.kind !== "update" &&
    state.sendable.has(session.id) &&
    groupOf(state, session.id) === undefined
  );
}

/** The sessions that can be linked, in the given (sidebar) order. */
export function linkCandidates<T extends LinkableSession>(
  state: LinkState,
  sessions: readonly T[],
): T[] {
  return sessions.filter((session) => canLink(state, session));
}

/** What the session menu offers: "Link with …", or editing and unlinking the own group. */
export type LinkOffer = "link" | "member" | null;

/**
 * "Link with …" only where at least one other session could join (only offer what works); a
 * member gets "Edit group" and "Unlink" instead.
 */
export function linkOffer<T extends LinkableSession>(
  state: LinkState,
  session: T,
  sessions: readonly T[],
): LinkOffer {
  if (groupOf(state, session.id) !== undefined) return "member";
  if (!canLink(state, session)) return null;
  return linkCandidates(state, sessions).some((other) => other.id !== session.id) ? "link" : null;
}

/** How a member row is marked: the group's color, the member's name, the brake's hint, and
 * whether it leads the group. */
export interface LinkMark {
  group: number;
  color: number;
  name: string;
  braked: boolean;
  lead: boolean;
}

/** The mark of every member row. */
export function linkMarks(state: LinkState): Map<number, LinkMark> {
  const marks = new Map<number, LinkMark>();
  for (const entry of state.groups) {
    for (const m of entry.members) {
      marks.set(m.session, {
        group: entry.id,
        color: entry.color,
        name: m.name,
        braked: entry.braked,
        lead: m.role === "lead",
      });
    }
  }
  return marks;
}

/** The CSS color of palette entry `color`. */
export function linkColor(color: number): string {
  const index = Number.isInteger(color) && color >= 0 ? color % LINK_COLORS : 0;
  return `var(--link-${String(index)})`;
}

/** The messages that wait for a click, per target row, oldest first. */
export function heldBySession(messages: readonly LinkMessage[]): Map<number, LinkMessage[]> {
  const rows = new Map<number, LinkMessage[]>();
  for (const entry of messages) {
    // A claimed one is being pasted; its card would offer a second paste.
    if (entry.auto || entry.claimed) continue;
    const list = rows.get(entry.to);
    if (list === undefined) rows.set(entry.to, [entry]);
    else list.push(entry);
  }
  return rows;
}

/**
 * The next message to paste on its own for each target, oldest first: one at a time per target,
 * so a second message never lands in the middle of the first. `busy`: targets whose delivery
 * runs right now.
 */
export function nextDeliveries(
  messages: readonly LinkMessage[],
  busy: ReadonlySet<number>,
): LinkMessage[] {
  const next = new Map<number, LinkMessage>();
  for (const entry of messages) {
    if (!entry.auto || entry.claimed || busy.has(entry.to) || next.has(entry.to)) continue;
    next.set(entry.to, entry);
  }
  return [...next.values()];
}

/** Groups whose brake turned on since `before`: each gets one hint. */
export function newlyBraked(
  before: readonly LinkGroup[],
  after: readonly LinkGroup[],
): LinkGroup[] {
  const was = new Set(before.filter((entry) => entry.braked).map((entry) => entry.id));
  return after.filter((entry) => entry.braked && !was.has(entry.id));
}

/** Why the edited names cannot be saved, `null` when they can. */
export type NameProblem = "invalid" | "reserved" | "twice";

/** Checks the names of the link dialog the way the backend will (links.rs `edit`). */
export function nameProblem(names: readonly string[]): NameProblem | null {
  const trimmed = names.map((name) => name.trim());
  if (trimmed.some((name) => name === ALL_MEMBERS || name === LEAD_ALIAS)) return "reserved";
  if (trimmed.some((name) => !NAME.test(name))) return "invalid";
  if (new Set(trimmed).size !== trimmed.length) return "twice";
  return null;
}

/**
 * The sidebar's selection after a Ctrl or Cmd click on `id`: toggled when the session can be
 * linked, else unchanged. `null` tells the caller the click was no selection (a normal click).
 */
export function toggleSelection(
  selected: ReadonlySet<number>,
  id: number,
  linkable: boolean,
): ReadonlySet<number> | null {
  if (!linkable) return null;
  const next = new Set(selected);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

/** The selected sessions that can still be linked, in sidebar order. */
export function selectedForLink<T extends LinkableSession>(
  state: LinkState,
  sessions: readonly T[],
  selected: ReadonlySet<number>,
): T[] {
  return linkCandidates(state, sessions).filter((session) => selected.has(session.id));
}

/** What became of a message in a group's history. */
export type LinkOutcome = "delivered" | "discarded";

/** One message of a group's history (backend `HistoryView`), newest first. */
export interface LinkHistoryEntry {
  /** Unix milliseconds when it was delivered or discarded. */
  at: number;
  /** The sender's member name; `null` for Hangar's introduction. */
  fromName: string | null;
  fromRole: LinkRole;
  toName: string;
  outcome: LinkOutcome;
  /** Cleaned by the backend, without the header line. */
  text: string;
}

function historyEntry(value: unknown): LinkHistoryEntry | null {
  if (typeof value !== "object" || value === null) return null;
  const at: unknown = Reflect.get(value, "at");
  const fromName: unknown = Reflect.get(value, "fromName");
  const toName: unknown = Reflect.get(value, "toName");
  const outcome: unknown = Reflect.get(value, "outcome");
  const text: unknown = Reflect.get(value, "text");
  if (typeof at !== "number" || !Number.isFinite(at)) return null;
  if (fromName !== null && (typeof fromName !== "string" || !validMemberName(fromName))) {
    return null;
  }
  if (typeof toName !== "string" || !validMemberName(toName)) return null;
  if (outcome !== "delivered" && outcome !== "discarded") return null;
  if (typeof text !== "string") return null;
  return {
    at,
    fromName,
    fromRole: parseRole(Reflect.get(value, "fromRole")),
    toName,
    outcome,
    text,
  };
}

/** The backend's `link_history` answer; malformed entries are left out. */
export function parseLinkHistory(value: unknown): LinkHistoryEntry[] {
  if (!Array.isArray(value)) return [];
  return value.map(historyEntry).filter((entry): entry is LinkHistoryEntry => entry !== null);
}

/** Characters of a message's first line the closed history row shows. */
export const HISTORY_PREVIEW_CHARS = 120;

/** The first non-empty line of `text`, cut for the closed history row. */
export function historyPreview(text: string): string {
  const line = text
    .split("\n")
    .map((part) => part.trim())
    .find((part) => part !== "");
  if (line === undefined) return "";
  const chars = Array.from(line);
  return chars.length > HISTORY_PREVIEW_CHARS
    ? `${chars.slice(0, HISTORY_PREVIEW_CHARS - 1).join("")}\u2026`
    : line;
}

/** How a session joins the link group a workspace template forms. */
export interface TemplateMember {
  memberName: string;
  role: LinkRole;
}

/**
 * What a workspace template keeps of the link group among the sessions `ids`: the members of
 * the group with the most of them (the first such group on a tie), with name and role. Empty
 * when no two of them share a group.
 */
export function templateMembers(
  state: LinkState,
  ids: readonly number[],
): Map<number, TemplateMember> {
  let best: LinkGroup | undefined;
  let most = 1;
  for (const entry of state.groups) {
    const count = entry.members.filter((m) => ids.includes(m.session)).length;
    if (count > most) {
      best = entry;
      most = count;
    }
  }
  const members = new Map<number, TemplateMember>();
  for (const m of best?.members ?? []) {
    if (ids.includes(m.session)) members.set(m.session, { memberName: m.name, role: m.role });
  }
  return members;
}

/** The group among `ids` a template would keep (`templateMembers`), for its "Ask before pasting". */
export function templateGroup(state: LinkState, ids: readonly number[]): LinkGroup | undefined {
  const [first] = templateMembers(state, ids).keys();
  return first === undefined ? undefined : groupOf(state, first);
}

/** A session a template started, with how it joins the template's link group. */
export interface StartedMember {
  id: number;
  memberName?: string | undefined;
  role?: LinkRole | undefined;
}

/** What `link_sessions` gets for a template's group. */
export interface TemplateLinkRequest {
  ids: number[];
  names: { session: number; name: string }[];
  lead: number | null;
}

/**
 * How the sessions a template started form its link group: only those whose process got the
 * send client (`sendable`; shells and failed starts are left out), with the names and the lead
 * the template keeps. A name twice or a second lead is dropped, so the backend gets a request it
 * can take. `null` when fewer than two can be linked (the caller says so).
 */
export function templateLinkRequest(
  started: readonly StartedMember[],
  sendable: ReadonlySet<number>,
): TemplateLinkRequest | null {
  const linkable = started.filter((entry) => sendable.has(entry.id)).slice(0, MAX_LINK_MEMBERS);
  if (linkable.length < 2) return null;
  const names: { session: number; name: string }[] = [];
  let lead: number | null = null;
  for (const entry of linkable) {
    const name = entry.memberName;
    if (name !== undefined && validMemberName(name) && !names.some((n) => n.name === name)) {
      names.push({ session: entry.id, name });
    }
    if (entry.role === "lead" && lead === null) lead = entry.id;
  }
  return { ids: linkable.map((entry) => entry.id), names, lead };
}

/** Typing stops a delivery this long after the person's last key, even after their Enter. */
export const TYPING_PAUSE_MS = 4000;
/** How often an automatic delivery looks at its target again. */
export const AUTO_POLL_MS = 250;
/** The introduction waits this long for its target to be ready, then becomes a card. */
export const INTRO_PATIENCE_MS = 5000;
/**
 * A member's message waits this long for a target that is not working and not ready (resting,
 * asking, a bell, a gate decision), then becomes a card. A working target is waited for.
 */
export const MESSAGE_PATIENCE_MS = 30_000;
/** A message waits this long while the person types in its target, then becomes a card. */
export const TYPING_PATIENCE_MS = 20_000;

/** What the window knows about a target session at this moment. */
export interface TargetFacts {
  state: AttentionState;
  /** Why it waits, while the state is "attention". */
  reason: AttentionReason | null;
  /** Why its last working phase ended ("quiet" also while the session is on screen). */
  endedBy: AttentionReason | null;
  /** It may show a question, whose answer an Enter would give (agentCommands.ts). */
  asks: boolean;
  /** The agent waits for the person's decision on a held tool call (gate). */
  gatePending: boolean;
  /**
   * The agent's own hooks report a permission or input dialog (ADR 0016): a sure sign, so the
   * Enter never goes in, whatever the screen shows.
   */
  permissionPending?: boolean;
  /** Milliseconds since the person last typed there, `null` before any key. */
  typedAgoMs: number | null;
  /** The person began a line there that waits for its Enter. */
  draft: boolean;
}

/**
 * Whether Hangar may paste a message with Enter into this target now (ADR 0014): only when the
 * target went quiet after a working phase, which is its prompt waiting for the next task. Never
 * while it works or rests without having worked, rang the bell, waits for a gate decision, shows a
 * dialog its hooks reported or may show a question, and never while the person types there
 * (`typing`). A target on screen turns
 * "idle" instead of "attention" when it goes quiet; its `endedBy` still says "quiet".
 */
export function autoReadiness(facts: TargetFacts): "ready" | "wait" | "typing" {
  if (facts.draft || (facts.typedAgoMs !== null && facts.typedAgoMs < TYPING_PAUSE_MS)) {
    return "typing";
  }
  if (facts.asks || facts.gatePending || facts.permissionPending === true) return "wait";
  const quietAfterWork =
    facts.state === "attention"
      ? facts.reason === "quiet"
      : facts.state === "idle" && facts.endedBy === "quiet";
  return quietAfterWork ? "ready" : "wait";
}

/** How patient an automatic delivery is. */
export interface AutoPatience {
  /** A working target is waited for without a limit (a member's message, not the introduction). */
  waitWhileWorking: boolean;
  /** Not working and not ready this long: it becomes a card. */
  patienceMs: number;
  /** The person types this long: it becomes a card. */
  typingPatienceMs: number;
}

/** What an automatic delivery needs from the window and the backend (faked in tests). */
export interface AutoDeliveryDeps {
  facts: () => TargetFacts;
  gone: () => boolean;
  bracketedPaste: () => boolean;
  /** Milliseconds since the target's last output, `null` before any. */
  quietFor: () => number | null;
  /** The backend checks again and claims the message for this paste; rejects when it waits. */
  take: () => Promise<string>;
  paste: (text: string) => Promise<void>;
  submit: () => void;
  /** The second step after the paste; a failure leaves it claimed, never pasted again. */
  close: () => Promise<void>;
  wait: (ms: number) => Promise<void>;
}

/**
 * How it ended: `pasted` with Enter; `gone` with its target; `refused` by the backend (it waits
 * for a click now); `timeout` or `typing` when the target was not ready in time; `failed` when
 * the target changed between the claim and the paste or the paste did not arrive. Everything
 * but `pasted` and `gone` turns the message into a card.
 */
export type AutoResult = "pasted" | "gone" | "refused" | "timeout" | "typing" | "failed";

/**
 * Delivers one link message on its own: waits until the target is ready (`autoReadiness`, then
 * bracketed paste on and output quiet as for the handover), has the backend claim it, checks the
 * target once more, pastes, presses Enter as a write of its own and closes it.
 */
export async function deliverAutomatically(
  deps: AutoDeliveryDeps,
  patience: AutoPatience,
  enterDelayMs: number,
): Promise<AutoResult> {
  let waited = 0;
  let typing = 0;
  for (;;) {
    if (deps.gone()) return "gone";
    const facts = deps.facts();
    const readiness = autoReadiness(facts);
    const quiet = deps.quietFor();
    if (
      readiness === "ready" &&
      deps.bracketedPaste() &&
      quiet !== null &&
      quiet >= READY_QUIET_MS
    ) {
      break;
    }
    if (readiness === "typing") {
      typing += AUTO_POLL_MS;
      if (typing >= patience.typingPatienceMs) return "typing";
    } else if (!(patience.waitWhileWorking && facts.state === "working")) {
      waited += AUTO_POLL_MS;
      if (waited >= patience.patienceMs) return "timeout";
    }
    await deps.wait(AUTO_POLL_MS);
  }
  let text: string;
  try {
    text = await deps.take();
  } catch {
    return "refused";
  }
  if (deps.gone()) return "gone";
  // The backend's round trip lies between the check and here.
  if (autoReadiness(deps.facts()) !== "ready" || !deps.bracketedPaste()) return "failed";
  try {
    await deps.paste(text);
  } catch {
    return "failed";
  }
  await deps.wait(enterDelayMs);
  if (!deps.gone()) deps.submit();
  await deps.close().catch(() => undefined);
  return "pasted";
}
