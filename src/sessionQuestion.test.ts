import { describe, expect, it } from "vitest";

import { askFor, stillAsked } from "./sessionQuestion";

describe("a question about one session's folder", () => {
  it("is asked only with a session and a folder", () => {
    expect(askFor("app.ts", 1, "C:\\repo")).toEqual({
      item: "app.ts",
      sessionId: 1,
      cwd: "C:\\repo",
    });
    expect(askFor("app.ts", null, "C:\\repo")).toBeNull();
    expect(askFor("app.ts", 1, null)).toBeNull();
  });

  it("stays while the panel shows the same session and folder", () => {
    const question = askFor("app.ts", 1, "C:\\repo");
    expect(stillAsked(question, 1, "C:\\repo")).toBe(question);
  });

  it("is gone after a switch, so its answer never acts on the other session", () => {
    // Discard opened with session 1, then the panel shows session 2.
    const discards: number[] = [];
    const confirm = (shown: ReturnType<typeof stillAsked<string>>) => {
      if (shown !== null) discards.push(shown.sessionId);
    };
    const question = askFor("app.ts", 1, "C:\\repo");
    const afterSwitch = stillAsked(question, 2, "C:\\repo");
    expect(afterSwitch).toBeNull();
    confirm(afterSwitch);
    expect(discards).toEqual([]);
    // Another folder of the same session counts as a switch too.
    expect(stillAsked(question, 1, "C:\\other")).toBeNull();
    // Confirmed before the switch, it acts on the session it was asked for.
    confirm(stillAsked(question, 1, "C:\\repo"));
    expect(discards).toEqual([1]);
    expect(stillAsked(null, 1, "C:\\repo")).toBeNull();
  });
});
