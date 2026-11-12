import { describe, expect, it } from "vitest";

import { LAST_PROMPT_MAX, promptWorthShowing } from "./lastPrompt";

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
