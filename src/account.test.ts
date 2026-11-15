import { describe, expect, it } from "vitest";

import {
  accountButtonText,
  accountErrorText,
  avatarMarker,
  avatarSource,
  displayName,
  failText,
  focusAfterChange,
  isCurrentView,
  isSignedIn,
  lacksUser,
  minutesLeft,
  nextAvatarFetch,
  otherSignInsText,
  parseAccountError,
  parseAccountView,
  parseProjects,
  projectMeta,
  projectsAction,
  roleText,
  seatsText,
  serverHost,
  USER_RECHECK_MS,
  type AccountView,
  type Project,
} from "./account";
import { createTranslator } from "./i18n";

const t = createTranslator("en");

const NO_OTHERS: string[] = [];

const BASE = {
  baseUrl: "https://hangar.example.test/api/v1",
  official: false,
  devicesUrl: "https://hangar.example.test/account/devices",
  otherSignIns: NO_OTHERS,
  revision: 1,
};

function view(state: unknown, base: Partial<typeof BASE> = {}): unknown {
  return { ...BASE, ...base, state };
}

const USER = {
  id: "7",
  email: "ada@example.test",
  name: "Ada",
  locale: "de",
  seatsAccountTotal: 3,
  seatsAccountUsed: 2,
  avatarVersion: null,
};

const PROJECT: Project = {
  id: "317",
  name: "Core",
  role: "owner",
  plan: "free",
  seatsAccountTotal: 3,
  seatsAccountUsed: 2,
  pendingInvitations: 1,
  canInvite: true,
};

describe("parseAccountView", () => {
  it("reads every state of the sign-in", () => {
    expect(parseAccountView(view({ kind: "idle", notice: null }))?.state).toEqual({
      kind: "idle",
      notice: null,
    });
    expect(parseAccountView(view({ kind: "idle", notice: "sessionEnded" }))?.state).toEqual({
      kind: "idle",
      notice: "sessionEnded",
    });
    expect(parseAccountView(view({ kind: "idle", notice: "signedOutLocally" }))?.state).toEqual({
      kind: "idle",
      notice: "signedOutLocally",
    });
    expect(
      parseAccountView(
        view({ kind: "idle", notice: null }, { otherSignIns: ["https://a.example.test/api/v1"] }),
      )?.otherSignIns,
    ).toEqual(["https://a.example.test/api/v1"]);
    const waiting = parseAccountView(
      view({
        kind: "waitingForUser",
        userCode: "BCDF-GHJK",
        verificationUri: "https://hangar.example.test/device",
        verificationUriComplete: "https://hangar.example.test/device?user_code=BCDF-GHJK",
        expiresAt: 1_800_000_000_000,
        interval: 5,
      }),
    );
    expect(waiting?.state.kind).toBe("waitingForUser");
    expect(parseAccountView(view({ kind: "signedIn", user: null }))?.state).toEqual({
      kind: "signedIn",
      user: null,
    });
    const signedIn = parseAccountView(view({ kind: "signedIn", user: USER }));
    expect(signedIn?.state).toEqual({
      kind: "signedIn",
      user: {
        id: "7",
        email: "ada@example.test",
        name: "Ada",
        seatsAccountTotal: 3,
        seatsAccountUsed: 2,
        avatarVersion: null,
      },
    });
    const failed = parseAccountView(
      view({ kind: "failed", reason: { kind: "denied", detail: null } }),
    );
    expect(failed?.state).toEqual({ kind: "failed", reason: { kind: "denied", detail: null } });
  });

  it("refuses malformed views", () => {
    expect(parseAccountView(null)).toBeNull();
    expect(parseAccountView(view({ kind: "unknown" }))).toBeNull();
    expect(parseAccountView(view({ kind: "idle", notice: "other" }))).toBeNull();
    expect(parseAccountView(view({ kind: "failed", reason: { kind: "nope" } }))).toBeNull();
    expect(
      parseAccountView(view({ kind: "waitingForUser", userCode: "X", expiresAt: -1 })),
    ).toBeNull();
    expect(parseAccountView({ ...BASE, official: "no", state: { kind: "idle" } })).toBeNull();
    expect(parseAccountView({ ...BASE, otherSignIns: [1], state: { kind: "idle" } })).toBeNull();
    expect(
      parseAccountView({
        baseUrl: BASE.baseUrl,
        official: false,
        devicesUrl: BASE.devicesUrl,
        state: { kind: "idle" },
      }),
    ).toBeNull();
  });
});

describe("the signed-in person", () => {
  const PNG = "data:image/png;base64,iVBORw0KGgo=";

  it("carries only a picture marker in the view", () => {
    const parsed = parseAccountView(
      view({ kind: "signedIn", user: { ...USER, avatarVersion: 3 } }),
    );
    expect(parsed?.state).toMatchObject({ kind: "signedIn", user: { avatarVersion: 3 } });
    expect(parsed !== null && avatarMarker(parsed)).toBe(3);
    for (const avatarVersion of [undefined, null, -1, 1.5, "3"]) {
      const odd = parseAccountView(view({ kind: "signedIn", user: { ...USER, avatarVersion } }));
      expect(odd?.state).toMatchObject({ user: { name: "Ada", avatarVersion: null } });
    }
  });

  it("asks for a picture once per version", () => {
    const at = (avatarVersion: number | null): AccountView => ({
      ...BASE,
      state: { kind: "signedIn", user: { ...USER, avatarVersion } },
    });
    expect(nextAvatarFetch(null, at(1))).toBe(1);
    expect(nextAvatarFetch(1, at(1))).toBeNull();
    expect(nextAvatarFetch(1, at(2))).toBe(2);
    expect(nextAvatarFetch(1, at(null))).toBeNull();
    expect(nextAvatarFetch(null, null)).toBeNull();
    expect(nextAvatarFetch(null, { ...BASE, state: { kind: "idle", notice: null } })).toBeNull();
  });

  it("accepts only data URLs of the allowed image types from the backend", () => {
    for (const avatar of [
      PNG,
      "data:image/webp;base64,UklGRg==",
      "data:image/jpeg;base64,/9j/4A==",
    ]) {
      expect(avatarSource(avatar)).toBe(avatar);
    }
    for (const avatar of [
      undefined,
      null,
      42,
      "https://hangar.example.test/api/v1/me/avatar?v=0123456789abcdef",
      "data:image/svg+xml;base64,PHN2Zy8+",
      "data:text/html;base64,PGI+",
      'data:image/png;base64,AA" onerror="x',
    ]) {
      expect(avatarSource(avatar)).toBeNull();
    }
  });

  it("keeps the view signed in when the person is malformed", () => {
    for (const user of [{ id: "7" }, { ...USER, email: 3 }, { ...USER, seatsAccountUsed: -1 }]) {
      expect(parseAccountView(view({ kind: "signedIn", user }))?.state).toEqual({
        kind: "signedIn",
        user: null,
      });
    }
  });
});

describe("view order", () => {
  const at = (revision: number): AccountView => ({
    ...BASE,
    revision,
    state: { kind: "signedIn", user: null },
  });

  it("never lets an older answer replace a newer event", () => {
    expect(isCurrentView(null, at(0))).toBe(true);
    expect(isCurrentView(at(3), at(4))).toBe(true);
    expect(isCurrentView(at(3), at(3))).toBe(true);
    expect(isCurrentView(at(4), at(3))).toBe(false);
  });

  it("reads the revision and refuses a broken one", () => {
    expect(parseAccountView(view({ kind: "idle", notice: null }))?.revision).toBe(1);
    expect(parseAccountView({ ...BASE, revision: -1, state: { kind: "idle" } })).toBeNull();
    expect(parseAccountView({ ...BASE, revision: "2", state: { kind: "idle" } })).toBeNull();
  });

  it("asks again only while the person is missing", () => {
    expect(lacksUser(at(1))).toBe(true);
    expect(lacksUser({ ...BASE, state: { kind: "idle", notice: null } })).toBe(false);
    expect(lacksUser(null)).toBe(false);
    expect(USER_RECHECK_MS.length).toBeGreaterThan(0);
  });
});

describe("parseProjects", () => {
  it("reads projects and refuses repeated ids", () => {
    const projects = parseProjects([
      { ...PROJECT, ownerId: "7" },
      {
        id: "318",
        name: "Docs",
        role: "member",
        plan: null,
        seatsAccountTotal: null,
        seatsAccountUsed: null,
        pendingInvitations: null,
        canInvite: false,
      },
    ]);
    expect(projects?.map((project) => project.id)).toEqual(["317", "318"]);
    expect(parseProjects([PROJECT, PROJECT])).toBeNull();
    expect(parseProjects([{ ...PROJECT, role: "boss" }])).toBeNull();
    expect(parseProjects([{ ...PROJECT, canInvite: "yes" }])).toBeNull();
    expect(parseProjects([{ ...PROJECT, seatsAccountUsed: -1 }])).toBeNull();
    expect(parseProjects({ projects: [] })).toBeNull();
    expect(parseProjects([])).toEqual([]);
  });
});

describe("parseAccountError", () => {
  it("keeps known kinds and the detail", () => {
    expect(parseAccountError({ kind: "server", detail: "Busy" })).toEqual({
      kind: "server",
      detail: "Busy",
    });
    expect(parseAccountError({ kind: "baseUrl", detail: null })).toEqual({
      kind: "baseUrl",
      detail: null,
    });
    expect(parseAccountError("boom")).toEqual({ kind: "unknown", detail: "boom" });
    expect(parseAccountError({ kind: "new" })).toEqual({ kind: "unknown", detail: null });
  });
});

describe("projectsAction", () => {
  const signedIn: AccountView = { ...BASE, state: { kind: "signedIn", user: null } };
  const idle: AccountView = { ...BASE, state: { kind: "idle", notice: null } };
  const elsewhere: AccountView = {
    ...signedIn,
    baseUrl: "https://other.example.test/api/v1",
  };

  it("loads on sign-in and clears on sign-out", () => {
    expect(projectsAction(null, signedIn)).toBe("load");
    expect(projectsAction(idle, signedIn)).toBe("load");
    expect(projectsAction(signedIn, signedIn)).toBe("keep");
    expect(projectsAction(signedIn, idle)).toBe("clear");
    expect(projectsAction(idle, idle)).toBe("keep");
    expect(projectsAction(null, idle)).toBe("keep");
  });

  it("reloads after a server change", () => {
    expect(projectsAction(signedIn, elsewhere)).toBe("load");
    expect(isSignedIn(elsewhere)).toBe(true);
    expect(isSignedIn(null)).toBe(false);
  });
});

describe("texts", () => {
  it("counts minutes up, never below one", () => {
    expect(minutesLeft(600_000, 0)).toBe(10);
    expect(minutesLeft(61_000, 0)).toBe(2);
    expect(minutesLeft(1_000, 0)).toBe(1);
    expect(minutesLeft(0, 5_000)).toBe(1);
  });

  it("names the person and the server", () => {
    expect(displayName({ ...USER })).toBe("Ada");
    expect(displayName({ ...USER, name: "  " })).toBe("ada@example.test");
    expect(displayName({ ...USER, name: null })).toBe("ada@example.test");
    expect(serverHost("https://team.example.test:8443/api/v1")).toBe("team.example.test:8443");
    expect(serverHost("nonsense")).toBe("nonsense");
  });

  it("labels the account button without repeating a word", () => {
    const signedIn = (user: unknown) => parseAccountView(view({ kind: "signedIn", user }));
    expect(accountButtonText(signedIn(USER), t)).toEqual({
      text: "Ada",
      label: "Account: Ada",
      title: "Account: Ada",
    });
    const pending = accountButtonText(signedIn(null), t);
    expect(pending).toEqual({ text: "Signed in", label: "Signed in", title: "Signed in" });
    expect(pending.title).not.toContain("Account");
    const signedOut = accountButtonText(parseAccountView(view({ kind: "idle", notice: null })), t);
    expect(signedOut.text).toBe("Sign in");
    expect(signedOut.title).toBe("Sign in to work with your team");
    expect(accountButtonText(null, t).text).toBe("Sign in");
  });

  it("builds the project line from what is known", () => {
    expect(projectMeta(PROJECT, t)).toEqual(["free", "2 of 3 seats", "Pending invitations: 1"]);
    expect(
      projectMeta(
        {
          ...PROJECT,
          plan: null,
          seatsAccountTotal: null,
          pendingInvitations: 0,
        },
        t,
      ),
    ).toEqual([]);
    expect(seatsText({ seatsAccountTotal: 10, seatsAccountUsed: 4 }, t)).toBe("4 of 10 seats");
    expect(roleText("owner", t)).toBe("Owner");
    expect(roleText("other", t)).toBeNull();
  });

  it("has a text for every failure and error", () => {
    expect(failText({ kind: "foreignPage", detail: null }, t)).toContain("another site");
    expect(accountErrorText({ kind: "baseUrl", detail: null }, t)).toContain("https://");
    expect(accountErrorText({ kind: "unknown", detail: null }, t)).not.toBe("");
  });
});

describe("focusAfterChange", () => {
  it("moves the focus to the main button of the new block", () => {
    expect(focusAfterChange("waitingForUser", "signedIn")).toBe("signOut");
    expect(focusAfterChange("signedIn", "idle")).toBe("signIn");
    expect(focusAfterChange("idle", "waitingForUser")).toBe("copyCode");
    expect(focusAfterChange("waitingForUser", "failed")).toBe("tryAgain");
  });

  it("leaves the focus alone on the first render and without a change", () => {
    expect(focusAfterChange(null, "signedIn")).toBeNull();
    expect(focusAfterChange("signedIn", "signedIn")).toBeNull();
  });
});

describe("otherSignInsText", () => {
  const others = ["https://a.example.test/api/v1", "http://127.0.0.1:8080/api/v1"];

  it("names the other servers after a sign-out", () => {
    for (const notice of ["signedOut", "signedOutLocally"] as const) {
      const parsed = parseAccountView(view({ kind: "idle", notice }, { otherSignIns: others }));
      expect(parsed).not.toBeNull();
      if (parsed === null) continue;
      expect(otherSignInsText(parsed, t)).toBe(
        "Sign-ins for other servers stay stored on this device: a.example.test, 127.0.0.1:8080. Switch to a server to sign out there.",
      );
    }
  });

  it("says nothing otherwise", () => {
    const signedIn = parseAccountView(
      view({ kind: "signedIn", user: null }, { otherSignIns: others }),
    );
    const alone = parseAccountView(view({ kind: "idle", notice: "signedOut" }));
    expect(signedIn !== null && otherSignInsText(signedIn, t)).toBeNull();
    expect(alone !== null && otherSignInsText(alone, t)).toBeNull();
  });
});
