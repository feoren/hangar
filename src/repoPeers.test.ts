import { describe, expect, it } from "vitest";

import { peersOf, repoTeammates, teammateNames, teammateStack } from "./repoPeers";
import { EMPTY_ACTIVITY, type PeerSession, type PeerView, type TeamActivity } from "./team";

const SHOP = "a".repeat(40);
const DOCS = "d".repeat(40);

function session(key: string, agent: string, repo = SHOP): PeerSession {
  return { key, agent, title: "", state: "working", repo, files: [], moreFiles: 0 };
}

function peer(member: string, sessions: PeerSession[], extra: Partial<PeerView> = {}): PeerView {
  return {
    member,
    device: `${member}-device`,
    name: member.toUpperCase(),
    own: false,
    presence: "active",
    sessions,
    ...extra,
  };
}

describe("teammates in the session's repository", () => {
  it("leaves out my other devices and lists each person once with their programs", () => {
    const peers = [
      peer("ann", [session("a1", "Claude Code")]),
      peer("me", [session("m1", "Codex")], { own: true }),
      peer("ann", [session("a2", "Claude Code"), session("a3", "Aider")], { device: "laptop" }),
      peer("bob", [session("b1", "Codex")], { name: "" }),
    ];
    expect(repoTeammates(peers)).toEqual([
      { member: "ann", name: "ANN", agents: ["Claude Code", "Aider"] },
      { member: "bob", name: "bob", agents: ["Codex"] },
    ]);
  });

  it("takes a name from another device when the first sent none", () => {
    const peers = [
      peer("ann", [session("a1", "Codex")], { name: " " }),
      peer("ann", [session("a2", "Codex")], { device: "laptop", name: "Anna" }),
    ];
    expect(repoTeammates(peers)).toEqual([{ member: "ann", name: "Anna", agents: ["Codex"] }]);
  });

  it("matches only the session's own repository", () => {
    const activity: TeamActivity = {
      ...EMPTY_ACTIVITY,
      sessions: [3],
      sessionRepos: [{ sessionId: 3, repo: SHOP }],
      peers: [peer("ann", [session("a1", "Codex", DOCS)]), peer("bob", [session("b1", "Aider")])],
    };
    const activities = new Map([["p", activity]]);
    expect(repoTeammates(peersOf(activities, 3)).map((p) => p.member)).toEqual(["bob"]);
    expect(repoTeammates(peersOf(activities, 4))).toEqual([]);
    expect(repoTeammates(peersOf(new Map(), 3))).toEqual([]);
  });

  it("shows at most two people and counts the rest", () => {
    const people = ["ann", "bob", "cid", "dan"].map((member) => ({
      member,
      name: member,
      agents: [],
    }));
    expect(teammateStack(people, 2)).toEqual({ shown: people.slice(0, 2), more: 2 });
    expect(teammateStack(people.slice(0, 1), 2)).toEqual({ shown: people.slice(0, 1), more: 0 });
  });

  it("names each person with their programs", () => {
    expect(
      teammateNames([
        { member: "a", name: "Anna", agents: ["Claude Code"] },
        { member: "b", name: "Ben", agents: ["Codex", "Aider"] },
        { member: "c", name: "Cid", agents: [] },
      ]),
    ).toBe("Anna (Claude Code), Ben (Codex, Aider), Cid");
  });
});
