// @vitest-environment happy-dom

// The Team window resizes like the Settings dialog: grips on its edges, the size from the
// shared hook and remembered under its own key. On Free without a team it explains the Team plan
// instead of offering projects (teamPlan.ts). The account and team stores are stubbed; without an
// account view the window renders its frame without a body.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const state: { view: AccountView | null; projects: Project[] | null } = {
    view: null,
    projects: null,
  };
  return state;
});

vi.mock("./useAccount", () => ({
  loadProjects: vi.fn(),
  useAccount: () => ({
    projects: mocks.projects,
    projectsError: null,
    loadingProjects: false,
    view: mocks.view,
    unavailable: false,
  }),
}));
vi.mock("./useTeam", () => ({
  useTeam: () => ({ activities: new Map(), pending: [], statuses: new Map(), busy: new Set() }),
}));

import type { AccountState, AccountView, Me, Project } from "./account";
import { createTranslator, I18nContext } from "./i18n";
import { TeamWindow } from "./TeamWindow";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

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

const FREE_USER: Me = {
  id: "u1",
  email: "ada@example.com",
  name: "Ada",
  seatsAccountTotal: 1,
  seatsAccountUsed: 1,
  avatarVersion: null,
};

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

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  mocks.view = null;
  mocks.projects = null;
  Object.defineProperty(window, "innerWidth", { value: 1400, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: 900, configurable: true });
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  host.remove();
});

function mount() {
  root = createRoot(host);
  act(() => {
    root?.render(
      <I18nContext.Provider value={t}>
        <TeamWindow
          sessions={[]}
          decisionTarget={null}
          onSendDecision={() => undefined}
          onClose={() => undefined}
        />
      </I18nContext.Provider>,
    );
  });
  const panel = host.querySelector<HTMLElement>(".team-window");
  if (panel === null) throw new Error("no team window");
  return panel;
}

describe("TeamWindow", () => {
  it("has resize grips and opens at the standard size", () => {
    const panel = mount();
    expect(panel.classList.contains("resizable")).toBe(true);
    expect(panel.querySelectorAll(".dialog-resize").length).toBeGreaterThan(0);
    expect(panel.style.width).toBe("936px");
    expect(panel.style.height).toBe("672px");
  });

  it("opens at the size the user chose last time", () => {
    localStorage.setItem("hangar.teamWindowSize", JSON.stringify({ width: 700, height: 500 }));
    const panel = mount();
    expect(panel.style.width).toBe("700px");
    expect(panel.style.height).toBe("500px");
  });

  it("explains the Team plan on Free without a team, with a way to the plans", () => {
    mocks.view = view({ kind: "signedIn", user: FREE_USER });
    mocks.projects = [project()];
    const panel = mount();
    expect(panel.textContent).toContain(t("teamPlan.text"));
    expect(panel.textContent).toContain(t("teamPlan.seePlans"));
    expect(panel.textContent).toContain(t("team.reload"));
    // No project to connect and no tabs: nothing is offered that the plan does not include.
    expect(panel.textContent).not.toContain(t("teamWindow.connectIntro"));
    expect(panel.querySelector('[role="tablist"]')).toBeNull();
  });

  it("names the Team plan below the sign-in when signed out", () => {
    mocks.view = view({ kind: "idle", notice: null });
    const panel = mount();
    expect(panel.textContent).toContain(t("teamWindow.signInIntro"));
    expect(panel.textContent).toContain(t("account.signIn"));
    expect(panel.textContent).toContain(t("teamPlan.text"));
  });

  it("keeps the team for a member of a grandfathered Free team", () => {
    mocks.view = view({ kind: "signedIn", user: FREE_USER });
    mocks.projects = [project({ role: "member", canInvite: false, seatsAccountUsed: 3 })];
    const panel = mount();
    expect(panel.textContent).not.toContain(t("teamPlan.text"));
    expect(panel.textContent).toContain(t("teamWindow.connectIntro"));
  });
});
