import { describe, expect, it } from "vitest";

import { LAST_PROMPT_MAX, needsLastPromptBar, promptWorthShowing } from "./lastPrompt";

describe("promptWorthShowing", () => {
  it("keeps real prompts with their whitespace collapsed", () => {
    expect(promptWorthShowing("  fix   the login\tform ")).toBe("fix the login form");
    expect(promptWorthShowing("ok")).toBe("ok");
  });

  it("skips empty lines, one-character answers and slash commands", () => {
    expect(promptWorthShowing("")).toBeNull();
    expect(promptWorthShowing("   ")).toBeNull();
    expect(promptWorthShowing("1")).toBeNull();
    expect(promptWorthShowing("y")).toBeNull();
    expect(promptWorthShowing("/clear")).toBeNull();
  });

  it("caps very long prompts", () => {
    const text = promptWorthShowing("a".repeat(LAST_PROMPT_MAX + 50));
    expect(text).toHaveLength(LAST_PROMPT_MAX);
    expect(text?.endsWith("…")).toBe(true);
  });
});

describe("needsLastPromptBar", () => {
  // Claude Code pins the last prompt above its conversation itself; a second line would repeat it.
  it("leaves the bar out for Claude Code and keeps it for other programs", () => {
    expect(needsLastPromptBar("claude")).toBe(false);
    expect(needsLastPromptBar("codex")).toBe(true);
    expect(needsLastPromptBar("gemini")).toBe(true);
  });
});
