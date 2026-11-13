import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { EMPTY_ACTIVITY, type PeerView, type TeamActivity, type TeamStatus } from "./team";
import {
  anyShared,
  emailInitials,
  initialTab,
  initials,
  meInitials,
  onlinePeople,
  pillLabel,
  shownProject,
  shownTab,
  teamPill,
  teamTabs,
} from "./teamOverview";

const t = createTranslator("en");

const CONNECTED: TeamStatus = { state: "connected", epoch: 1, members: 3, devices: 4 };
const WAITING: TeamStatus = { state: "waitingForConfirmation", safetyNumber: [] };

function peer(member: string, patch: Partial<PeerView> = {}): PeerView {
  return {
    member,
    device: `${member}-device`,
    name: member,
    own: false,
    presence: "active",
    sessions: [],
    ...patch,
  };
}

function activity(peers: PeerView[]): TeamActivity {
  return { ...EMPTY_ACTIVITY, peers };
}

describe("initials", () => {
  it("takes the first letters of the first and the last word", () => {
    expect(initials("Anna Berg")).toBe("AB");
    expect(initials("anna maria berg")).toBe("AB");
    expect(initials("  anna ")).toBe("A");
  });

  it("works with other scripts and skips signs", () => {
    expect(initials("Ελένη Παππά")).toBe("ΕΠ");
    expect(initials("(Bob) [Lee]")).toBe("BL");
    expect(initials("李明")).toBe("李");
  });

  it("is empty without letters or digits", () => {
    expect(initials("")).toBe("");
    expect(initials(" - ")).toBe("");
  });

  it("uses only the local part of an e-mail address", () => {
    expect(emailInitials("anna.berg@example.com")).toBe("AB");
    expect(emailInitials("bob@example.com")).toBe("B");
  });
});

describe("onlinePeople", () => {
  it("lists other people once, without own devices, away people or unconnected projects", () => {
    const statuses = new Map<string, TeamStatus>([
      ["p1", CONNECTED],
      ["p2", CONNECTED],
      ["p3", WAITING],
    ]);
    const activities = new Map<string, TeamActivity>([
      [
        "p1",
        activity([
          peer("anna"),
          peer("me", { own: true }),
          peer("ben", { presence: "away" }),
          peer("cleo", { presence: "idle" }),
        ]),
      ],
      ["p2", activity([peer("anna", { device: "other" }), peer("dan")])],
      ["p3", activity([peer("eve")])],
    ]);
    expect(onlinePeople(statuses, activities).map((person) => person.member)).toEqual([
      "anna",
      "cleo",
      "dan",
    ]);
  });

  it("reveals invisible characters in names", () => {
    const statuses = new Map([["p1", CONNECTED]]);
    const activities = new Map([["p1", activity([peer("anna", { name: " An‮na " })])]]);
    const [person] = onlinePeople(statuses, activities);
    expect(person?.name).not.toContain("‮");
  });
});

describe("teamPill", () => {
  const none = new Map<string, TeamActivity>();
  const byName = (person: { name: string }) => person.name;

  it("is off without shared projects", () => {
    const pill = teamPill(new Map([["p1", { state: "off" }]]), none, 0);
    expect(pill.kind).toBe("off");
    expect(pill.tone).toBe("quiet");
    expect(pillLabel(pill, t, byName)).toBe("Team: Not connected");
  });

  it("puts open requests first", () => {
    const pill = teamPill(new Map([["p1", CONNECTED]]), none, 2);
    expect(pill.kind).toBe("requests");
    expect(pill.tone).toBe("attention");
    expect(pillLabel(pill, t, byName)).toBe("Team: Requests: 2");
  });

  it("asks for attention while this device waits for its confirmation", () => {
    const pill = teamPill(
      new Map<string, TeamStatus>([
        ["p1", CONNECTED],
        ["p2", WAITING],
      ]),
      none,
      0,
    );
    expect(pill.kind).toBe("waiting");
    expect(pill.tone).toBe("attention");
    expect(pillLabel(pill, t, byName)).toBe("Team: Waiting for confirmation");
  });

  it("asks for attention when a connection has a problem", () => {
    const pill = teamPill(new Map([["p1", { state: "connecting", retrying: true }]]), none, 0);
    expect(pill.kind).toBe("problem");
  });

  it("stays calm while another window holds the connection", () => {
    const pill = teamPill(
      new Map([["p1", { state: "needsAttention", reason: "otherInstance" }]]),
      none,
      0,
    );
    expect(pill.kind).toBe("off");
  });

  it("counts the teammates online while connected", () => {
    const statuses = new Map([["p1", CONNECTED]]);
    const pill = teamPill(statuses, new Map([["p1", activity([peer("anna"), peer("ben")])]]), 0);
    expect(pill.kind).toBe("connected");
    expect(pill.tone).toBe("ok");
    expect(pill.online).toHaveLength(2);
    expect(pillLabel(pill, t, byName)).toBe("Team: 2 online: anna, ben");
  });

  it("says connected when nobody else is online", () => {
    const pill = teamPill(new Map([["p1", CONNECTED]]), none, 0);
    expect(pillLabel(pill, t, byName)).toBe("Team: Connected");
  });

  it("shows connecting without asking for attention", () => {
    const pill = teamPill(new Map([["p1", { state: "connecting", retrying: false }]]), none, 0);
    expect(pill.kind).toBe("connecting");
    expect(pill.tone).toBe("quiet");
  });
});

describe("tabs", () => {
  it("offers requests only while there are any", () => {
    expect(teamTabs(0)).toEqual(["now", "people"]);
    expect(teamTabs(1)).toEqual(["now", "people", "requests"]);
  });

  it("opens on the requests while there are any", () => {
    expect(initialTab(0)).toBe("now");
    expect(initialTab(3)).toBe("requests");
  });

  it("falls back to now when the chosen tab is gone", () => {
    expect(shownTab("requests", 0)).toBe("now");
    expect(shownTab("requests", 1)).toBe("requests");
    expect(shownTab("people", 0)).toBe("people");
  });
});

describe("shownProject", () => {
  const statuses = new Map<string, TeamStatus>([["p2", CONNECTED]]);

  it("keeps the chosen project while it exists", () => {
    expect(shownProject(["p1", "p2"], "p1", statuses)).toBe("p1");
  });

  it("prefers the first shared project, then the first one", () => {
    expect(shownProject(["p1", "p2"], null, statuses)).toBe("p2");
    expect(shownProject(["p1", "p2"], "gone", new Map())).toBe("p1");
    expect(shownProject([], null, statuses)).toBeNull();
  });

  it("opens on a project waiting for its confirmation, then on the one shown last", () => {
    const waiting = new Map<string, TeamStatus>([
      ["p2", CONNECTED],
      ["p3", WAITING],
    ]);
    expect(shownProject(["p1", "p2", "p3"], null, waiting, "p2")).toBe("p3");
    expect(shownProject(["p1", "p2", "p3"], "p2", waiting, null)).toBe("p2");
    expect(shownProject(["p1", "p2"], null, statuses, "p1")).toBe("p1");
    expect(shownProject(["p1", "p2"], null, statuses, "gone")).toBe("p2");
  });

  it("knows whether any project is shared", () => {
    expect(anyShared(["p1", "p2"], statuses)).toBe(true);
    expect(anyShared(["p1"], statuses)).toBe(false);
    expect(anyShared(["p1"], new Map([["p1", { state: "removed" }]]))).toBe(false);
  });
});

describe("meInitials", () => {
  const me = {
    id: "u1",
    email: "anna.berg@example.com",
    seatsAccountTotal: null,
    seatsAccountUsed: null,
    avatarVersion: null,
  };

  it("prefers the name and falls back to the e-mail address", () => {
    expect(meInitials({ ...me, name: "Anna Maria" })).toBe("AM");
    expect(meInitials({ ...me, name: "  " })).toBe("AB");
    expect(meInitials({ ...me, name: null })).toBe("AB");
    expect(meInitials(null)).toBe("");
  });
});
