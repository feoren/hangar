import { describe, expect, it } from "vitest";

import { pickProgram } from "./launch";

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
