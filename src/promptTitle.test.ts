import { describe, expect, it } from "vitest";

import {
  PROMPT_TITLE_MAX,
  PromptLineTracker,
  isAutoTitle,
  promptTitle,
  promptVerdict,
  type PromptSession,
} from "./promptTitle";

describe("PromptLineTracker", () => {
  it("returns the typed line on Enter, across chunks", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("fix the ")).toEqual([]);
    expect(tracker.feed("login")).toEqual([]);
    expect(tracker.feed("\r")).toEqual(["fix the login"]);
    expect(tracker.feed("next\r")).toEqual(["next"]);
  });

  it("applies Backspace, Ctrl+W, Ctrl+U and Ctrl+C", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("helo\x7f\x7fllo\r")).toEqual(["hello"]);
    expect(tracker.feed("one two \x17three\r")).toEqual(["one three"]);
    expect(tracker.feed("gone\x15kept\r")).toEqual(["kept"]);
    expect(tracker.feed("gone\x03kept\r")).toEqual(["kept"]);
  });

  it("skips arrows, focus reports and cursor position replies", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("\x1b[12;1R\x1b[Iab\x1b[D\x1bOAcd\r")).toEqual(["abcd"]);
  });

  it("keeps a split escape sequence out of the line", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("a\x1b[");
    tracker.feed("1;");
    expect(tracker.feed("5Cb\r")).toEqual(["ab"]);
  });

  it("takes a lone Escape key without losing the next character", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("\x1b");
    expect(tracker.feed("fix\r")).toEqual(["fix"]);
  });

  it("takes bracketed paste as text with its line breaks as spaces", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("see \x1b[200~first\r\nsec");
    expect(tracker.feed("ond\x1b[201~ now\r")).toEqual(["see first  second now"]);
  });

  it("treats Alt+Enter as a line break inside the prompt", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("one\x1b\rtwo\r")).toEqual(["one two"]);
  });

  it("ignores characters of other scripts correctly on Backspace", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("ab\u{1F600}\x7fc\r")).toEqual(["abc"]);
  });
});

describe("promptTitle", () => {
  it("trims and collapses whitespace", () => {
    expect(promptTitle("  fix   the\tlogin  page ")).toBe("fix the login page");
  });

  it("ignores empty lines, slash commands and short answers", () => {
    expect(promptTitle("   ")).toBeNull();
    expect(promptTitle("/model opus")).toBeNull();
    expect(promptTitle("1")).toBeNull();
    expect(promptTitle("y")).toBeNull();
    expect(promptTitle("Yes")).toBeNull();
    expect(promptTitle("nein")).toBeNull();
  });

  it("caps long lines with an ellipsis at a word", () => {
    const title = promptTitle(
      "Refactor the session restore so that worktrees survive a restart of the app",
    );
    expect(title).toBe("Refactor the session restore so that worktrees\u2026");
    expect(Array.from(title ?? "").length).toBeLessThanOrEqual(PROMPT_TITLE_MAX);
  });

  it("cuts inside a long word when no space is near", () => {
    const title = promptTitle(`a${"b".repeat(80)}`) ?? "";
    expect(Array.from(title).length).toBe(PROMPT_TITLE_MAX);
    expect(title.endsWith("\u2026")).toBe(true);
  });

  it("keeps scripts without spaces", () => {
    expect(promptTitle("修复登录错误")).toBe("修复登录错误");
  });
});

describe("isAutoTitle", () => {
  const names = ["Claude Code", "claude"];
  it("accepts program names, numbered and worktree titles and the folder", () => {
    expect(isAutoTitle("Claude Code", "E:\\Work\\hangar", names)).toBe(true);
    expect(isAutoTitle("Claude Code (2)", "E:\\Work\\hangar", names)).toBe(true);
    expect(isAutoTitle("Claude Code \u00b7 1a2b3c", "E:\\Work\\hangar", names)).toBe(true);
    expect(isAutoTitle("hangar", "E:\\Work\\hangar", names)).toBe(true);
  });

  it("refuses names the user gave", () => {
    expect(isAutoTitle("Release prep", "E:\\Work\\hangar", names)).toBe(false);
  });
});

describe("promptVerdict", () => {
  const agent: PromptSession = {
    title: "Claude Code",
    cwd: "E:\\Work\\hangar",
    programNames: ["Claude Code", "claude"],
    shell: "agent",
  };

  it("renames an agent session after a task", () => {
    expect(promptVerdict(agent, "fix the login page")).toEqual({
      kind: "rename",
      title: "fix the login page",
    });
  });

  it("waits through slash commands and answers", () => {
    expect(promptVerdict(agent, "/init")).toEqual({ kind: "wait" });
    expect(promptVerdict(agent, "")).toEqual({ kind: "wait" });
  });

  it("stops for plain shells, update sessions and names the user gave", () => {
    expect(promptVerdict({ ...agent, shell: "powershell" }, "dir")).toEqual({ kind: "stop" });
    expect(promptVerdict({ ...agent, kind: "update" }, "fix it now")).toEqual({ kind: "stop" });
    expect(promptVerdict({ ...agent, title: "Mine" }, "fix it now")).toEqual({ kind: "stop" });
  });
});
