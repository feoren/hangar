import { describe, expect, it } from "vitest";

import {
  LAST_PROGRAM_STORAGE_KEY,
  legacyLastProgram,
  parseLastProgram,
  pickProgram,
} from "./launch";

const programs = [{ id: "pwsh" }, { id: "claude" }, { id: "codex" }, { id: "aider" }];

describe("pickProgram", () => {
  it("takes the most recently used program", () => {
    expect(pickProgram(programs, "codex")?.id).toBe("codex");
  });

  it("falls back to the first when the last one is missing or unknown", () => {
    expect(pickProgram(programs, null)?.id).toBe("pwsh");
    expect(pickProgram(programs, "gemini")?.id).toBe("pwsh");
    const none: { id: string }[] = [];
    expect(pickProgram(none, "pwsh")).toBeUndefined();
  });
});

describe("the last program an older version stored", () => {
  it("is read once for the migration, checked like the backend checks it", () => {
    const store = new Map<string, string>();
    const storage = { getItem: (key: string) => store.get(key) ?? null };
    expect(LAST_PROGRAM_STORAGE_KEY).toBe("hangar.lastProgram");
    expect(legacyLastProgram(storage)).toBeNull();
    store.set(LAST_PROGRAM_STORAGE_KEY, "claude");
    expect(legacyLastProgram(storage)).toBe("claude");
    for (const bad of ["", "  ", "a\u0007b", "p".repeat(201)]) {
      store.set(LAST_PROGRAM_STORAGE_KEY, bad);
      expect(legacyLastProgram(storage), bad).toBeNull();
    }
    expect(parseLastProgram("p".repeat(200))).toBe("p".repeat(200));
    expect(parseLastProgram(42)).toBeNull();
    const blocked = {
      getItem: (): string | null => {
        throw new Error("blocked");
      },
    };
    expect(legacyLastProgram(blocked)).toBeNull();
  });
});
