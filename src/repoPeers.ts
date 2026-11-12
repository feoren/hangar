// Teammates in the same repository as one of this device's sessions: the file tree marks their
// files (fileTree.ts), a worktree row in the sidebar shows them as small avatars. Pure, tested in
// repoPeers.test.ts and fileTree.test.ts.

import type { PeerView, TeamActivity } from "./team";

/**
 * The teammates working in this session's repository: of every shared project this session is
 * shared with, the teammates' sessions with the same root commit, each device once. A project may
 * link several repositories and the presence names files relative to their repository, so the
 * sessions in the other ones stay out.
 */
export function peersOf(
  activities: ReadonlyMap<string, TeamActivity>,
  sessionId: number,
): PeerView[] {
  const peers = new Map<string, PeerView>();
  for (const activity of activities.values()) {
    const repo = activity.sessionRepos.find((entry) => entry.sessionId === sessionId)?.repo;
    if (repo === undefined) continue;
    for (const peer of activity.peers) {
      const sessions = peer.sessions.filter((session) => session.repo === repo);
      if (sessions.length === 0) continue;
      const device = `${peer.member}\n${peer.device}`;
      const known = peers.get(device);
      if (known === undefined) {
        peers.set(device, { ...peer, sessions });
        continue;
      }
      for (const session of sessions) {
        if (!known.sessions.some((other) => other.key === session.key)) {
          known.sessions.push(session);
        }
      }
    }
  }
  return [...peers.values()];
}

/** A teammate in the session's repository, all of their devices together. */
export interface RepoTeammate {
  member: string;
  /** The name a device sent, else the member id. */
  name: string;
  /** Their programs there, each once, in the order first seen. */
  agents: string[];
}

/**
 * The people beside a session in its repository, each once: other devices of the signed-in
 * person stay out (that is me), as do devices without a session there.
 */
export function repoTeammates(peers: readonly PeerView[]): RepoTeammate[] {
  const people = new Map<string, RepoTeammate>();
  for (const peer of peers) {
    if (peer.own || peer.sessions.length === 0) continue;
    let person = people.get(peer.member);
    if (person === undefined) {
      person = { member: peer.member, name: peer.name.trim(), agents: [] };
      people.set(peer.member, person);
    } else if (person.name === "") {
      person.name = peer.name.trim();
    }
    for (const session of peer.sessions) {
      const agent = session.agent.trim();
      if (agent !== "" && !person.agents.includes(agent)) person.agents.push(agent);
    }
  }
  return [...people.values()].map((person) => ({
    ...person,
    name: person.name === "" ? person.member : person.name,
  }));
}

/** At most `max` people shown, the rest as a count. */
export function teammateStack(
  people: readonly RepoTeammate[],
  max: number,
): { shown: RepoTeammate[]; more: number } {
  return { shown: people.slice(0, max), more: Math.max(0, people.length - max) };
}

/** "Anna (Claude Code), Ben (Codex)": each person with their programs there. */
export function teammateNames(people: readonly RepoTeammate[]): string {
  return people
    .map((person) =>
      person.agents.length === 0 ? person.name : `${person.name} (${person.agents.join(", ")})`,
    )
    .join(", ");
}
