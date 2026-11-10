import { describe, expect, it } from "vitest";

import { accountMenuEntries } from "./accountMenu";

describe("account menu", () => {
  it("offers the account, the three color modes and signing out while signed in", () => {
    const entries = accountMenuEntries(true, "dark");
    expect(entries.map((entry) => entry.kind)).toEqual([
      "account",
      "separator",
      "themeHeading",
      "theme",
      "theme",
      "theme",
      "separator",
      "signOut",
    ]);
  });

  it("offers signing in and the color modes while signed out", () => {
    const kinds = accountMenuEntries(false, "system").map((entry) => entry.kind);
    expect(kinds[0]).toBe("signIn");
    expect(kinds).not.toContain("signOut");
    expect(kinds).not.toContain("account");
  });

  it("marks exactly the chosen color mode, System included", () => {
    const marked = (setting: "system" | "light" | "dark") =>
      accountMenuEntries(true, setting).flatMap((entry) =>
        entry.kind === "theme" && entry.current ? [entry.setting] : [],
      );
    expect(marked("system")).toEqual(["system"]);
    expect(marked("light")).toEqual(["light"]);
    expect(marked("dark")).toEqual(["dark"]);
  });
});
