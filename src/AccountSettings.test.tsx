// @vitest-environment happy-dom

// Settings, Account: the sign-in block and the server address share one card under the intro.
// The server address stays folded behind one line naming the server in use, open from the start
// only for an own server. When the sign-in state changes, the focus moves to the new block's main
// button, but stays in the server field when it was there. The account store is stubbed.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AccountState, AccountView } from "./account";
import { createTranslator, I18nContext } from "./i18n";

const mocks = vi.hoisted(() => {
  const state: { view: AccountView | null; unavailable: boolean } = {
    view: null,
    unavailable: false,
  };
  return state;
});

vi.mock("./useAccount", () => ({
  useAccount: () => ({
    view: mocks.view,
    unavailable: mocks.unavailable,
    projects: null,
    projectsError: null,
    loadingProjects: false,
    avatar: null,
  }),
  account: {
    signIn: () => Promise.resolve(null),
    cancelSignIn: () => Promise.resolve(null),
    signOut: () => Promise.resolve(null),
    setBaseUrl: () => Promise.resolve(null),
    open: () => Promise.resolve(null),
    checkServer: () => Promise.resolve({ problem: "unavailable" }),
  },
}));

import { AccountSettings } from "./AccountSettings";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

const OFFICIAL = "https://hangar.example.com/api/v1";
const OWN = "https://team.example.org/api/v1";

function view(state: AccountState, baseUrl = OFFICIAL): AccountView {
  return {
    baseUrl,
    official: baseUrl === OFFICIAL,
    devicesUrl: "https://hangar.example.com/devices",
    state,
    otherSignIns: [],
    revision: 1,
  };
}

const IDLE: AccountState = { kind: "idle", notice: null };
const WAITING: AccountState = {
  kind: "waitingForUser",
  userCode: "ABCD-EFGH",
  verificationUri: "https://hangar.example.com/device",
  verificationUriComplete: "https://hangar.example.com/device?code=ABCD-EFGH",
  expiresAt: Date.now() + 600_000,
  interval: 5,
};

beforeEach(() => {
  mocks.view = null;
  mocks.unavailable = false;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
});

function render(next: AccountView | null) {
  mocks.view = next;
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <AccountSettings />
      </I18nContext.Provider>,
    );
  });
}

const button = (text: string) =>
  [...host.querySelectorAll("button")].find((element) => element.textContent === text);

const serverToggle = () =>
  [...host.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")].find((element) =>
    element.textContent.includes(t("account.server")),
  );

describe("AccountSettings", () => {
  it("shows only the intro while the first status loads", () => {
    render(null);
    expect(host.textContent).toBe(t("account.intro"));
    expect(host.querySelector(".setting-card")).toBeNull();
  });

  it("says so when the account cannot be reached", () => {
    mocks.unavailable = true;
    render(null);
    expect(host.textContent).toContain(t("account.unavailable"));
  });

  it("puts sign-in and server address in one card below the intro", () => {
    render(view(IDLE));
    expect(host.querySelector(".setting-group-desc")?.textContent).toBe(t("account.intro"));
    const card = host.querySelector(".setting-card");
    expect(card?.contains(button(t("account.signIn")) ?? null)).toBe(true);
    expect(card?.contains(serverToggle() ?? null)).toBe(true);
  });

  it("folds the server address away for the official service, naming it", () => {
    render(view(IDLE));
    const toggle = serverToggle();
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(toggle?.textContent).toContain(t("account.serverOfficial"));
    expect(host.querySelector("#account-server")).toBeNull();
    act(() => {
      toggle?.click();
    });
    expect(host.querySelector("#account-server")).not.toBeNull();
  });

  it("opens the server address for an own server, with its test", () => {
    render(view(IDLE, OWN));
    const toggle = serverToggle();
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(toggle?.textContent).toContain(t("account.serverName", { host: "team.example.org" }));
    expect(host.querySelector<HTMLInputElement>("#account-server")?.value).toBe(OWN);
    expect(button(t("account.check"))).toBeDefined();
  });

  it("names an own server once, on its line, not again next to sign in", () => {
    render(view(IDLE, OWN));
    const name = t("account.serverName", { host: "team.example.org" });
    const state = host.querySelector(".account-state");
    expect(state?.textContent).not.toContain(name);
    expect(serverToggle()?.textContent).toContain(name);
  });

  it("moves the focus to the code's copy button when the sign-in starts", () => {
    render(view(IDLE));
    button(t("account.signIn"))?.focus();
    expect(document.activeElement?.textContent).toBe(t("account.signIn"));
    render(view(WAITING));
    expect(document.activeElement).toBe(button(t("account.copyCode")));
  });

  it("leaves the focus in the server field when the state changes", () => {
    render(view(IDLE));
    act(() => {
      serverToggle()?.click();
    });
    const field = host.querySelector<HTMLInputElement>("#account-server");
    field?.focus();
    render(view(WAITING));
    expect(document.activeElement).toBe(field);
  });

  it("names the Free plan instead of counting a single seat, with a way to the plans", () => {
    const user = {
      id: "u1",
      email: "ada@example.com",
      name: "Ada",
      seatsAccountTotal: 1,
      seatsAccountUsed: 1,
      avatarVersion: null,
    };
    render(view({ kind: "signedIn", user }));
    expect(host.textContent).toContain(t("account.freePlan"));
    expect(button(t("teamPlan.seePlans"))).toBeDefined();
    expect(host.textContent).not.toContain(t("team.seats", { used: 1, total: 1 }));

    render(
      view({ kind: "signedIn", user: { ...user, seatsAccountTotal: 5, seatsAccountUsed: 3 } }),
    );
    expect(host.textContent).not.toContain(t("account.freePlan"));
    expect(host.textContent).toContain(
      t("account.ownSeats", { seats: t("team.seats", { used: 3, total: 5 }) }),
    );
  });
});
