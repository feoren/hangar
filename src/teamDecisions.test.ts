import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  EMPTY_ACTIVITY,
  decisionText,
  parseTeamActivity,
  parseTeamError,
  readsTeamContext,
  teamErrorText,
} from "./team";

const DECISION = {
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

describe("team decisions", () => {
  it("reads the decisions and their state, absent as none from an older backend", () => {
    const activity = parseTeamActivity({
      ...EMPTY_ACTIVITY,
      decisions: [DECISION],
      decisionsState: "ready",
    });
    expect(activity?.decisions).toEqual([DECISION]);
    expect(activity?.decisionsState).toBe("ready");
    const older = parseTeamActivity({
      ...EMPTY_ACTIVITY,
      decisions: undefined,
      decisionsState: undefined,
    });
    expect(older?.decisions).toEqual([]);
    expect(older?.decisionsState).toBe("unknown");
    expect(parseTeamActivity({ ...EMPTY_ACTIVITY, decisionsState: "maybe" })).toBeNull();
    expect(
      parseTeamActivity({ ...EMPTY_ACTIVITY, decisionsState: "waitingForKeys" })?.decisionsState,
    ).toBe("waitingForKeys");
    // A decision without its blob cannot be withdrawn by it: refused.
    expect(
      parseTeamActivity({ ...EMPTY_ACTIVITY, decisions: [{ ...DECISION, blob: "" }] }),
    ).toBeNull();
    expect(
      parseTeamActivity({ ...EMPTY_ACTIVITY, decisions: [{ ...DECISION, former: "no" }] }),
    ).toBeNull();
    expect(
      parseTeamActivity({ ...EMPTY_ACTIVITY, decisions: [{ ...DECISION, at: "today" }] }),
    ).toBeNull();
    expect(
      parseTeamActivity({ ...EMPTY_ACTIVITY, decisions: [{ ...DECISION, decision: "" }] }),
    ).toBeNull();
  });

  it("names the limit so the form can keep its text", () => {
    const t = createTranslator("de");
    const problem = parseTeamError({ kind: "decisionLimit", detail: null });
    expect(problem.kind).toBe("decisionLimit");
    expect(teamErrorText(problem, t)).toBe(t("team.error.decisionLimit"));
    for (const kind of [
      "invalidDecision",
      "decisionsUnsupported",
      "decisionsNotReady",
      "unknownDecision",
      "notOwnerOrAdmin",
    ]) {
      expect(parseTeamError({ kind, detail: null }).kind).toBe(kind);
    }
  });

  it("offers sending only where the agent does not read the team's context itself", () => {
    const injects = { contextInject: true };
    expect(readsTeamContext({ mode: "team", gate: "active" }, injects)).toBe(true);
    expect(readsTeamContext({ mode: "private", gate: "none" }, injects)).toBe(false);
    expect(readsTeamContext({ mode: "team", gate: "launcher" }, injects)).toBe(false);
    expect(readsTeamContext({ mode: "team", gate: "active" }, { contextInject: false })).toBe(
      false,
    );
    expect(readsTeamContext({ mode: "team", gate: "active" }, undefined)).toBe(false);
  });

  it("pastes the decision with its reason on the next line", () => {
    expect(decisionText({ decision: "Marker A", reason: " Marker B " })).toBe("Marker A\nMarker B");
    expect(decisionText({ decision: "Marker A", reason: "" })).toBe("Marker A");
  });
});
