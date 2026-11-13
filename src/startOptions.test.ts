import { describe, expect, it } from "vitest";

import { instructionsUnavailable, startOptions } from "./startOptions";

describe("instructionsUnavailable", () => {
  it("names installed programs behind a launcher only", () => {
    expect(
      instructionsUnavailable([
        { name: "Codex CLI", installed: true, instructions: "launcher" },
        { name: "Claude Code", installed: true, instructions: "yes" },
        { name: "Old Codex", installed: false, instructions: "launcher" },
        { name: "PowerShell", installed: true, instructions: "none" },
        { name: "Older backend", installed: true },
      ]),
    ).toEqual(["Codex CLI"]);
    expect(instructionsUnavailable([])).toEqual([]);
  });
});

describe("startOptions", () => {
  const on = { hangarMd: true, worktreeUpstreamCheck: true };
  const off = { hangarMd: false, worktreeUpstreamCheck: false };

  it("asks for the upstream check only in a fresh worktree", () => {
    expect(startOptions(on, true)).toEqual({ instructions: true, startPrompt: "worktree" });
    expect(startOptions(on, false)).toEqual({ instructions: true, startPrompt: null });
  });

  it("follows both settings", () => {
    expect(startOptions(off, true)).toEqual({ instructions: false, startPrompt: null });
    expect(startOptions({ hangarMd: false, worktreeUpstreamCheck: true }, true)).toEqual({
      instructions: false,
      startPrompt: "worktree",
    });
  });
});
