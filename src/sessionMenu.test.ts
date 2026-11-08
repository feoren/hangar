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
});
