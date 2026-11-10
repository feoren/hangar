import { describe, expect, it } from "vitest";

import { accountMenuEntries } from "./accountMenu";

describe("account menu", () => {
  it("offers the notes, the three color modes and signing out while signed in", () => {
    const entries = accountMenuEntries(true, "dark", false);
    expect(entries.map((entry) => entry.kind)).toEqual([
      "notes",
      "separator",
      "themeHeading",
      "theme",
      "theme",
      "theme",
      "separator",
      "signOut",
    ]);
  });

  it("offers signing in, the notes and the color modes while signed out", () => {
    const kinds = accountMenuEntries(false, "system", false).map((entry) => entry.kind);
    expect(kinds.slice(0, 3)).toEqual(["signIn", "separator", "notes"]);
    expect(kinds).not.toContain("signOut");
  });

  it("no longer links to the account settings; the gear opens them", () => {
    for (const signedIn of [true, false]) {
      const kinds: string[] = accountMenuEntries(signedIn, "dark", false).map((e) => e.kind);
      expect(kinds).not.toContain("account");
    }
  });

  it("tells whether the notes entry shows or hides the panel", () => {
    const notes = (open: boolean) =>
      accountMenuEntries(true, "dark", open).flatMap((entry) =>
        entry.kind === "notes" ? [entry.open] : [],
      );
    expect(notes(false)).toEqual([false]);
    expect(notes(true)).toEqual([true]);
  });

  it("marks exactly the chosen color mode, System included", () => {
    const marked = (setting: "system" | "light" | "dark") =>
      accountMenuEntries(true, setting, false).flatMap((entry) =>
        entry.kind === "theme" && entry.current ? [entry.setting] : [],
      );
    expect(marked("system")).toEqual(["system"]);
    expect(marked("light")).toEqual(["light"]);
    expect(marked("dark")).toEqual(["dark"]);
  });
});
