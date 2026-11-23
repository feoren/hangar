import { describe, expect, it } from "vitest";

import type { AccountState, AccountView, Me, Project } from "./account";
import type { TeamStatus } from "./team";
import {
  isTeamProject,
  noSeatHint,
  offersTeam,
  onFreePlan,
  PRICING_URL,
  refusedForSeat,
  seatsFull,
  teamAccess,
} from "./teamPlan";

function view(state: AccountState): AccountView {
  return {
    baseUrl: "https://hangar.example.com/api/v1",
    official: true,
    devicesUrl: "https://hangar.example.com/devices",
    state,
    otherSignIns: [],
    revision: 1,
  };
}

function me(total: number | null, used: number | null = 1): Me {
  return {
    id: "u1",
    email: "ada@example.com",
    name: "Ada",
    seatsAccountTotal: total,
    seatsAccountUsed: used,
    avatarVersion: null,
  };
}

function project(patch: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Shop",
    role: "owner",
    plan: "free",
    seatsAccountTotal: 1,
    seatsAccountUsed: 1,
    pendingInvitations: null,
    canInvite: true,
    ...patch,
  };
}

const signedIn = (user: Me | null) => view({ kind: "signedIn", user });
const REFUSED: TeamStatus = { state: "needsAttention", reason: "refused" };
const OFF: TeamStatus = { state: "off" };

describe("teamAccess", () => {
  it("waits while the account has not answered, so nobody is locked out by a slow start", () => {
    expect(teamAccess(null, null)).toBe("pending");
    expect(offersTeam("pending")).toBe(true);
  });

  it("offers no team functions signed out, also while a sign-in runs or failed", () => {
    expect(teamAccess(view({ kind: "idle", notice: null }), null)).toBe("no");
    const failed = view({ kind: "failed", reason: { kind: "network", detail: null } });
    expect(teamAccess(failed, null)).toBe("no");
    const waiting = view({
      kind: "waitingForUser",
      userCode: "ABCD",
      verificationUri: "https://hangar.example.com/device",
      verificationUriComplete: "https://hangar.example.com/device?code=ABCD",
      expiresAt: 0,
      interval: 5,
    });
    expect(teamAccess(waiting, null)).toBe("no");
    expect(offersTeam("no")).toBe(false);
  });

  it("offers no team functions on Free without a team: one person, private use", () => {
    expect(teamAccess(signedIn(me(1)), [])).toBe("no");
    expect(teamAccess(signedIn(me(1)), [project()])).toBe("no");
  });

  it("offers them to a member of someone else's team, paid or grandfathered", () => {
    const member = project({ role: "member", canInvite: false, seatsAccountTotal: 1 });
    expect(teamAccess(signedIn(me(1)), [member])).toBe("yes");
    expect(teamAccess(signedIn(me(1)), [project({ role: "admin" })])).toBe("yes");
    // A role this Hangar does not know is still someone else's team.
    expect(teamAccess(signedIn(me(1)), [project({ role: "other" })])).toBe("yes");
  });

  it("offers them to an owner whose Free team already holds more people (grandfathered)", () => {
    const grandfathered = project({ seatsAccountTotal: 1, seatsAccountUsed: 3 });
    expect(teamAccess(signedIn(me(1, 3)), [grandfathered])).toBe("yes");
  });

  it("offers them to an account with more than one seat, even before projects load", () => {
    expect(teamAccess(signedIn(me(5)), null)).toBe("yes");
    expect(teamAccess(signedIn(me(5)), [])).toBe("yes");
    // The project list may know a fresh purchase before the stored person does.
    expect(teamAccess(signedIn(me(1)), [project({ seatsAccountTotal: 5 })])).toBe("yes");
  });

  it("waits while the projects or the person's details are still missing", () => {
    expect(teamAccess(signedIn(me(1)), null)).toBe("pending");
    expect(teamAccess(signedIn(null), [])).toBe("pending");
    expect(teamAccess(signedIn(me(null, null)), null)).toBe("pending");
    // A team is a team, whatever else is missing.
    expect(teamAccess(signedIn(null), [project({ role: "member" })])).toBe("yes");
  });
});

describe("isTeamProject", () => {
  it("counts own projects only with room for or people beyond the owner", () => {
    expect(isTeamProject(project())).toBe(false);
    expect(isTeamProject(project({ seatsAccountTotal: null, seatsAccountUsed: null }))).toBe(false);
    expect(isTeamProject(project({ seatsAccountUsed: 2 }))).toBe(true);
    expect(isTeamProject(project({ seatsAccountTotal: 3 }))).toBe(true);
    expect(isTeamProject(project({ role: "member" }))).toBe(true);
  });
});

describe("seats", () => {
  it("is full when every seat is taken, never without numbers", () => {
    expect(seatsFull(project({ seatsAccountTotal: 3, seatsAccountUsed: 3 }))).toBe(true);
    expect(seatsFull(project({ seatsAccountTotal: 1, seatsAccountUsed: 3 }))).toBe(true);
    expect(seatsFull(project({ seatsAccountTotal: 5, seatsAccountUsed: 3 }))).toBe(false);
    expect(seatsFull(project({ seatsAccountTotal: null, seatsAccountUsed: 3 }))).toBe(false);
    expect(seatsFull(project({ seatsAccountTotal: 3, seatsAccountUsed: null }))).toBe(false);
  });

  it("names the plan instead of Invite people when no seat is free", () => {
    const full = project({ seatsAccountTotal: 3, seatsAccountUsed: 3 });
    expect(noSeatHint(full, OFF)).toBe(true);
    expect(noSeatHint(project({ seatsAccountTotal: 5, seatsAccountUsed: 3 }), OFF)).toBe(false);
    // Members cannot invite: nothing to replace for them while they are connected.
    expect(noSeatHint({ ...full, role: "member", canInvite: false }, OFF)).toBe(false);
  });

  it("names the plan next to a refusal when every seat is taken", () => {
    const full = project({ role: "member", canInvite: false, seatsAccountUsed: 3 });
    expect(refusedForSeat(full, REFUSED)).toBe(true);
    expect(noSeatHint(full, REFUSED)).toBe(true);
    expect(refusedForSeat(full, OFF)).toBe(false);
    const room = project({ role: "member", seatsAccountTotal: 10, seatsAccountUsed: 3 });
    expect(refusedForSeat(room, REFUSED)).toBe(false);
    const otherReason: TeamStatus = { state: "needsAttention", reason: "keychain" };
    expect(refusedForSeat(full, otherReason)).toBe(false);
  });

  it("knows Free by its single seat", () => {
    expect(onFreePlan(me(1))).toBe(true);
    expect(onFreePlan(me(3))).toBe(false);
    expect(onFreePlan(me(null))).toBe(false);
  });

  it("sends people to the pricing page", () => {
    expect(PRICING_URL).toBe("https://hangar.feoren.com/pricing");
  });
});
