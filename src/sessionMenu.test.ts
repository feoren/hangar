import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { sessionMenuEntries } from "./sessionMenu";

const t = createTranslator("en");

describe("sessionMenuEntries", () => {
  it("offers end process for a running session", () => {
    const entries = sessionMenuEntries({ status: "running" }, t);
    expect(entries.map((entry) => entry.key)).toEqual(["rename", "marker", "sep", "kill", "close"]);
    const close = entries.at(-1);
    expect(close?.type === "action" ? close.label : "").toBe(t("session.killAndClose"));
  });

  it("offers restart for an ended session", () => {
    const entries = sessionMenuEntries({ status: "exited" }, t);
    expect(entries.map((entry) => entry.key)).toEqual([
      "rename",
      "marker",
      "sep",
      "restart",
      "close",
    ]);
    const close = entries.at(-1);
    expect(close?.type === "action" ? close.label : "").toBe(t("session.close"));
  });

  it("offers the sub-agents only for sessions that have them", () => {
    const keys = (subAgents: boolean | undefined, shown = false) =>
      sessionMenuEntries(
        subAgents === undefined ? { status: "running" } : { status: "running", subAgents },
        t,
        shown,
      ).map((entry) => entry.key);
    expect(keys(undefined)).not.toContain("subAgents");
    expect(keys(false)).not.toContain("subAgents");
    expect(keys(true)).toEqual([
      "rename",
      "marker",
      "sep-sub-agents",
      "subAgents",
      "sep",
      "kill",
      "close",
    ]);
    const label = (shown: boolean) => {
      const entry = sessionMenuEntries({ status: "running", subAgents: true }, t, shown).find(
        (item) => item.key === "subAgents",
      );
      return entry?.type === "action" ? entry.label : "";
    };
    expect(label(false)).toBe(t("session.showSubAgents"));
    expect(label(true)).toBe(t("session.hideSubAgents"));
    // An open view can always be closed, even after the session lost its sub-agents.
    expect(keys(false, true)).toContain("subAgents");
  });
});
