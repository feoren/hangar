import { describe, expect, it } from "vitest";

import { GUARD_LINES, looksLikeQuestion } from "./promptGuard";

// Synthetic screens in the shape of each CLI; texts are harmless markers, not real transcripts.

describe("looksLikeQuestion: Claude Code shapes", () => {
  it("sees a permission dialog with a pointer on the first choice", () => {
    expect(
      looksLikeQuestion([
        "⏺ Bash(echo marker)",
        " Bash command",
        "   echo marker",
        " Do you want to proceed?",
        " ❯ 1. Yes",
        "   2. Yes, and don't ask again for echo commands in this folder",
        "   3. No, and tell Claude what to do differently (esc)",
      ]),
    ).toBe(true);
  });

  it("sees a choice list even without a known phrase", () => {
    expect(looksLikeQuestion(["Pick one", " ❯ 1. Marker A", "   2. Marker B"])).toBe(true);
  });

  it("lets the idle input prompt through", () => {
    expect(
      looksLikeQuestion([
        "⏺ Done. The marker file is updated.",
        "",
        "────────────────────────────────",
        "> ",
        "────────────────────────────────",
        "  ? for shortcuts",
      ]),
    ).toBe(false);
  });
});

describe("looksLikeQuestion: Codex CLI shapes", () => {
  it("sees the command approval", () => {
    expect(
      looksLikeQuestion([
        "Would you like to run the following command?",
        "  $ echo marker",
        "› 1. Yes, proceed",
        "  2. Yes, and don't ask again for commands that start with `echo`",
        "  3. No, and tell Codex what to do differently",
      ]),
    ).toBe(true);
  });

  it("lets the idle composer through", () => {
    expect(
      looksLikeQuestion([
        "• Updated the marker file.",
        "",
        "› Ask Codex to do anything",
        "  100% context left",
      ]),
    ).toBe(false);
  });
});

describe("looksLikeQuestion: Gemini CLI shapes", () => {
  it("sees the tool confirmation", () => {
    expect(
      looksLikeQuestion([
        "│ ?  Shell echo marker",
        "│ Allow execution of: 'echo'?",
        "│ ● 1. Yes, allow once",
        "│   2. Yes, allow always",
        "│   3. No, suggest changes (esc)",
      ]),
    ).toBe(true);
  });

  it("lets the idle input box through", () => {
    expect(
      looksLikeQuestion([
        "✦ The marker is in place.",
        "╭──────────╮",
        "│ >   Type your message or @path/to/file │",
        "╰──────────╯",
      ]),
    ).toBe(false);
  });
});

describe("looksLikeQuestion: numbered answers", () => {
  it("lets a numbered Claude Code answer through", () => {
    expect(
      looksLikeQuestion([
        "● 1. Updated the marker file.",
        "  2. Added a test for it.",
        "",
        "> ",
        "  ? for shortcuts",
      ]),
    ).toBe(false);
  });
});

describe("looksLikeQuestion: classic prompts and limits", () => {
  it("sees yes or no prompts", () => {
    expect(looksLikeQuestion(["Overwrite marker.txt? (y/n)"])).toBe(true);
    expect(looksLikeQuestion(["Continue? [Y/n]"])).toBe(true);
    expect(looksLikeQuestion(["Continue? [y/N]"])).toBe(true);
  });

  it("does not take a plain numbered list in the answer for a choice", () => {
    expect(looksLikeQuestion(["Steps:", "1. Marker one", "2. Marker two", "> "])).toBe(false);
  });

  it("only reads the newest lines", () => {
    const old = [
      "Do you want to proceed?",
      ...Array.from({ length: GUARD_LINES }, (_, i) => `line ${String(i)}`),
    ];
    expect(looksLikeQuestion(old)).toBe(false);
  });

  it("ignores empty rows when counting the newest lines", () => {
    expect(looksLikeQuestion(["Do you want to proceed?", ...Array<string>(40).fill("")])).toBe(
      true,
    );
  });
});
