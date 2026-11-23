import { describe, expect, it } from "vitest";

import {
  AUTO_POLL_MS,
  autoReadiness,
  deliverAutomatically,
  EMPTY_LINKS,
  TYPING_PAUSE_MS,
  canLink,
  groupOf,
  groupsWithRoom,
  heldBySession,
  MAX_LINK_MEMBERS,
  selectableForLink,
  selectionPlan,
  linkCandidates,
  ownLineKey,
  linkColor,
  linkMarks,
  linkOffer,
  nameProblem,
  newlyBraked,
  nextDeliveries,
  parseLinkState,
  selectedForLink,
  toggleSelection,
  validMemberName,
  type AutoDeliveryDeps,
  type LinkGroup,
  type LinkMessage,
  type LinkState,
  type TargetFacts,
} from "./links";

const group: LinkGroup = {
  id: 1,
  color: 2,
  askFirst: false,
  braked: false,
  members: [
    { session: 1, name: "app", role: "member" },
    { session: 2, name: "web", role: "member" },
  ],
};

function message(
  id: number,
  to: number,
  auto: boolean,
  fromName: string | null = "app",
): LinkMessage {
  return { id, group: 1, fromName, to, text: "go on", at: id, auto, claimed: false };
}

function state(overrides: Partial<LinkState> = {}): LinkState {
  return {
    groups: [group],
    sendable: new Set([1, 2, 3, 4]),
    unattended: new Set(),
    messages: [],
    ...overrides,
  };
}

const sessions = [
  { id: 1, status: "running" },
  { id: 2, status: "running" },
  { id: 3, status: "running" },
  { id: 4, status: "exited" },
  { id: 5, status: "running" },
  { id: 6, status: "running", kind: "update" },
];

describe("parseLinkState", () => {
  it("reads the backend's snapshot", () => {
    const parsed = parseLinkState({
      groups: [
        {
          id: 1,
          color: 2,
          askFirst: true,
          braked: false,
          members: [
            { session: 1, name: "app" },
            { session: 2, name: "web" },
          ],
        },
      ],
      sendable: [1, 2],
      unattended: [2],
      messages: [
        { id: 3, group: 1, fromName: null, to: 1, text: "hi", at: 5, auto: true, claimed: false },
      ],
    });
    expect(parsed.groups[0]?.askFirst).toBe(true);
    expect([...parsed.sendable]).toEqual([1, 2]);
    expect(parsed.messages[0]?.fromName).toBeNull();
    expect([...parsed.unattended]).toEqual([2]);
  });

  it("leaves out what is malformed", () => {
    const parsed = parseLinkState({
      groups: [
        { ...group, color: 9 },
        { ...group, members: [{ session: 1, name: "app" }] },
        {
          ...group,
          members: [
            { session: 1, name: "App" },
            { session: 2, name: "web" },
          ],
        },
        group,
      ],
      sendable: [1, -1, "2", 1.5],
      messages: [
        { ...message(1, 2, true), text: " " },
        { ...message(2, 2, true), fromName: "Bad Name" },
        { ...message(0, 2, true) },
        message(3, 2, true),
      ],
    });
    expect(parsed.groups).toEqual([group]);
    expect([...parsed.sendable]).toEqual([1]);
    expect(parsed.messages.map((entry) => entry.id)).toEqual([3]);
    expect(parseLinkState(null)).toBe(EMPTY_LINKS);
    expect(parseLinkState({ groups: "x" }).groups).toEqual([]);
  });
});

describe("linking", () => {
  it("offers only running agent sessions with the client that are in no group", () => {
    const current = state();
    expect(canLink(current, { id: 3, status: "running" })).toBe(true);
    // A member, an ended one, one started before linking existed, an update.
    expect(canLink(current, { id: 1, status: "running" })).toBe(false);
    expect(canLink(current, { id: 4, status: "exited" })).toBe(false);
    expect(canLink(current, { id: 5, status: "running" })).toBe(false);
    expect(
      canLink(state({ sendable: new Set([6]) }), { id: 6, status: "running", kind: "update" }),
    ).toBe(false);
    expect(linkCandidates(current, sessions).map((s) => s.id)).toEqual([3]);
  });

  it("offers the menu entry only where another session or a group could take this one", () => {
    // Session 3 is the only one left, but the group has room for it.
    const lone = state();
    expect(linkOffer(lone, { id: 3, status: "running" }, sessions)).toBe("link");
    const eight = Array.from({ length: MAX_LINK_MEMBERS }, (_, index) => ({
      session: 10 + index,
      name: `m${String(index)}`,
      role: "member" as const,
    }));
    const full = state({ groups: [{ ...group, members: eight }] });
    expect(groupsWithRoom(full)).toEqual([]);
    expect(linkOffer(full, { id: 3, status: "running" }, sessions)).toBe("link");
    expect(
      linkOffer(full, { id: 3, status: "running" }, [{ id: 3, status: "running" }]),
    ).toBeNull();
    const two = state({ groups: [] });
    expect(linkOffer(two, { id: 3, status: "running" }, sessions)).toBe("link");
    expect(linkOffer(lone, { id: 1, status: "running" }, sessions)).toBe("member");
    expect(linkOffer(two, { id: 5, status: "running" }, sessions)).toBeNull();
    expect(groupOf(lone, 2)?.id).toBe(1);
    expect(groupOf(lone, 3)).toBeUndefined();
  });

  it("selects with a modifier click only what can be linked", () => {
    const empty: ReadonlySet<number> = new Set();
    const one = toggleSelection(empty, 3, true);
    expect(one === null ? null : [...one]).toEqual([3]);
    expect(toggleSelection(empty, 1, false)).toBeNull();
    const none = one === null ? empty : toggleSelection(one, 3, true);
    expect(none === null ? null : [...none]).toEqual([]);
    const current = state({ groups: [] });
    expect(selectedForLink(current, sessions, new Set([2, 4, 1])).map((s) => s.id)).toEqual([1, 2]);
  });

  it("lets the selection form a new group or join the one group among it", () => {
    const current = state();
    // Members can be selected too, so the others join their group.
    expect([...selectableForLink(current, sessions)]).toEqual([1, 2, 3]);
    const plan = (ids: number[], links: LinkState = current) =>
      selectionPlan(links, sessions, new Set(ids));
    expect(plan([1, 3])).toEqual({ kind: "add", group, ids: [3] });
    expect(plan([3, 1, 2])).toEqual({ kind: "add", group, ids: [3] });
    // Nobody to add, or an ended session only.
    expect(plan([1, 2])).toBeNull();
    expect(plan([1, 4])).toBeNull();
    const free = state({ groups: [] });
    expect(plan([1, 3], free)).toEqual({ kind: "new", ids: [1, 3] });
    expect(plan([3], free)).toBeNull();
    // Two groups among the selection: "Link" would not know which.
    const other: LinkGroup = {
      ...group,
      id: 2,
      members: [
        { session: 3, name: "relay", role: "member" },
        { session: 5, name: "pi", role: "member" },
      ],
    };
    const both = state({ groups: [group, other], sendable: new Set([1, 2, 3, 5, 7]) });
    const wide = [...sessions, { id: 7, status: "running" }];
    expect(selectionPlan(both, wide, new Set([1, 3, 7]))).toBeNull();
    expect(selectionPlan(both, wide, new Set([3, 7]))).toEqual({
      kind: "add",
      group: other,
      ids: [7],
    });
    // A group that would grow beyond the limit.
    const eight = Array.from({ length: MAX_LINK_MEMBERS }, (_, index) => ({
      session: 10 + index,
      name: `m${String(index)}`,
      role: "member" as const,
    }));
    const full = state({ groups: [{ ...group, members: eight }], sendable: new Set([3, 10]) });
    expect(
      selectionPlan(
        full,
        [
          { id: 10, status: "running" },
          { id: 3, status: "running" },
        ],
        new Set([10, 3]),
      ),
    ).toBeNull();
  });

  it("checks member names as the backend does", () => {
    expect(validMemberName("web-2")).toBe(true);
    for (const bad of ["", "Web", "all", "a b", "x".repeat(33)]) {
      expect(validMemberName(bad), bad).toBe(false);
    }
    expect(nameProblem(["app", " web "])).toBeNull();
    expect(nameProblem(["app", "App"])).toBe("invalid");
    expect(nameProblem(["app", "all"])).toBe("reserved");
    expect(nameProblem(["app", "app "])).toBe("twice");
  });
});

describe("marks", () => {
  it("marks every member with its group's color and name", () => {
    const marks = linkMarks(state({ groups: [{ ...group, braked: true }] }));
    expect(marks.get(2)).toEqual({ group: 1, color: 2, name: "web", braked: true, lead: false });
    expect(marks.has(3)).toBe(false);
    expect(linkColor(2)).toBe("var(--link-2)");
    expect(linkColor(8)).toBe("var(--link-2)");
    expect(linkColor(-1)).toBe("var(--link-0)");
  });

  it("names a group once when its brake turns on", () => {
    const braked = { ...group, braked: true };
    expect(newlyBraked([group], [braked])).toEqual([braked]);
    expect(newlyBraked([braked], [braked])).toEqual([]);
    expect(newlyBraked([braked], [group])).toEqual([]);
  });
});

describe("delivery", () => {
  it("delivers one message at a time per target, oldest first", () => {
    const messages = [
      message(1, 2, true),
      message(2, 2, true),
      message(3, 1, false),
      message(4, 1, true),
      message(5, 3, true),
    ];
    expect(nextDeliveries(messages, new Set()).map((m) => m.id)).toEqual([1, 4, 5]);
    expect(nextDeliveries(messages, new Set([2, 3])).map((m) => m.id)).toEqual([4]);
  });

  it("shows the held messages below their target's row", () => {
    const rows = heldBySession([
      message(1, 2, false),
      message(2, 2, true),
      message(3, 2, false, null),
    ]);
    expect(rows.get(2)?.map((m) => m.id)).toEqual([1, 3]);
    expect(rows.has(1)).toBe(false);
  });
});

describe("claimed messages", () => {
  it("are neither delivered again nor shown as a card", () => {
    const claimed = { ...message(1, 2, true), claimed: true };
    const held = { ...message(2, 3, false), claimed: true };
    expect(nextDeliveries([claimed, message(3, 2, true)], new Set()).map((m) => m.id)).toEqual([3]);
    expect(heldBySession([held]).size).toBe(0);
  });

  it("get no card while their automatic delivery still runs, even once the claim ran out", () => {
    // A slow paste can outlast the backend's claim: the message then reads as a held one, and
    // a card's "Send" would paste it a second time.
    const ranOut = message(4, 2, false);
    expect(heldBySession([ranOut], new Set([4])).size).toBe(0);
    expect(
      heldBySession([ranOut], new Set([9]))
        .get(2)
        ?.map((m) => m.id),
    ).toEqual([4]);
    // Once the delivery ended, it is a card again.
    expect(
      heldBySession([ranOut], new Set())
        .get(2)
        ?.map((m) => m.id),
    ).toEqual([4]);
  });
});

const QUIET: TargetFacts = {
  state: "attention",
  reason: "quiet",
  endedBy: "quiet",
  asks: false,
  gatePending: false,
  typedAgoMs: null,
  draft: false,
};

describe("autoReadiness", () => {
  it("is ready only once the target went quiet after working", () => {
    expect(autoReadiness(QUIET)).toBe("ready");
    // On screen, the quiet end turns the state idle and keeps its reason.
    expect(autoReadiness({ ...QUIET, state: "idle", reason: null })).toBe("ready");
    // Never worked, working, a bell, an exit, a gate decision or a question: wait.
    expect(autoReadiness({ ...QUIET, state: "idle", reason: null, endedBy: null })).toBe("wait");
    expect(autoReadiness({ ...QUIET, state: "working", reason: null })).toBe("wait");
    expect(autoReadiness({ ...QUIET, reason: "bell", endedBy: "bell" })).toBe("wait");
    expect(autoReadiness({ ...QUIET, state: "idle", endedBy: "bell" })).toBe("wait");
    expect(autoReadiness({ ...QUIET, reason: "gate" })).toBe("wait");
    expect(autoReadiness({ ...QUIET, gatePending: true })).toBe("wait");
    expect(autoReadiness({ ...QUIET, asks: true })).toBe("wait");
  });

  it("never sends Enter while the agent's hooks report a dialog, whatever the screen says", () => {
    // A quiet end with an empty-looking screen would be ready; the hook signal decides.
    expect(autoReadiness({ ...QUIET, permissionPending: true })).toBe("wait");
    expect(autoReadiness({ ...QUIET, reason: "permission" })).toBe("wait");
    expect(autoReadiness({ ...QUIET, permissionPending: false })).toBe("ready");
  });

  it("waits while the person types or left a line half typed", () => {
    expect(autoReadiness({ ...QUIET, draft: true })).toBe("typing");
    expect(autoReadiness({ ...QUIET, typedAgoMs: TYPING_PAUSE_MS - 1 })).toBe("typing");
    expect(autoReadiness({ ...QUIET, typedAgoMs: TYPING_PAUSE_MS })).toBe("ready");
  });
});

interface Fake {
  deps: AutoDeliveryDeps;
  log: string[];
  facts: { current: TargetFacts };
  waited: () => number;
}

function fake(overrides: Partial<AutoDeliveryDeps> = {}, start: TargetFacts = QUIET): Fake {
  const log: string[] = [];
  const facts = { current: start };
  let waited = 0;
  const deps: AutoDeliveryDeps = {
    facts: () => facts.current,
    gone: () => false,
    bracketedPaste: () => true,
    quietFor: () => 5000,
    take: () => {
      log.push("take");
      return Promise.resolve("[Message]\n| hi");
    },
    paste: (text) => {
      log.push(`paste:${text}`);
      return Promise.resolve();
    },
    submit: () => {
      log.push("enter");
    },
    close: () => {
      log.push("close");
      return Promise.resolve();
    },
    wait: (ms) => {
      waited += ms;
      return Promise.resolve();
    },
    ...overrides,
  };
  return { deps, log, facts, waited: () => waited };
}

const PATIENT = { waitWhileWorking: true, patienceMs: 1000, typingPatienceMs: 2000 };

describe("deliverAutomatically", () => {
  it("claims, pastes, presses Enter as its own write and closes", async () => {
    const { deps, log } = fake();
    expect(await deliverAutomatically(deps, PATIENT, 10)).toBe("pasted");
    expect(log).toEqual(["take", "paste:[Message]\n| hi", "enter", "close"]);
  });

  it("waits for a working target without a limit, then delivers", async () => {
    const working = { ...QUIET, state: "working" as const, reason: null, endedBy: null };
    const box = fake({}, working);
    let polls = 0;
    box.deps.wait = () => {
      polls += 1;
      if (polls === 40) box.facts.current = QUIET;
      return Promise.resolve();
    };
    expect(await deliverAutomatically(box.deps, PATIENT, 10)).toBe("pasted");
    expect(polls * AUTO_POLL_MS).toBeGreaterThan(PATIENT.patienceMs);
  });

  it("gives up on a target that rests without having worked", async () => {
    const fresh = { ...QUIET, state: "idle" as const, reason: null, endedBy: null };
    const box = fake({}, fresh);
    expect(await deliverAutomatically(box.deps, PATIENT, 10)).toBe("timeout");
    expect(box.log).toEqual([]);
    expect(box.waited()).toBeGreaterThanOrEqual(PATIENT.patienceMs - AUTO_POLL_MS);
    // The introduction does not wait for a working target either.
    const busy = fake({}, { ...fresh, state: "working" });
    expect(await deliverAutomatically(busy.deps, { ...PATIENT, waitWhileWorking: false }, 10)).toBe(
      "timeout",
    );
  });

  it("gives up while the person keeps typing", async () => {
    const box = fake({}, { ...QUIET, draft: true });
    expect(await deliverAutomatically(box.deps, PATIENT, 10)).toBe("typing");
    expect(box.log).toEqual([]);
  });

  it("turns a message into a card while a dialog waits, without pasting", async () => {
    const asked = fake();
    asked.facts.current = { ...QUIET, permissionPending: true };
    expect(await deliverAutomatically(asked.deps, PATIENT, 10)).toBe("timeout");
    expect(asked.log).toEqual([]);
  });

  it("pastes nothing when the backend holds it or the target changed after the claim", async () => {
    const refused = fake({ take: () => Promise.reject(new Error("[link_held] waits")) });
    expect(await deliverAutomatically(refused.deps, PATIENT, 10)).toBe("refused");
    expect(refused.log).toEqual([]);
    const changed = fake();
    changed.deps.take = () => {
      changed.facts.current = { ...QUIET, asks: true };
      return Promise.resolve("x");
    };
    expect(await deliverAutomatically(changed.deps, PATIENT, 10)).toBe("failed");
    expect(changed.log).toEqual([]);
    const lost = fake({ paste: () => Promise.reject(new Error("stalled")) });
    expect(await deliverAutomatically(lost.deps, PATIENT, 10)).toBe("failed");
    expect(lost.log).toEqual(["take"]);
  });

  it("needs bracketed paste and a quiet output, and stops for a gone target", async () => {
    const noPaste = fake({ bracketedPaste: () => false });
    expect(await deliverAutomatically(noPaste.deps, PATIENT, 10)).toBe("timeout");
    const gone = fake({ gone: () => true });
    expect(await deliverAutomatically(gone.deps, PATIENT, 10)).toBe("gone");
    expect(gone.log).toEqual([]);
    // A failed close leaves the message claimed; the delivery still counts as pasted.
    const stuck = fake({ close: () => Promise.reject(new Error("gone")) });
    expect(await deliverAutomatically(stuck.deps, PATIENT, 10)).toBe("pasted");
  });
});

describe("ownLineKey", () => {
  const keys = (sequence: string[]): boolean[] => {
    let line = false;
    return sequence.map((key) => {
      const step = ownLineKey(line, key);
      line = step.line;
      return step.sent;
    });
  };

  it("counts an Enter that sends a line the person typed", () => {
    expect(keys(["f", "i", "x", "\r"])).toEqual([false, false, false, true]);
    expect(keys(["café 👋", "\r"])).toEqual([false, true]);
  });

  it("does not count a bare Enter or arrows and Enter on a dialog", () => {
    expect(keys(["\r"])).toEqual([false]);
    expect(keys(["\x1b[B", "\x1b[B", "\r"])).toEqual([false, false, false]);
    expect(keys(["\x1b", "\t", "\x7f", "\r"])).toEqual([false, false, false, false]);
  });

  it("starts over after Enter or Ctrl+C", () => {
    expect(keys(["a", "\r", "\r"])).toEqual([false, true, false]);
    expect(keys(["a", "\x03", "\r"])).toEqual([false, false, false]);
  });
});
