import { describe, expect, it } from "vitest";

import {
  groupMenuEntries,
  leadEdit,
  olderIntroductions,
  pendingByGroup,
  sendInTurn,
  stackCards,
  stackSecrets,
} from "./linkCards";
import type { LinkGroup, LinkMessage, LinkState } from "./links";

const group: LinkGroup = {
  id: 1,
  color: 0,
  askFirst: false,
  braked: false,
  brake: "none",
  budgetLeft: 30,
  budget: 30,
  pendingIntroductions: [],
  members: [
    { session: 1, name: "app", role: "member" },
    { session: 2, name: "web", role: "lead" },
    { session: 3, name: "docs", role: "member" },
  ],
};

function message(
  id: number,
  fromName: string | null,
  overrides: Partial<LinkMessage> = {},
): LinkMessage {
  return {
    id,
    group: 1,
    fromName,
    to: 1,
    text: `text ${String(id)}`,
    at: id * 10,
    auto: false,
    claimed: false,
    braked: false,
    ...overrides,
  };
}

function state(messages: LinkMessage[], groups: LinkGroup[] = [group]): LinkState {
  return { groups, sendable: new Set([1, 2, 3]), unattended: new Set(), messages };
}

describe("stackCards", () => {
  it("keeps the cards of members as they are, oldest first", () => {
    const list = [message(1, "web"), message(2, "docs")];
    const stack = stackCards(list);
    expect(stack.cards.map((m) => m.id)).toEqual([1, 2]);
    expect(stack.superseded.size).toBe(0);
  });

  it("shows only the newest introduction of a group; it takes the older ones along", () => {
    const list = [message(1, null), message(2, "web"), message(3, null), message(4, null)];
    const stack = stackCards(list);
    expect(stack.cards.map((m) => m.id)).toEqual([2, 4]);
    expect(stack.superseded.get(4)).toEqual([1, 3]);
  });

  it("keeps one introduction per group apart", () => {
    const list = [message(1, null), message(2, null, { group: 2 })];
    expect(stackCards(list).cards.map((m) => m.id)).toEqual([1, 2]);
  });

  it("tells introductions of the same time apart by id", () => {
    const list = [message(5, null, { at: 1 }), message(6, null, { at: 1 })];
    const stack = stackCards(list);
    expect(stack.cards.map((m) => m.id)).toEqual([6]);
    expect(stack.superseded.get(6)).toEqual([5]);
  });
});

describe("stackSecrets", () => {
  it("names every kind of secret of the stack once, in order", () => {
    expect(
      stackSecrets([{ secrets: ["githubToken"] }, {}, { secrets: ["privateKey", "githubToken"] }]),
    ).toEqual(["githubToken", "privateKey"]);
    expect(stackSecrets([{}, {}])).toEqual([]);
  });
});

describe("pendingByGroup", () => {
  it("lists what goes on its own per group, with the member names", () => {
    const pending = pendingByGroup(
      state([
        message(1, "web", { auto: true, to: 1 }),
        message(2, null, { auto: true, to: 3 }),
        // Waits for a click: a card, not on its way.
        message(3, "app", { to: 2 }),
      ]),
    );
    expect(pending.get(1)).toEqual([
      { id: 1, fromName: "web", toName: "app", text: "text 1" },
      { id: 2, fromName: null, toName: "docs", text: "text 2" },
    ]);
  });

  it("counts a paste of this window that outlasted the backend's claim", () => {
    const pending = pendingByGroup(state([message(1, "web")]), new Set([1]));
    expect(pending.get(1)?.map((entry) => entry.id)).toEqual([1]);
  });

  it("counts a claimed message, which is being pasted now", () => {
    const pending = pendingByGroup(state([message(1, "web", { auto: true, claimed: true })]));
    expect(pending.get(1)).toHaveLength(1);
  });

  it("leaves out a message of a group or target that is gone", () => {
    expect(pendingByGroup(state([message(1, "web", { auto: true, group: 9 })])).size).toBe(0);
    expect(pendingByGroup(state([message(1, "web", { auto: true, to: 7 })])).size).toBe(0);
  });
});

describe("sendInTurn", () => {
  it("sends one after another, each after the one before went in", async () => {
    const order: string[] = [];
    const sent = await sendInTurn([1, 2, 3], async (id) => {
      order.push(`start ${String(id)}`);
      await Promise.resolve();
      order.push(`end ${String(id)}`);
      return true;
    });
    expect(sent).toBe(3);
    expect(order).toEqual(["start 1", "end 1", "start 2", "end 2", "start 3", "end 3"]);
  });

  it("stops at the first that did not go in and leaves the rest", async () => {
    const tried: number[] = [];
    const sent = await sendInTurn([1, 2, 3], (id) => {
      tried.push(id);
      return Promise.resolve(id !== 2);
    });
    expect(sent).toBe(1);
    expect(tried).toEqual([1, 2]);
  });
});

describe("groupMenuEntries", () => {
  it("offers the lead among the members, then edit, history and unlink", () => {
    expect(groupMenuEntries(group)).toEqual([
      { kind: "lead", session: null, current: false },
      { kind: "lead", session: 1, current: false },
      { kind: "lead", session: 2, current: true },
      { kind: "lead", session: 3, current: false },
      { kind: "edit" },
      { kind: "history" },
      { kind: "unlink" },
    ]);
  });

  it("marks No lead when nobody leads", () => {
    const plain = {
      ...group,
      members: group.members.map((m) => ({ ...m, role: "member" as const })),
    };
    expect(groupMenuEntries(plain)[0]).toEqual({ kind: "lead", session: null, current: true });
  });

  it("says first which part of the brake holds, to let it go on", () => {
    expect(groupMenuEntries({ ...group, braked: true, brake: "rate" })[0]).toEqual({
      kind: "paused",
      brake: "rate",
    });
    expect(groupMenuEntries({ ...group, braked: true, brake: "budget", budgetLeft: 0 })[0]).toEqual(
      { kind: "paused", brake: "budget" },
    );
    expect(groupMenuEntries(group).some((entry) => entry.kind === "paused")).toBe(false);
  });

  it("tells the budget left quietly once some of it is used", () => {
    expect(groupMenuEntries({ ...group, budgetLeft: 12 })[0]).toEqual({
      kind: "budget",
      left: 12,
      total: 30,
    });
    // Unused, or while every message waits for a click anyway: nothing to say.
    expect(groupMenuEntries(group).some((entry) => entry.kind === "budget")).toBe(false);
    expect(
      groupMenuEntries({ ...group, askFirst: true, budgetLeft: 3 }).some(
        (entry) => entry.kind === "budget",
      ),
    ).toBe(false);
  });

  it("names the introductions still owed", () => {
    const entries = groupMenuEntries({ ...group, pendingIntroductions: [3] });
    expect(entries[0]).toEqual({ kind: "pendingIntro", sessions: [3] });
  });
});

describe("leadEdit", () => {
  it("changes the lead only, and only to a member", () => {
    expect(leadEdit(group, 1)).toEqual({ names: [], askFirst: false, lead: 1 });
    expect(leadEdit(group, null)).toEqual({ names: [], askFirst: false, lead: null });
    // Leads already, or no member: nothing to do.
    expect(leadEdit(group, 2)).toBeNull();
    expect(leadEdit(group, 9)).toBeNull();
  });
});

describe("olderIntroductions", () => {
  const delivered = message(5, null, { at: 50 });

  it("finds the older introductions for the same session and group", () => {
    const list = [
      message(1, null, { at: 10 }),
      message(4, null, { at: 50 }),
      delivered,
      // Newer than the delivered one: it stays.
      message(6, null, { at: 60 }),
      message(7, null, { at: 50 }),
    ];
    expect(olderIntroductions(list, delivered)).toEqual([1, 4]);
  });

  it("never takes a member's message, another group's or another session's", () => {
    const list = [
      message(1, "web", { at: 10 }),
      message(2, null, { at: 10, group: 2 }),
      message(3, null, { at: 10, to: 3 }),
      delivered,
    ];
    expect(olderIntroductions(list, delivered)).toEqual([]);
  });

  it("leaves an older introduction that a click is pasting right now", () => {
    const list = [message(1, null, { at: 10, claimed: true }), delivered];
    expect(olderIntroductions(list, delivered)).toEqual([]);
  });

  it("does nothing after a member's message went in", () => {
    const list = [message(1, null, { at: 10 }), message(5, "web", { at: 50 })];
    expect(olderIntroductions(list, message(5, "web", { at: 50 }))).toEqual([]);
  });
});
