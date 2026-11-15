import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import {
  buildHandover,
  canHandOver,
  canRequestSummary,
  FILES_MAX,
  HANDOVER_MAX_CHARS,
  handoverRequest,
  handoverStart,
  handoverTargets,
  PROMPT_MAX_CHARS,
  SubmittedLines,
  waitUntilReady,
} from "./handover";
import { createTranslator } from "./i18n";

const t = createTranslator("en");

function profile(id: string, command: string, extra: Partial<Profile> = {}): Profile {
  return {
    id,
    name: id,
    command,
    args: [],
    cwd: null,
    icon: null,
    source: "catalog",
    installed: true,
    homepage: null,
    installHint: null,
    ...extra,
  };
}

describe("buildHandover", () => {
  it("names the prompt, the files and the notes, and asks for a summary first", () => {
    const text = buildHandover(
      {
        from: "Claude Code",
        folder: "C:\\work\\my app",
        lastPrompt: "Fix the login\nand add a test",
        files: ["src/login.ts", "src/login.test.ts"],
        notes: [{ title: "Plan", excerpt: "Login first, then the settings." }],
      },
      t,
    );
    expect(text).toContain("Claude Code");
    expect(text).toContain("C:\\work\\my app");
    expect(text).toContain("> Fix the login\n> and add a test");
    expect(text).toContain("Changed files (2):\n- src/login.ts\n- src/login.test.ts");
    expect(text).toContain("- Plan: Login first, then the settings.");
    expect(text.endsWith(t("handover.instruction"))).toBe(true);
  });

  it("leaves out what is missing and strips control characters", () => {
    const text = buildHandover(
      { from: "Codex\u0007", folder: "/w", lastPrompt: null, files: [], notes: [] },
      t,
    );
    expect(text).not.toContain("\u0007");
    expect(text).not.toContain(t("handover.lastPrompt"));
    expect(text).not.toContain(t("handover.notes"));
    expect(text.split("\n\n")).toHaveLength(2);
  });

  it("stays within its bounds", () => {
    const files = Array.from(
      { length: 500 },
      (_, i) => `src/${"deep/".repeat(20)}file-${String(i)}.ts`,
    );
    const notes = Array.from({ length: 9 }, (_, i) => ({
      title: `Note ${String(i)}`,
      excerpt: "x".repeat(400),
    }));
    const text = buildHandover(
      { from: "A", folder: "/w", lastPrompt: "p".repeat(5000), files, notes },
      t,
    );
    expect(text.length).toBeLessThanOrEqual(HANDOVER_MAX_CHARS);
    expect(text).toContain(`> ${"p".repeat(PROMPT_MAX_CHARS - 1)}…`);
    expect(text).toContain("more");
    expect(text).not.toContain("Note 5");
    expect(text.split("\n").filter((line) => line.startsWith("- src/")).length).toBeLessThanOrEqual(
      FILES_MAX,
    );
    expect(text.endsWith(t("handover.instruction"))).toBe(true);
  });
});

describe("who and how", () => {
  it("offers installed agents other than the session's own program", () => {
    const profiles = [
      profile("claude", "claude"),
      profile("codex", "codex"),
      profile("pwsh", "C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
      profile("gemini", "gemini", { installed: false }),
    ];
    expect(handoverTargets(profiles, "claude").map((p) => p.id)).toEqual(["codex"]);
    expect(canHandOver({}, "claude")).toBe(true);
    expect(canHandOver({}, "bash")).toBe(false);
    expect(canHandOver({ kind: "update" }, "claude")).toBe(false);
    expect(canHandOver({}, undefined)).toBe(false);
  });

  it("keeps the folder and the worktree, team only where the program can be one", () => {
    const worktree = { path: "/repo-wt", branch: "hangar/x", repoRoot: "/repo" };
    const team = profile("claude", "claude", { teamCapable: true, teamBlock: null });
    const plain = profile("aider", "aider");
    expect(handoverStart({ cwd: "/w" }, team)).toEqual({ folder: "/w", mode: "private" });
    expect(handoverStart({ cwd: "/repo-wt", mode: "team", worktree }, team)).toEqual({
      folder: "/repo-wt",
      mode: "team",
      worktree,
    });
    expect(handoverStart({ cwd: "/repo-wt", mode: "team", worktree }, plain)).toEqual({
      folder: "/repo-wt",
      mode: "private",
      worktree,
    });
  });

  it("asks for a summary only from an idle running session", () => {
    expect(canRequestSummary({ status: "running" }, "idle")).toBe(true);
    expect(canRequestSummary({ status: "running" }, "attention")).toBe(true);
    expect(canRequestSummary({ status: "running" }, "working")).toBe(false);
    expect(canRequestSummary({ status: "exited" }, "idle")).toBe(false);
  });
});

describe("waitUntilReady", () => {
  const noWait = () => Promise.resolve();

  it("waits for bracketed paste and a pause in the output", async () => {
    let polls = 0;
    const ready = await waitUntilReady(
      {
        bracketedPaste: () => polls >= 3,
        quietFor: () => {
          polls += 1;
          return polls >= 5 ? 1500 : 10;
        },
        gone: () => false,
      },
      noWait,
    );
    expect(ready).toBe("ready");
    expect(polls).toBe(5);
  });

  it("gives up on a closed session", async () => {
    const closed = await waitUntilReady(
      { bracketedPaste: () => false, quietFor: () => null, gone: () => true },
      noWait,
    );
    expect(closed).toBe("gone");
  });

  it("does not paste after the timeout without bracketed paste", async () => {
    const late = await waitUntilReady(
      { bracketedPaste: () => false, quietFor: () => 5000, gone: () => false },
      noWait,
      1000,
    );
    expect(late).toBe("notReady");
  });

  it("pastes after the timeout when bracketed paste is on but the output never pauses", async () => {
    const busy = await waitUntilReady(
      { bracketedPaste: () => true, quietFor: () => 10, gone: () => false },
      noWait,
      1000,
    );
    expect(busy).toBe("ready");
  });

  it("reports a session that closed right at the timeout as gone", async () => {
    let polls = 0;
    const closed = await waitUntilReady(
      {
        bracketedPaste: () => true,
        quietFor: () => 10,
        gone: () => {
          polls += 1;
          return polls > 4;
        },
      },
      noWait,
      1000,
    );
    expect(closed).toBe("gone");
  });
});

describe("handoverRequest", () => {
  it("keeps real requests with their whitespace collapsed", () => {
    expect(handoverRequest("  fix   the login	form ")).toBe("fix the login form");
    expect(handoverRequest("ok")).toBe("ok");
  });

  it("skips empty lines, one-character answers and slash commands", () => {
    for (const line of ["", "   ", "1", "y", "/clear"]) expect(handoverRequest(line)).toBeNull();
  });

  it("caps a long request at what the handover quotes", () => {
    const text = handoverRequest("a".repeat(PROMPT_MAX_CHARS + 50));
    expect(Array.from(text ?? "")).toHaveLength(PROMPT_MAX_CHARS);
    expect(text?.endsWith("…")).toBe(true);
  });
});

describe("SubmittedLines", () => {
  it("keeps the newest request of an agent session", () => {
    const lines = new SubmittedLines();
    expect(lines.request).toBeNull();
    lines.record("fix the login", true);
    lines.record("/clear", true);
    lines.record("y", true);
    lines.record("   ", true);
    expect(lines.request).toBe("fix the login");
    lines.record("now the logout", true);
    expect(lines.request).toBe("now the logout");
  });

  it("never takes a shell's command line as a request", () => {
    const lines = new SubmittedLines();
    lines.record("git status", false);
    expect(lines.request).toBeNull();
    lines.record("write the tests", true);
    lines.record("ls -la", false);
    expect(lines.request).toBe("write the tests");
  });
});
