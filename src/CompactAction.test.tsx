import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentCommand } from "./api";
import type { SessionContext } from "./contextUsage";

let context: SessionContext | undefined;

vi.mock("./useContextUsage", () => ({
  useSessionContext: () => context,
}));

const { CompactAction } = await import("./CompactAction");

const compact: AgentCommand = {
  labelKey: "compact",
  text: "/compress",
  submit: true,
  confirm: false,
};
const model: AgentCommand = { labelKey: "model", text: "/model", submit: true, confirm: false };

function render(commands: AgentCommand[], busy = false): string {
  return renderToStaticMarkup(
    <CompactAction sessionId={1} commands={commands} busy={busy} onRun={vi.fn()} />,
  );
}

describe("CompactAction", () => {
  beforeEach(() => {
    context = { used: 90_000, window: 100_000 };
  });

  it("shows an icon button named for the action from 80 % on", () => {
    const html = render([model, compact]);
    expect(html).toContain('class="icon-button session-compact"');
    // Without a provider the UI speaks German, the source language.
    expect(html).toContain('aria-label="Kontext komprimieren"');
    expect(html).toContain('title="Kontext komprimieren (/compress)"');
    context = { used: 80_000, window: 100_000 };
    expect(render([compact])).not.toBe("");
  });

  it("stays away below the warning share and before the agent answered", () => {
    context = { used: 79_000, window: 100_000 };
    expect(render([compact])).toBe("");
    context = undefined;
    expect(render([compact])).toBe("");
  });

  it("stays away for programs without a compact command and while the agent is busy", () => {
    expect(render([model])).toBe("");
    expect(render([])).toBe("");
    expect(render([compact], true)).toBe("");
  });
});
