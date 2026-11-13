// What the sidebar footer and the Team window show at a glance: initials for avatars, the
// teammates online, the team pill's state and which tabs the Team window offers. Pure functions;
// the components only render their results.

import type { Me } from "./account";
import type { Translate } from "./i18n";
import { revealInvisible } from "./invisible";
import { isSharing, statusTone, type TeamActivity, type TeamStatus } from "./team";

/**
 * Up to two initials of a name ("Anna Berg" gives "AB", "anna" gives "A"); empty when the name
 * holds no letter or digit, so the avatar shows a person icon instead.
 */
export function initials(name: string): string {
  const words = name
    .trim()
    .split(/[\s._@-]+/u)
    .map((word) => /[\p{L}\p{N}]/u.exec(word)?.[0] ?? "")
    .filter((letter) => letter !== "");
  const first = words[0] ?? "";
  const last = words.length > 1 ? (words[words.length - 1] ?? "") : "";
  return (first + last).toLocaleUpperCase();
}

/** Initials of an e-mail address: only the part before the "@" counts. */
export function emailInitials(email: string): string {
  return initials(email.split("@")[0] ?? "");
}

/** A teammate who is connected and not away, once per person. */
export interface OnlinePerson {
  member: string;
  /** As the teammate's device sent it, invisible characters revealed; may be empty. */
  name: string;
}

/**
 * The teammates online in the connected projects: other people (not this person's other
 * devices) whose presence is not "away", each person once, in the order they appear.
 */
export function onlinePeople(
  statuses: ReadonlyMap<string, TeamStatus>,
  activities: ReadonlyMap<string, TeamActivity>,
): OnlinePerson[] {
  const people: OnlinePerson[] = [];
  const seen = new Set<string>();
  for (const [projectId, activity] of activities) {
    if (statuses.get(projectId)?.state !== "connected") continue;
    for (const peer of activity.peers) {
      if (peer.own || peer.presence === "away" || seen.has(peer.member)) continue;
      seen.add(peer.member);
      people.push({ member: peer.member, name: revealInvisible(peer.name.trim()) });
    }
  }
  return people;
}

/**
 * What the team pill says. `requests`: someone waits for an answer from this device;
 * `waiting`: this device waits for another one to confirm it; `problem`: a connection needs the
 * person; `connected`, `connecting` and `off` as their names say.
 */
export type PillKind = "requests" | "waiting" | "problem" | "connected" | "connecting" | "off";

/** The pill's color family; the text always says the same, never the color alone. */
export type PillTone = "ok" | "attention" | "quiet";

export interface TeamPill {
  kind: PillKind;
  tone: PillTone;
  online: OnlinePerson[];
  /** Device confirmations and release requests waiting for this device. */
  requests: number;
}

const PILL_TONES: Record<PillKind, PillTone> = {
  requests: "attention",
  waiting: "attention",
  problem: "attention",
  connected: "ok",
  connecting: "quiet",
  off: "quiet",
};

/**
 * The pill across all projects: open requests come first, then a device waiting for its
 * confirmation, then a connection that needs the person, then any connected project.
 */
export function teamPill(
  statuses: ReadonlyMap<string, TeamStatus>,
  activities: ReadonlyMap<string, TeamActivity>,
  requests: number,
): TeamPill {
  const shared = [...statuses.values()].filter(isSharing);
  let kind: PillKind = "off";
  if (requests > 0) kind = "requests";
  else if (shared.some((status) => status.state === "waitingForConfirmation")) kind = "waiting";
  else if (shared.some((status) => statusTone(status) === "problem")) kind = "problem";
  else if (shared.some((status) => status.state === "connected")) kind = "connected";
  else if (shared.some((status) => status.state === "connecting")) kind = "connecting";
  const online = kind === "off" ? [] : onlinePeople(statuses, activities);
  return { kind, tone: PILL_TONES[kind], online, requests };
}

/** The pill's state in words. */
function stateText(pill: TeamPill, t: Translate): string {
  switch (pill.kind) {
    case "requests":
      return t("teamPill.requests", { count: pill.requests });
    case "waiting":
      return t("team.status.waiting");
    case "problem":
      return t("team.status.attention");
    case "connected":
      return pill.online.length > 0
        ? t("teamPill.online", { count: pill.online.length })
        : t("team.status.connected");
    case "connecting":
      return t("team.status.connecting");
    case "off":
      return t("teamPill.off");
  }
}

/**
 * The pill's accessible name and tooltip: what it opens and the state in full, with the names of
 * who is online. The pill itself shows only its mark and how many people are online.
 */
export function pillLabel(
  pill: TeamPill,
  t: Translate,
  nameOf: (person: OnlinePerson) => string,
): string {
  const names = pill.online.map(nameOf).join(", ");
  const text = stateText(pill, t);
  const state = names === "" ? text : `${text}: ${names}`;
  return t("teamPill.label", { state });
}

/** Tabs of the Team window, in their order. */
export type TeamTab = "now" | "people" | "requests";

/** "Requests" only while something waits for an answer. */
export function teamTabs(requests: number): TeamTab[] {
  return requests > 0 ? ["now", "people", "requests"] : ["now", "people"];
}

/** The tab the window opens on: the requests while there are any, otherwise "Now". */
export function initialTab(requests: number): TeamTab {
  return requests > 0 ? "requests" : "now";
}

/** The tab to show: the chosen one while it exists, otherwise "Now". */
export function shownTab(chosen: TeamTab, requests: number): TeamTab {
  return teamTabs(requests).includes(chosen) ? chosen : "now";
}

/**
 * The project the window shows: the one chosen in this window while it still exists, else one
 * whose device waits for its confirmation (the pill sent the person here for its safety number),
 * else the one shown last, else the first shared one, else the first one; `null` without
 * projects.
 */
export function shownProject(
  projectIds: readonly string[],
  chosen: string | null,
  statuses: ReadonlyMap<string, TeamStatus>,
  remembered: string | null = null,
): string | null {
  if (chosen !== null && projectIds.includes(chosen)) return chosen;
  const waiting = projectIds.find((id) => statuses.get(id)?.state === "waitingForConfirmation");
  if (waiting !== undefined) return waiting;
  if (remembered !== null && projectIds.includes(remembered)) return remembered;
  const shared = projectIds.find((id) => {
    const status = statuses.get(id);
    return status !== undefined && isSharing(status);
  });
  return shared ?? projectIds[0] ?? null;
}

/** Whether at least one of the projects is shared; without that, the window offers to connect. */
export function anyShared(
  projectIds: readonly string[],
  statuses: ReadonlyMap<string, TeamStatus>,
): boolean {
  return projectIds.some((id) => {
    const status = statuses.get(id);
    return status !== undefined && isSharing(status);
  });
}

/** Initials of the signed-in person: from the name, else from the e-mail address. */
export function meInitials(user: Me | null): string {
  if (user === null) return "";
  const name = user.name?.trim() ?? "";
  return name === "" ? emailInitials(user.email) : initials(name);
}
