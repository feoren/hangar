import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  ARM_DELAY_MS,
  armDelayLeft,
  canToggle,
  confirmationKindText,
  divergentText,
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
  activitiesOf,
  EMPTY_ACTIVITY,
  folderLabel,
  freshTeammateOverlaps,
  isTeammateFileOverlap,
  legacyLinks,
  needsRelink,
  personName,
  personTag,
  personWithAgent,
  rememberSeen,
  repoChoices,
  sessionStateOf,
  sessionStates,
  sessionStateText,
  teammateMarkerText,
  teammateNotice,
  teammateNoticeDue,
  teammateOverlapKey,
  teammateOverlapsBySession,
  teammateOverlapText,
  type PendingConfirmation,
  type TeamActivity,
  type TeammateOverlap,
  type TeamStatus,
} from "./team";

const t = createTranslator("en");

const CONNECTED = { state: "connected", epoch: 1, members: 2, devices: 2 };

const PEER = {
  member: "bob-0123456789abcdef",
  device: "d1",
  name: "Bob",
  own: false,
  presence: "active",
  sessions: [
    {
      key: "k1",
      agent: "Codex",
      title: "Login",
      state: "working",
      files: ["src/auth.rs"],
      moreFiles: 0,
    },
  ],
};

const OVERLAP: TeammateOverlap = {
  sessionId: 3,
  member: "bob-0123456789abcdef",
  device: "d1",
  otherSession: "k1",
  name: "Bob",
  agent: "Codex",
  title: "Login",
  level: "overlappingLines",
  path: "src/auth.rs",
  lines: [{ start: 10, end: 15 }],
  files: 1,
};

const ACTIVITY = {
  linked: [{ root: "a".repeat(40), folder: "c:/src/shop/.git", name: "shop" }],
  peers: [PEER],
  overlaps: [OVERLAP],
  divergent: [{ member: "carol-0123456789abcdef", device: "device-0123456789", name: "Carol" }],
};

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
      {
        projectId: "317",
        status: { state: "connecting", retrying: false },
        activity: EMPTY_ACTIVITY,
      },
    ]);
    expect(snapshot?.pending).toHaveLength(1);
    expect(snapshot?.repos).toEqual([]);
  });

  it("reads the teammates' activity and the repositories to link", () => {
    const snapshot = parseTeamSnapshot({
      teams: [{ projectId: "317", status: CONNECTED, activity: ACTIVITY }],
      pending: [],
      repos: [{ root: "a".repeat(40), folder: "c:/src/shop/.git", name: "shop" }],
    });
    expect(snapshot?.teams[0]?.activity.peers[0]?.sessions[0]?.files).toEqual(["src/auth.rs"]);
    expect(snapshot?.teams[0]?.activity.overlaps[0]?.lines).toEqual([{ start: 10, end: 15 }]);
    expect(snapshot?.repos[0]?.name).toBe("shop");
    const activities = snapshot === null ? new Map() : activitiesOf(snapshot);
    expect(activities.get("317")).toEqual(ACTIVITY);
  });

  it("rejects malformed activity", () => {
    const team = (activity: unknown) => ({
      teams: [{ projectId: "317", status: CONNECTED, activity }],
      pending: [],
    });
    expect(parseTeamSnapshot(team({ ...ACTIVITY, peers: null }))).toBeNull();
    expect(
      parseTeamSnapshot(team({ ...ACTIVITY, overlaps: [{ ...OVERLAP, sessionId: -1 }] })),
    ).toBeNull();
    expect(
      parseTeamSnapshot(team({ ...ACTIVITY, overlaps: [{ ...OVERLAP, level: "x" }] })),
    ).toBeNull();
    // Overlap rows are keyed by the teammate's session key as well: it must be there.
    expect(
      parseTeamSnapshot(team({ ...ACTIVITY, overlaps: [{ ...OVERLAP, otherSession: "" }] })),
    ).toBeNull();
    expect(parseTeamSnapshot(team({ ...ACTIVITY, linked: [{ root: "", name: "x" }] }))).toBeNull();
    expect(parseTeamSnapshot({ teams: [], pending: [], repos: [{ root: 1 }] })).toBeNull();
    // A folder offered for linking always names its folder; a link from before folders may not.
    expect(
      parseTeamSnapshot({ teams: [], pending: [], repos: [{ root: "a", name: "x" }] }),
    ).toBeNull();
    const legacy = parseTeamSnapshot(team({ ...ACTIVITY, linked: [{ root: "a", name: "x" }] }));
    expect(legacy?.teams[0]?.activity.linked[0]?.folder).toBeNull();
    // Rows are keyed by session key: a missing or repeated key is malformed.
    const session = PEER.sessions[0];
    expect(
      parseTeamSnapshot(team({ ...ACTIVITY, peers: [{ ...PEER, sessions: [session, session] }] })),
    ).toBeNull();
    expect(
      parseTeamSnapshot(
        team({ ...ACTIVITY, peers: [{ ...PEER, sessions: [{ ...session, key: "" }] }] }),
      ),
    ).toBeNull();
    // A session state this Hangar does not know reads as idle.
    const future = parseTeamSnapshot(
      team({
        ...ACTIVITY,
        peers: [{ ...PEER, sessions: [{ ...PEER.sessions[0], state: "dreaming" }] }],
      }),
    );
    expect(future?.teams[0]?.activity.peers[0]?.sessions[0]?.state).toBe("idle");
    // A device reporting another history must name who it is.
    expect(
      parseTeamSnapshot(team({ ...ACTIVITY, divergent: [{ member: "", device: "d", name: "" }] })),
    ).toBeNull();
    expect(parseTeamSnapshot(team({ ...ACTIVITY, divergent: null }))).toBeNull();
    // An older backend without the list: none.
    const older = parseTeamSnapshot(team({ linked: [], peers: [], overlaps: [] }));
    expect(older?.teams[0]?.activity.divergent).toEqual([]);
  });

  it("names a device that reports a different team history", () => {
    const device = ACTIVITY.divergent[0];
    if (device === undefined) throw new Error("fixture");
    const text = divergentText(device, t);
    expect(text).toContain("Carol #carol-01…");
    expect(text).toContain("device device-0…");
    expect(text).toContain("different team history");
    // Without a name the member id stands in; invisible characters show as marks.
    expect(divergentText({ ...device, name: "" }, t)).toContain("Member carol-01…");
    expect(divergentText({ ...device, name: "Ca‮rol" }, t)).not.toContain("‮");
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

describe("teammates", () => {
  const activity = (overlaps: TeammateOverlap[]): TeamActivity => ({
    linked: [],
    peers: [],
    overlaps,
    divergent: [],
  });

  it("maps the attention states to the team's session states", () => {
    expect(sessionStateOf("working")).toBe("working");
    expect(sessionStateOf("attention")).toBe("waiting");
    expect(sessionStateOf("idle")).toBe("idle");
    expect(sessionStateOf(undefined)).toBe("idle");
    expect(sessionStates([2, 1], new Map([[1, "working" as const]]))).toEqual([
      { sessionId: 2, state: "idle" },
      { sessionId: 1, state: "working" },
    ]);
    expect(sessionStateText("waiting", t)).toBe("waiting");
  });

  it("offers linked repositories first and every running one once", () => {
    const shop = { root: "a", folder: "c:/src/shop/.git", name: "shop" };
    const site = { root: "b", folder: "c:/src/site/.git", name: "site" };
    expect(repoChoices([shop], [site, shop])).toEqual([
      { repo: shop, linked: true },
      { repo: site, linked: false },
    ]);
    // Another clone of the linked repository is offered unticked.
    const fork = { ...shop, folder: "c:/src/fork/.git" };
    expect(repoChoices([shop], [shop, fork])).toEqual([
      { repo: shop, linked: true },
      { repo: fork, linked: false },
    ]);
    expect(folderLabel("c:/src/shop/.git")).toBe("c:/src/shop");
    expect(folderLabel("/srv/bare.git")).toBe("/srv/bare.git");
  });

  it("asks to tick the folders again for a link from before folders", () => {
    const legacy = { root: "a", folder: null, name: "shop" };
    const shop = { root: "a", folder: "c:/src/shop/.git", name: "shop" };
    // Not offered as ticked, and the running folder waits for the person.
    expect(repoChoices([legacy], [shop])).toEqual([{ repo: shop, linked: false }]);
    expect(needsRelink([legacy], [shop])).toBe(true);
    expect(needsRelink([legacy, shop], [shop])).toBe(false);
    expect(needsRelink([legacy], [])).toBe(false);
    expect(needsRelink([shop], [shop])).toBe(false);
    // The old link can be removed on its own; folder links are not listed for that.
    expect(legacyLinks([legacy, shop])).toEqual([legacy]);
    expect(legacyLinks([shop])).toEqual([]);
  });

  it("names people by their name, their id or as the own other device", () => {
    expect(personName({ name: "Anna", member: "m" }, t)).toBe("Anna");
    expect(personName({ name: " ", member: "member-0123456789" }, t)).toBe("Member member-0…");
    expect(personName({ name: "Anna", member: "m", own: true }, t)).toBe("You (another device)");
    // A self-chosen name always comes with the member id the log vouches for.
    expect(personTag({ name: "Anna", member: "member-0123456789" }, t)).toBe("Anna #member-0…");
    expect(personTag({ name: "", member: "m" }, t)).toBe("Member m");
    expect(personTag({ name: "Anna", member: "m", own: true }, t)).toBe("You (another device)");
    expect(personWithAgent({ name: "Anna", member: "m", agent: "Claude Code" }, t)).toBe(
      "Anna #m (Claude Code)",
    );
    expect(personWithAgent({ name: "Anna", member: "m", agent: "" }, t)).toBe("Anna #m");
    // Names come from other devices: hidden and reordering characters show as marks.
    expect(personWithAgent({ name: "An‮na", member: "m", agent: "X​" }, t)).toBe(
      "An<U+202E>na #m (X<U+200B>)",
    );
  });

  it("describes overlaps down to the lines, the file or the area", () => {
    expect(teammateOverlapText(OVERLAP, t)).toBe(
      "Bob #bob-0123… (Codex) is changing the same lines: src/auth.rs, lines 10 to 15",
    );
    expect(teammateOverlapText({ ...OVERLAP, level: "sameFile", lines: [] }, t)).toBe(
      "Bob #bob-0123… (Codex) is changing this file too: src/auth.rs",
    );
    expect(teammateOverlapText({ ...OVERLAP, level: "sameArea", path: null }, t)).toBe(
      "Bob #bob-0123… (Codex) is working in the same area",
    );
    // A file the relay named but this device cannot: no marker, no notice, the area text.
    const unnamed = { ...OVERLAP, level: "sameFile" as const, path: null };
    expect(isTeammateFileOverlap(unnamed)).toBe(false);
    expect(teammateOverlapText(unnamed, t)).toBe(
      "Bob #bob-0123… (Codex) is working in the same area",
    );
    expect(teammateOverlapText({ ...OVERLAP, path: "src/\u202Ex.rs", level: "sameFile" }, t)).toBe(
      "Bob #bob-0123… (Codex) is changing this file too: src/<U+202E>x.rs",
    );
  });

  it("groups overlaps by session, worst first, and marks only file-level ones", () => {
    const area = { ...OVERLAP, level: "sameArea" as const, path: null, member: "c" };
    const grouped = teammateOverlapsBySession(
      new Map([
        ["p1", activity([area])],
        ["p2", activity([OVERLAP, { ...OVERLAP, sessionId: 4 }])],
      ]),
    );
    expect(grouped.get(3)?.map((o) => o.level)).toEqual(["overlappingLines", "sameArea"]);
    expect(teammateMarkerText(grouped.get(3) ?? [], t)).toBe(
      "Bob #bob-0123… (Codex) is changing the same lines: src/auth.rs, lines 10 to 15",
    );
    expect(teammateMarkerText([area], t)).toBeNull();
    expect(teammateMarkerText([OVERLAP, { ...OVERLAP, device: "d2" }], t)).toMatch(/\(\+1\)$/);
  });

  it("announces each collision once per session, device and file", () => {
    const seen = new Set<string>();
    const activities = new Map([["p1", activity([OVERLAP, { ...OVERLAP, level: "sameArea" }])]]);
    const fresh = freshTeammateOverlaps(seen, activities);
    expect(fresh).toEqual([OVERLAP]);
    for (const overlap of fresh) seen.add(teammateOverlapKey(overlap));
    expect(freshTeammateOverlaps(seen, activities)).toEqual([]);
    // Another file of the same teammate is news.
    const other = { ...OVERLAP, path: "src/other.rs" };
    expect(freshTeammateOverlaps(seen, new Map([["p1", activity([other])]]))).toEqual([other]);
    expect(teammateNotice(OVERLAP, "Claude 1", t)).toEqual({
      title: "Bob #bob-0123… (Codex) is changing the same file as Claude 1",
      body: "src/auth.rs, lines 10 to 15",
    });
  });

  it("keeps the announced collisions and the desktop notices bounded", () => {
    const seen = new Set<string>();
    rememberSeen(seen, ["a", "b", "c"], 2);
    expect([...seen]).toEqual(["b", "c"]);
    // Seen again: it becomes the newest and stays.
    rememberSeen(seen, ["b", "d"], 2);
    expect([...seen]).toEqual(["b", "d"]);
    // However many collisions arrive, the desktop hears of them once per gap.
    expect(teammateNoticeDue(null, 1000)).toBe(true);
    expect(teammateNoticeDue(1000, 1000 + 59_999)).toBe(false);
    expect(teammateNoticeDue(1000, 1000 + 60_000)).toBe(true);
    expect(teammateNoticeDue(5000, 1000)).toBe(false);
  });

  it("knows the new attention reason and error kind", () => {
    const other: TeamStatus = { state: "needsAttention", reason: "otherAccount" };
    expect(parseTeamStatus(other)).toEqual(other);
    expect(statusKey(other)).toBe("team.status.otherAccount");
    expect(teamErrorText(parseTeamError({ kind: "unknownRepository", detail: null }), t)).toBe(
      "No session runs in that repository any more.",
    );
  });
});
