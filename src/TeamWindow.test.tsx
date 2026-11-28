// @vitest-environment happy-dom

// The Team window resizes like the Settings dialog: grips on its edges, the size from the
// shared hook and remembered under its own key. On Free without a team it explains the Team plan
// instead of offering projects (teamPlan.ts). Connected, it opens on who is here and what each
// person works on, without tabs; answers this device owes sit on top under "Waiting for you", and
// the rarely used parts sit behind "Manage". The account and team stores are stubbed; without an
// account view the window renders its frame without a body.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const state: {
    view: AccountView | null;
    projects: Project[] | null;
    statuses: Map<string, TeamStatus>;
    activities: Map<string, TeamActivity>;
    pending: PendingConfirmation[];
  } = {
    view: null,
    projects: null,
    statuses: new Map(),
    activities: new Map(),
    pending: [],
  };
  return state;
});

vi.mock("./useAccount", () => ({
  loadProjects: vi.fn(),
  account: { open: vi.fn(() => Promise.resolve(null)) },
  useAccount: () => ({
    projects: mocks.projects,
    projectsError: null,
    loadingProjects: false,
    view: mocks.view,
    unavailable: false,
  }),
}));
vi.mock("./useTeam", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./useTeam")>();
  return {
    ...actual,
    useTeam: () => ({
      activities: mocks.activities,
      pending: mocks.pending,
      statuses: mocks.statuses,
      busy: new Set(),
      repos: [],
    }),
  };
});

import type { AccountState, AccountView, Me, Project } from "./account";
import { createTranslator, I18nContext } from "./i18n";
import {
  EMPTY_ACTIVITY,
  type PeerView,
  type PendingConfirmation,
  type TaskView,
  type TeamActivity,
  type TeamStatus,
} from "./team";
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
  mocks.statuses = new Map();
  mocks.activities = new Map();
  mocks.pending = [];
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

function mount(onClose: () => void = () => undefined) {
  act(() => root?.unmount());
  root = createRoot(host);
  act(() => {
    root?.render(
      <I18nContext.Provider value={t}>
        <TeamWindow
          sessions={[]}
          decisionTarget={null}
          onSendDecision={() => undefined}
          onClose={onClose}
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
    // No project to connect and no team controls: nothing is offered the plan does not include.
    expect(panel.textContent).not.toContain(t("teamWindow.connectIntro"));
    expect(panel.textContent).not.toContain(t("teamWindow.manage"));
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

  describe("with a connected team", () => {
    const TEAM_USER: Me = { ...FREE_USER, seatsAccountTotal: 5 };
    const ADA: PeerView = {
      member: "u_ada",
      device: "d_ada",
      name: "Ada",
      own: false,
      presence: "active",
      sessions: [
        {
          key: "s1",
          agent: "Claude Code",
          title: "Login",
          state: "working",
          repo: "r",
          files: [],
          moreFiles: 0,
        },
      ],
    };
    const LOGIN: TaskView = {
      task: "t1",
      title: "Fix the login",
      area: null,
      member: "u_ada",
      device: "d_ada",
      name: "Ada",
      mine: false,
      own: false,
      since: 0,
      expiresAt: null,
      expiring: false,
      request: null,
    };
    const REQUEST: PendingConfirmation = {
      id: "r1",
      projectId: "p1",
      member: "u_ben",
      device: "d_ben",
      kind: "newMember",
      safetyNumber: ["11111", "22222", "33333", "44444", "55555", "66666"],
      readmitted: false,
      logFull: false,
    };

    function connect(activity: Partial<TeamActivity> = {}) {
      mocks.view = view({ kind: "signedIn", user: TEAM_USER });
      mocks.projects = [project({ plan: "team", seatsAccountTotal: 5 })];
      mocks.statuses = new Map([["p1", { state: "connected", epoch: 1, members: 2, devices: 2 }]]);
      mocks.activities = new Map([["p1", { ...EMPTY_ACTIVITY, ...activity }]]);
    }

    const buttonNamed = (panel: HTMLElement, text: string) =>
      [...panel.querySelectorAll("button")].find((button) => button.textContent === text);

    function press(panel: HTMLElement, key: string) {
      const target = document.activeElement instanceof HTMLElement ? document.activeElement : panel;
      act(() => {
        target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      });
    }

    it("opens on who is here and what each person works on, without tabs", () => {
      connect({ peers: [ADA], tasks: [LOGIN] });
      const panel = mount();
      expect(panel.querySelector('[role="tablist"]')).toBeNull();
      expect(panel.textContent).toContain(t("team.activity.people"));
      expect(panel.textContent).toContain("Ada");
      expect(panel.textContent).toContain(t("team.activity.workingOn", { title: "Fix the login" }));
      expect(panel.querySelector(".presence-dot.online")).not.toBeNull();
      // The rarely used parts wait behind "Manage".
      expect(panel.textContent).not.toContain(t("team.share"));
      expect(buttonNamed(panel, t("teamWindow.manage"))).toBeDefined();
    });

    it("offers to invite people while nobody else is here", () => {
      connect();
      const panel = mount();
      expect(panel.textContent).toContain(t("team.activity.nobody"));
      expect(buttonNamed(panel, t("team.invite"))).toBeDefined();
    });

    it("shows sharing behind Manage and steps back on Escape before closing", () => {
      connect({ peers: [ADA] });
      const onClose = vi.fn();
      const panel = mount(onClose);
      act(() => buttonNamed(panel, t("teamWindow.manage"))?.click());
      expect(panel.textContent).toContain(t("team.share"));
      expect(panel.textContent).not.toContain(t("team.activity.people"));
      const back = buttonNamed(panel, t("teamWindow.back"));
      expect(back).toBeDefined();
      back?.focus();

      press(panel, "Escape");
      expect(onClose).not.toHaveBeenCalled();
      expect(panel.textContent).toContain(t("team.activity.people"));
      expect(document.activeElement?.textContent).toBe(t("teamWindow.manage"));

      press(panel, "Escape");
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it("puts nothing on top while nothing waits for an answer", () => {
      connect({ peers: [ADA] });
      const panel = mount();
      expect(panel.querySelector(".team-attention")).toBeNull();
      expect(panel.textContent).not.toContain(t("teamWindow.waitingForYou"));
    });

    it("puts a request on top under Waiting for you, with the team below it", () => {
      connect({ peers: [ADA] });
      mocks.pending = [REQUEST];
      const panel = mount();
      const body = panel.querySelector(".team-window-body");
      const waiting = body?.firstElementChild;
      expect(waiting?.classList.contains("team-attention")).toBe(true);
      expect(waiting?.textContent).toContain(t("teamWindow.waitingForYou"));
      expect(waiting?.textContent).toContain(t("team.confirm.title"));
      expect(body?.textContent).toContain(t("team.activity.people"));
    });

    it("keeps a request on top under Manage too", () => {
      connect({ peers: [ADA] });
      mocks.pending = [REQUEST];
      const panel = mount();
      act(() => buttonNamed(panel, t("teamWindow.manage"))?.click());
      expect(panel.querySelector(".team-window-body")?.firstElementChild?.textContent).toContain(
        t("team.confirm.title"),
      );
    });
  });
});
