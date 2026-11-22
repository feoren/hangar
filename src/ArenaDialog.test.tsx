// @vitest-environment happy-dom

// The comparison of a running arena: "Take over" stays locked while the file list was cut, and
// the row says why instead of leaving a greyed-out button without a reason.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { newContestant, type ArenaRun } from "./arena";
import { ArenaDialog } from "./ArenaDialog";
import { createTranslator, I18nContext } from "./i18n";
import type { SessionView } from "./sessions";

const backend = vi.hoisted(() => ({
  diffStat: vi.fn<(worktree: unknown, baseCommit: string) => Promise<unknown>>(),
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, arenaDiffStat: backend.diffStat } };
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("de");
let root: Root;
let host: HTMLDivElement;

const worktree = {
  path: "C:\\data\\worktrees\\repo-1234abcd\\arena-1a2b3c-claude",
  branch: "hangar/arena-1a2b3c-claude",
  repoRoot: "C:\\src\\repo",
};

const arena: ArenaRun = {
  id: "1a2b3c",
  baseCommit: "a".repeat(40),
  repoRoot: worktree.repoRoot,
  prompt: "Fix the bug",
  testCommand: "",
  startedAt: 0,
  contestants: [
    {
      ...newContestant(1, "claude", "Claude Code", worktree),
      promptSent: true,
      sawWorking: true,
      doneAt: 1000,
    },
  ],
};

const session: SessionView = {
  id: 1,
  profileId: "claude",
  title: "Claude Code",
  cwd: worktree.path,
  status: "running",
  exitCode: null,
  createdAt: 0,
};

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.clearAllMocks();
});

async function render(truncated: boolean) {
  backend.diffStat.mockResolvedValue({
    files: [{ path: "src/a.ts", added: 2, removed: 1 }],
    added: 2,
    removed: 1,
    truncated,
  });
  await act(async () => {
    root.render(
      <I18nContext.Provider value={t}>
        <ArenaDialog
          agents={[]}
          folders={[]}
          badgeFor={() => ({ kind: "letters", text: "CC", color: "#888888" })}
          arena={arena}
          sessions={[session]}
          attention={new Map()}
          onStart={() => Promise.resolve(null)}
          onShow={() => undefined}
          onResend={() => undefined}
          onRunTests={() => undefined}
          onApply={() => Promise.resolve("")}
          onCloseOthers={() => undefined}
          onClose={() => undefined}
        />
      </I18nContext.Provider>,
    );
    // The changes are counted by the backend.
    await Promise.resolve();
  });
}

function applyButton(): HTMLButtonElement | undefined {
  return [...document.querySelectorAll<HTMLButtonElement>(".arena-actions button")].find(
    (button) => button.textContent === t("arena.apply"),
  );
}

describe("ArenaDialog", () => {
  it("says why taking over is locked when not all files were counted", async () => {
    await render(true);
    const button = applyButton();
    expect(button?.disabled).toBe(true);
    const hintId = button?.getAttribute("aria-describedby") ?? "";
    expect(document.getElementById(hintId)?.textContent).toBe(t("arena.tooManyFiles"));
  });

  it("offers taking over without the hint when every file was counted", async () => {
    await render(false);
    const button = applyButton();
    expect(button?.disabled).toBe(false);
    expect(button?.hasAttribute("aria-describedby")).toBe(false);
    expect(document.body.textContent).not.toContain(t("arena.tooManyFiles"));
  });
});
