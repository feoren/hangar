// @vitest-environment happy-dom

// The Team view's decisions: one "Record decision" form with two fields that keeps its text when
// the team server refuses, the list with Withdraw and "Send to session", and on an older team
// server a quiet line for owners and admins only.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const teamRecordDecision = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const teamWithdrawDecision = vi.hoisted(() => vi.fn<() => Promise<unknown>>());

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, teamRecordDecision, teamWithdrawDecision } };
});

import { createTranslator, I18nContext } from "./i18n";
import { EMPTY_ACTIVITY, type DecisionView, type TeamActivity } from "./team";
import { TeamDecisionsView, type DecisionTarget } from "./TeamDecisionsView";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

const DECISION: DecisionView = {
  blob: "blob-7",
  id: "0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b",
  decision: "Use marker A",
  reason: "Marker B",
  member: "u_ada",
  name: "Ada",
  at: 1_700_000_000_000,
  mine: false,
  former: false,
  canWithdraw: true,
};

beforeEach(() => {
  teamRecordDecision.mockResolvedValue(null);
  teamWithdrawDecision.mockResolvedValue(null);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.clearAllMocks();
});

function render(
  activity: Partial<TeamActivity>,
  options: { manages?: boolean; target?: DecisionTarget | null; onSend?: () => void } = {},
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <TeamDecisionsView
          projectId="p1"
          activity={{ ...EMPTY_ACTIVITY, ...activity }}
          busy={new Set()}
          manages={options.manages ?? false}
          target={options.target ?? null}
          onSend={options.onSend ?? (() => undefined)}
        />
      </I18nContext.Provider>,
    );
  });
}

const button = (text: string) =>
  [...host.querySelectorAll("button")].find((element) => element.textContent === text);

function type(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype =
    element instanceof HTMLInputElement
      ? HTMLInputElement.prototype
      : HTMLTextAreaElement.prototype;
  act(() => {
    // The prototype's setter, so React's value tracker sees a change.
    Reflect.set(prototype, "value", value, element);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("TeamDecisionsView", () => {
  it("shows nothing until the team server is known and a quiet line on an old one for managers", () => {
    render({ decisionsState: "unknown" });
    expect(host.textContent).toBe("");
    render({ decisionsState: "unsupported" });
    expect(host.textContent).toBe("");
    render({ decisionsState: "unsupported" }, { manages: true });
    expect(host.textContent).toBe(t("team.decisions.needsServer"));
    expect(button(t("team.decisions.record"))).toBeUndefined();
  });

  it("says calmly when decisions start while this device has no key of the team", () => {
    render({ decisionsState: "waitingForKeys" });
    expect(host.textContent).toBe(t("team.decisions.waitingForKeys"));
    expect(button(t("team.decisions.record"))).toBeUndefined();
  });

  it("lists decisions with who and when, and withdraws one after asking", async () => {
    render({ decisionsState: "ready", decisions: [DECISION] });
    expect(host.textContent).toContain("Use marker A");
    expect(host.textContent).toContain("Marker B");
    expect(host.textContent).toContain("Ada #u_ada");
    expect(button(t("team.decisions.send"))).toBeUndefined();
    // Withdraw asks first; the focus goes to Cancel, Escape cancels.
    act(() => {
      button(t("team.decisions.withdraw"))?.click();
    });
    expect(host.textContent).toContain(t("team.decisions.withdrawConfirm"));
    expect(document.activeElement?.textContent).toBe(t("common.cancel"));
    expect(teamWithdrawDecision).not.toHaveBeenCalled();
    act(() => {
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(host.textContent).not.toContain(t("team.decisions.withdrawConfirm"));
    expect(document.activeElement?.textContent).toBe(t("team.decisions.withdraw"));
    // Asked again and confirmed: exactly this blob goes.
    act(() => {
      button(t("team.decisions.withdraw"))?.click();
    });
    const confirm = [...host.querySelectorAll(".team-confirm-ask button")].find(
      (element) => element.textContent === t("team.decisions.withdraw"),
    );
    await act(async () => {
      if (confirm instanceof HTMLButtonElement) confirm.click();
      await Promise.resolve();
    });
    expect(teamWithdrawDecision).toHaveBeenCalledWith("p1", DECISION.blob);
    // Without the right, no Withdraw.
    render({ decisionsState: "ready", decisions: [{ ...DECISION, canWithdraw: false }] });
    expect(button(t("team.decisions.withdraw"))).toBeUndefined();
  });

  it("names one person's limit honestly and keeps the text", async () => {
    teamRecordDecision.mockRejectedValue({ kind: "decisionLimitOwn", detail: null });
    render({ decisionsState: "ready" });
    act(() => {
      button(t("team.decisions.record"))?.click();
    });
    const what = host.querySelector<HTMLInputElement>("input");
    if (what === null) throw new Error("no form");
    type(what, "Use marker A");
    await act(async () => {
      button(t("team.decisions.confirm"))?.click();
      await Promise.resolve();
      await Promise.resolve();
    });
    const alert = host.querySelector('[role="alert"]')?.textContent;
    expect(alert).toBe(t("team.error.decisionLimitOwn"));
    expect(alert).not.toBe(t("team.error.decisionLimit"));
    expect(createTranslator("de")("team.error.decisionLimitOwn")).toContain("deiner eigenen");
    expect(what.value).toBe("Use marker A");
  });

  it("marks a former member's decision", () => {
    render({ decisionsState: "ready", decisions: [{ ...DECISION, former: true }] });
    expect(host.querySelector(".team-task-meta")?.textContent).toContain(
      t("team.decisions.former"),
    );
  });

  it("shows a reason's line breaks as lines, and only other invisible characters as marks", () => {
    render({
      decisionsState: "ready",
      decisions: [{ ...DECISION, reason: "Line one\nLine two‮" }],
    });
    const reason = host.querySelector(".team-decision-reason-text");
    expect(reason?.textContent).toBe("Line oneLine two<U+202E>");
    expect(reason?.querySelectorAll("br")).toHaveLength(1);
    expect(host.textContent).not.toContain("<U+000A>");
  });

  it("sends a decision to a session whose agent does not read it by itself", () => {
    const onSend = vi.fn();
    render(
      { decisionsState: "ready", decisions: [DECISION] },
      { target: { id: 3, title: "Codex" }, onSend },
    );
    act(() => {
      button(t("team.decisions.send"))?.click();
    });
    expect(onSend).toHaveBeenCalledWith("Use marker A\nMarker B", 3);
  });

  it("records with two fields and keeps the text when the team keeps as many as it can", async () => {
    teamRecordDecision.mockRejectedValue({ kind: "decisionLimit", detail: null });
    render({ decisionsState: "ready" });
    expect(host.textContent).toContain(t("team.decisions.none"));
    act(() => {
      button(t("team.decisions.record"))?.click();
    });
    const what = host.querySelector<HTMLInputElement>("input");
    const why = host.querySelector<HTMLTextAreaElement>("textarea");
    if (what === null || why === null) throw new Error("no form");
    expect(what.maxLength).toBe(300);
    expect(why.maxLength).toBe(1000);
    type(what, "Use marker A");
    type(why, "Marker B");
    await act(async () => {
      button(t("team.decisions.confirm"))?.click();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(teamRecordDecision).toHaveBeenCalledWith("p1", "Use marker A", "Marker B");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(t("team.error.decisionLimit"));
    expect(what.value).toBe("Use marker A");
    expect(why.value).toBe("Marker B");
  });
});
