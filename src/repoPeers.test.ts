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
      { member: "ann", name: "ANN", agents: ["Claude Code", "Aider"], presence: "online" },
      { member: "bob", name: "bob", agents: ["Codex"], presence: "online" },
    ]);
  });

  it("takes a name from another device when the first sent none", () => {
    const peers = [
      peer("ann", [session("a1", "Codex")], { name: " " }),
      peer("ann", [session("a2", "Codex")], { device: "laptop", name: "Anna" }),
    ];
    expect(repoTeammates(peers)).toEqual([
      { member: "ann", name: "Anna", agents: ["Codex"], presence: "online" },
    ]);
  });

  it("shows a person away only while every device of theirs there is away", () => {
    const away = { presence: "away" as const };
    expect(repoTeammates([peer("ann", [session("a1", "Codex")], away)])[0]?.presence).toBe("away");
    const both = [
      peer("ann", [session("a1", "Codex")], away),
      peer("ann", [session("a2", "Codex")], { device: "laptop" }),
    ];
    expect(repoTeammates(both)[0]?.presence).toBe("online");
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
      presence: "online" as const,
    }));
    expect(teammateStack(people, 2)).toEqual({ shown: people.slice(0, 2), more: 2 });
    expect(teammateStack(people.slice(0, 1), 2)).toEqual({ shown: people.slice(0, 1), more: 0 });
  });

  it("names each person with their programs", () => {
    expect(
      teammateNames([
        { member: "a", name: "Anna", agents: ["Claude Code"], presence: "online" },
        { member: "b", name: "Ben", agents: ["Codex", "Aider"], presence: "away" },
        { member: "c", name: "Cid", agents: [], presence: "online" },
      ]),
    ).toBe("Anna (Claude Code), Ben (Codex, Aider), Cid");
  });
});
