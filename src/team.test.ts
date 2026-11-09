import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  ARM_DELAY_MS,
  armDelayLeft,
  canToggle,
  confirmationKindText,
  isArmed,
  isSharing,
  parsePendingConfirmations,
  parseTeamError,
  parseTeamSnapshot,
  parseTeamStatus,
  requestPosition,
  shortId,
  shownRequest,
  statusFor,
  statusKey,
  statusLine,
  statusesOf,
  statusTone,
  teamErrorText,
  withoutRequest,
  type PendingConfirmation,
  type TeamStatus,
} from "./team";

const t = createTranslator("en");

const SAFETY = ["12345", "67890", "13579", "24680", "11223", "44556"];

const REQUEST = {
  id: "req-1",
  projectId: "317",
  member: "member-0123456789abcdef",
  device: "device-fedcba9876543210",
  kind: "ownDevice",
  safetyNumber: SAFETY,
  readmitted: false,
};

describe("parseTeamStatus", () => {
  it("reads every state", () => {
    expect(parseTeamStatus({ state: "off" })).toEqual({ state: "off" });
    expect(parseTeamStatus({ state: "connecting", retrying: true })).toEqual({
      state: "connecting",
      retrying: true,
    });
    expect(parseTeamStatus({ state: "connected", epoch: 4, members: 3, devices: 5 })).toEqual({
      state: "connected",
      epoch: 4,
      members: 3,
      devices: 5,
    });
    expect(parseTeamStatus({ state: "waitingForConfirmation", safetyNumber: SAFETY })).toEqual({
      state: "waitingForConfirmation",
      safetyNumber: SAFETY,
    });
    expect(parseTeamStatus({ state: "needsAttention", reason: "historyMismatch" })).toEqual({
      state: "needsAttention",
      reason: "historyMismatch",
    });
    expect(parseTeamStatus({ state: "removed" })).toEqual({ state: "removed" });
    expect(parseTeamStatus({ state: "projectDeleted" })).toEqual({ state: "projectDeleted" });
  });

  it("turns unknown reasons and unknown states into needs attention, never off or connected", () => {
    const unknown = { state: "needsAttention", reason: "unknown" };
    expect(parseTeamStatus({ state: "needsAttention", reason: "solarFlare" })).toEqual(unknown);
    expect(parseTeamStatus({ state: "migrating" })).toEqual(unknown);
  });

  it("rejects malformed values", () => {
    expect(parseTeamStatus(null)).toBeNull();
    expect(parseTeamStatus("off")).toBeNull();
    expect(parseTeamStatus({})).toBeNull();
    expect(parseTeamStatus({ state: "" })).toBeNull();
    expect(parseTeamStatus({ state: "connecting" })).toBeNull();
    expect(parseTeamStatus({ state: "connected", epoch: -1, members: 1, devices: 1 })).toBeNull();
    expect(parseTeamStatus({ state: "connected", epoch: 1, members: 1.5, devices: 1 })).toBeNull();
    expect(parseTeamStatus({ state: "needsAttention", reason: 3 })).toBeNull();
  });

  it("accepts only 6 groups of 5 digits as a safety number", () => {
    const waiting = (safetyNumber: unknown) =>
      parseTeamStatus({ state: "waitingForConfirmation", safetyNumber });
    expect(waiting(SAFETY.slice(0, 5))).toBeNull();
    expect(waiting([...SAFETY, "77889"])).toBeNull();
    expect(waiting(["1234", ...SAFETY.slice(1)])).toBeNull();
    expect(waiting(["12a45", ...SAFETY.slice(1)])).toBeNull();
    expect(waiting([12345, ...SAFETY.slice(1)])).toBeNull();
    expect(waiting("123451234512345")).toBeNull();
  });
});

describe("parsePendingConfirmations", () => {
  it("reads requests and keeps unknown kinds as unknown", () => {
    expect(parsePendingConfirmations([REQUEST])).toEqual([REQUEST]);
    expect(parsePendingConfirmations([{ ...REQUEST, kind: "newMember" }])?.[0]?.kind).toBe(
      "newMember",
    );
    expect(parsePendingConfirmations([{ ...REQUEST, kind: "robot" }])?.[0]?.kind).toBe("unknown");
    expect(parsePendingConfirmations([])).toEqual([]);
  });

  it("reads whether a removed member would be admitted again", () => {
    expect(parsePendingConfirmations([{ ...REQUEST, readmitted: true }])?.[0]?.readmitted).toBe(
      true,
    );
    expect(parsePendingConfirmations([REQUEST])?.[0]?.readmitted).toBe(false);
  });

  it("treats a missing or wrong readmitted flag as malformed, never as not readmitted", () => {
    const { readmitted: _dropped, ...withoutFlag } = REQUEST;
    expect(_dropped).toBe(false);
    expect(parsePendingConfirmations([withoutFlag])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, readmitted: "true" }])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, readmitted: 1 }])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, readmitted: null }])).toBeNull();
  });

  it("rejects malformed entries and repeated ids", () => {
    expect(parsePendingConfirmations(null)).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, id: "" }])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, member: 7 }])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, kind: null }])).toBeNull();
    expect(parsePendingConfirmations([{ ...REQUEST, safetyNumber: ["1"] }])).toBeNull();
    expect(parsePendingConfirmations([REQUEST, REQUEST])).toBeNull();
  });
});

describe("parseTeamSnapshot", () => {
  it("reads teams and pending confirmations", () => {
    const snapshot = parseTeamSnapshot({
      teams: [{ projectId: "317", status: { state: "connecting", retrying: false } }],
      pending: [REQUEST],
    });
    expect(snapshot?.teams).toEqual([
      { projectId: "317", status: { state: "connecting", retrying: false } },
    ]);
    expect(snapshot?.pending).toHaveLength(1);
  });

  it("rejects malformed snapshots", () => {
    expect(parseTeamSnapshot(null)).toBeNull();
    expect(parseTeamSnapshot({ teams: [], pending: null })).toBeNull();
    expect(parseTeamSnapshot({ teams: [{ projectId: "1" }], pending: [] })).toBeNull();
    expect(
      parseTeamSnapshot({ teams: [{ projectId: "", status: { state: "off" } }], pending: [] }),
    ).toBeNull();
    const twice = { projectId: "1", status: { state: "off" } };
    expect(parseTeamSnapshot({ teams: [twice, twice], pending: [] })).toBeNull();
  });

  it("treats projects missing from the snapshot as off", () => {
    const snapshot = parseTeamSnapshot({
      teams: [{ projectId: "1", status: { state: "removed" } }],
      pending: [],
    });
    expect(snapshot).not.toBeNull();
    if (snapshot === null) return;
    const statuses = statusesOf(snapshot);
    expect(statusFor(statuses, "1")).toEqual({ state: "removed" });
    expect(statusFor(statuses, "2")).toEqual({ state: "off" });
  });
});

describe("parseTeamError", () => {
  it("reads known kinds and falls back to unknown", () => {
    expect(parseTeamError({ kind: "busy", detail: null })).toEqual({ kind: "busy", detail: null });
    expect(parseTeamError({ kind: "keychain", detail: "locked" })).toEqual({
      kind: "keychain",
      detail: "locked",
    });
    expect(parseTeamError({ kind: "meteor", detail: null })).toEqual({
      kind: "unknown",
      detail: null,
    });
    expect(parseTeamError("boom")).toEqual({ kind: "unknown", detail: "boom" });
    expect(parseTeamError(undefined)).toEqual({ kind: "unknown", detail: null });
  });

  it("has a text for every kind", () => {
    for (const kind of [
      "notSignedIn",
      "unknownProject",
      "unknownRequest",
      "keychain",
      "config",
      "busy",
      "unknown",
    ] as const) {
      expect(teamErrorText({ kind, detail: null }, t)).not.toBe("");
    }
    expect(teamErrorText({ kind: "unknown", detail: null }, t)).toBe(t("error.unknown"));
  });
});

const STATES: TeamStatus[] = [
  { state: "off" },
  { state: "connecting", retrying: false },
  { state: "connecting", retrying: true },
  { state: "connected", epoch: 1, members: 2, devices: 3 },
  { state: "waitingForConfirmation", safetyNumber: SAFETY },
  { state: "needsAttention", reason: "historyMismatch" },
  { state: "needsAttention", reason: "signedOut" },
  { state: "needsAttention", reason: "keychain" },
  { state: "needsAttention", reason: "needsUpdate" },
  { state: "needsAttention", reason: "refused" },
  { state: "needsAttention", reason: "storage" },
  { state: "needsAttention", reason: "otherInstance" },
  { state: "needsAttention", reason: "unknown" },
  { state: "removed" },
  { state: "projectDeleted" },
];

describe("status texts", () => {
  it("shows nothing while off and a distinct text for every other state", () => {
    expect(statusLine({ state: "off" }, t)).toBeNull();
    const keys = STATES.filter((status) => status.state !== "off").map(statusKey);
    expect(keys).not.toContain(null);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("adds members and devices while connected", () => {
    expect(statusLine({ state: "connected", epoch: 9, members: 2, devices: 3 }, t)).toEqual({
      tone: "ok",
      text: "Connected · Members: 2 · Devices: 3",
    });
  });

  it("marks retrying and attention as problems", () => {
    expect(statusTone({ state: "connecting", retrying: false })).toBe("busy");
    expect(statusTone({ state: "connecting", retrying: true })).toBe("problem");
    expect(statusTone({ state: "needsAttention", reason: "refused" })).toBe("problem");
    expect(statusTone({ state: "removed" })).toBe("stopped");
  });

  it("marks unreadable team data as a problem and another window as waiting", () => {
    const storage: TeamStatus = { state: "needsAttention", reason: "storage" };
    const other: TeamStatus = { state: "needsAttention", reason: "otherInstance" };
    expect(parseTeamStatus({ state: "needsAttention", reason: "storage" })).toEqual(storage);
    expect(parseTeamStatus({ state: "needsAttention", reason: "otherInstance" })).toEqual(other);
    expect(statusKey(storage)).toBe("team.status.storage");
    expect(statusKey(other)).toBe("team.status.otherInstance");
    expect(statusTone(storage)).toBe("problem");
    expect(statusTone(other)).toBe("busy");
    expect(statusLine(other, t)).toEqual({
      tone: "busy",
      text: "Used by another Hangar window. This window takes over once the other one closes.",
    });
  });

  it("names the kind of a pending confirmation", () => {
    expect(confirmationKindText("ownDevice", t)).toBe("A new device of yours");
    expect(confirmationKindText("newMember", t)).toBe("A new member");
    expect(confirmationKindText("unknown", t)).toBe("A new device");
  });
});

describe("switch", () => {
  it("is on while sharing, off when off, removed or deleted", () => {
    const on = STATES.filter(isSharing).map((status) => status.state);
    expect(on).not.toContain("off");
    expect(on).not.toContain("removed");
    expect(on).not.toContain("projectDeleted");
    expect(isSharing({ state: "needsAttention", reason: "keychain" })).toBe(true);
    expect(isSharing({ state: "waitingForConfirmation", safetyNumber: SAFETY })).toBe(true);
  });

  it("cannot be turned on for a deleted project", () => {
    expect(canToggle({ state: "projectDeleted" })).toBe(false);
    expect(canToggle({ state: "removed" })).toBe(true);
    expect(canToggle({ state: "off" })).toBe(true);
  });
});

describe("helpers", () => {
  it("shortens long ids only", () => {
    expect(shortId("abc")).toBe("abc");
    expect(shortId("0123456789")).toBe("0123456789");
    expect(shortId("0123456789abcdef")).toBe("01234567…");
  });

  it("drops an answered request from the pending list", () => {
    const list = parsePendingConfirmations([REQUEST, { ...REQUEST, id: "req-2" }]);
    const pending: PendingConfirmation[] = list ?? [];
    expect(withoutRequest(pending, "req-1").map((entry) => entry.id)).toEqual(["req-2"]);
    expect(withoutRequest(pending, "nope")).toHaveLength(2);
  });
});

describe("one request at a time", () => {
  const entry = (id: string): PendingConfirmation => ({ ...REQUEST, kind: "ownDevice", id });
  const pending = [entry("req-1"), entry("req-2"), entry("req-3")];

  it("shows the first request when none was shown or nothing waits", () => {
    expect(shownRequest(pending, null)?.id).toBe("req-1");
    expect(shownRequest([], null)).toBeNull();
    expect(shownRequest([], "req-1")).toBeNull();
  });

  it("keeps showing the same request while it is still pending", () => {
    expect(shownRequest(pending, "req-2")?.id).toBe("req-2");
    expect(shownRequest([entry("req-0"), ...pending], "req-3")?.id).toBe("req-3");
  });

  it("moves on to the first request once the shown one has left", () => {
    expect(shownRequest(withoutRequest(pending, "req-2"), "req-2")?.id).toBe("req-1");
    expect(shownRequest(pending, "gone")?.id).toBe("req-1");
  });

  it("counts the position in the queue", () => {
    expect(requestPosition(pending, "req-1")).toEqual({ index: 1, count: 3 });
    expect(requestPosition(pending, "req-3")).toEqual({ index: 3, count: 3 });
    expect(requestPosition(pending, "gone")).toBeNull();
  });

  it("arms the answers only after the delay", () => {
    expect(ARM_DELAY_MS).toBe(1000);
    expect(isArmed(5000, 5000)).toBe(false);
    expect(isArmed(5000, 5000 + ARM_DELAY_MS - 1)).toBe(false);
    expect(isArmed(5000, 5000 + ARM_DELAY_MS)).toBe(true);
    expect(isArmed(5000, 9000)).toBe(true);
    expect(isArmed(0, 50, 50)).toBe(true);
  });

  it("tells how long until armed and treats a clock running backwards as just shown", () => {
    expect(armDelayLeft(5000, 5000)).toBe(ARM_DELAY_MS);
    expect(armDelayLeft(5000, 5400)).toBe(600);
    expect(armDelayLeft(5000, 7000)).toBe(0);
    expect(armDelayLeft(5000, 4000)).toBe(ARM_DELAY_MS);
    expect(isArmed(5000, 4000)).toBe(false);
  });
});
