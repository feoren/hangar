// Hangar's introduction of a link group (ADR 0014) as the person sees it. The text that goes to
// the agent is English and written for an agent (links.rs `introduction`); its card shows people
// a short summary instead: that the session now works in a group, and with whom. The agent's text
// stays exactly as it is and can still be shown. Pure.

import type { LinkRole, LinkState } from "./links";
import { isProgramTitle, projectFolder } from "./sessionLabel";

/** Opening of the introduction sent after the group's lead changed (links.rs). */
const ROLES_CHANGED = "The roles in your link group";

/**
 * Which introduction it is: the first one (also a newcomer's), or the one after the roles
 * changed. Members hear nothing when others join (ADR 0014, "Calmer groups").
 */
export type IntroKind = "linked" | "rolesChanged";

/** One other member as the card shows it. */
export interface IntroMember {
  session: number;
  /** What the chip says: the session's own name, or its project folder for a program title. */
  label: string;
  /** The tooltip: name, program and folder. */
  detail: string;
  profileId: string;
  lead: boolean;
}

export interface IntroSummary {
  kind: IntroKind;
  /** The session the introduction goes to leads the group. */
  leads: boolean;
  /** The other members, in the group's order. */
  members: IntroMember[];
}

/** A session as the summary needs it. */
export interface IntroSession {
  id: number;
  title: string;
  cwd: string;
  profileId: string;
}

/** The kind of introduction `text` is. */
export function introKind(text: string): IntroKind {
  return text.trimStart().startsWith(ROLES_CHANGED) ? "rolesChanged" : "linked";
}

/**
 * The card's summary of Hangar's introduction `message` (a link message without a sender) to
 * session `to` in group `group`; `null` when the group is gone. Members whose session is no
 * longer known show their member name.
 */
export function introSummary(
  state: LinkState,
  message: { group: number; to: number; text: string },
  sessions: readonly IntroSession[],
  programName: (profileId: string) => string | undefined,
): IntroSummary | null {
  const group = state.groups.find((entry) => entry.id === message.group);
  if (group === undefined) return null;
  const own = group.members.find((member) => member.session === message.to);
  const found = group.members
    .filter((member) => member.session !== message.to)
    .map((member) => {
      const session = sessions.find((entry) => entry.id === member.session);
      return introMember(member.session, member.name, member.role, session, programName);
    });
  // Two members a folder would name alike ("hangar" twice) show their own titles instead.
  const twice = (label: string) => found.filter((entry) => entry.member.label === label).length > 1;
  const members = found.map((entry) =>
    entry.titled !== null && twice(entry.member.label) ? entry.titled : entry.member,
  );
  return { kind: introKind(message.text), leads: own?.role === "lead", members };
}

/** A member as its chip shows it, and as its title would show it where a folder is not enough. */
function introMember(
  id: number,
  name: string,
  role: LinkRole,
  session: IntroSession | undefined,
  programName: (profileId: string) => string | undefined,
): { member: IntroMember; titled: IntroMember | null } {
  const lead = role === "lead";
  if (session === undefined) {
    return {
      member: { session: id, label: name, detail: name, profileId: "", lead },
      titled: null,
    };
  }
  const program = programName(session.profileId) ?? session.profileId;
  const folder = projectFolder(session.cwd);
  const title = session.title.trim();
  const chip = (label: string): IntroMember => ({
    session: id,
    label,
    detail: [...new Set([label, program, folder].filter((part) => part !== ""))].join(" · "),
    profileId: session.profileId,
    lead,
  });
  // "Claude Code (2)" says less than the folder it works in.
  const byFolder =
    folder !== "" && title !== "" && isProgramTitle(title, [program, session.profileId]);
  return byFolder
    ? { member: chip(folder), titled: chip(title) }
    : { member: chip(title || program), titled: null };
}
