import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext, LANGUAGES, translate } from "./i18n";
import type { SubAgent } from "./subAgents";
import { badgeLabel, badgeText, parseSubAgentCounts, runningIn } from "./subAgentCounts";

let counted = 0;
let watched: number | null = null;

vi.mock("./useSubAgentCounts", () => ({
  useSubAgentCount: () => counted,
}));
vi.mock("./useSubAgents", () => ({
  useWatchedRunning: () => watched,
}));

const { RailSubAgentCount, SubAgentCountLabel, SubAgentCountMark } =
  await import("./SubAgentCount");

function render(node: ReactNode): string {
  // German on every machine: without a provider the language follows the system's.
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator("de")}>{node}</I18nContext.Provider>,
  );
}

function agent(id: string, status: SubAgent["status"]): SubAgent {
  return {
    id,
    description: null,
    agentType: null,
    spawnDepth: null,
    status,
    startedAt: null,
    lastActivity: null,
    tool: null,
    target: null,
    note: null,
    tokens: null,
    excerpt: null,
  };
}

describe("parseSubAgentCounts", () => {
  it("keeps sessions with running sub-agents and drops broken entries", () => {
    const parsed = parseSubAgentCounts({
      sessions: [
        { sessionId: 4, running: 2 },
        { sessionId: 5, running: 0 },
        { sessionId: 6, running: -1 },
        { sessionId: "7", running: 1 },
        { sessionId: 8, running: 1.5 },
        null,
        { sessionId: 9, running: 12 },
      ],
    });
    expect(parsed).toEqual(
      new Map([
        [4, 2],
        [9, 12],
      ]),
    );
  });

  it("refuses anything that is not a snapshot", () => {
    for (const value of [null, 3, "x", [], { sessions: "no" }, {}]) {
      expect(parseSubAgentCounts(value)).toBeNull();
    }
  });
});

describe("badge text and label", () => {
  it("shows the count up to nine, then 9+", () => {
    expect(badgeText(1)).toBe("1");
    expect(badgeText(9)).toBe("9");
    expect(badgeText(10)).toBe("9+");
    expect(badgeText(140)).toBe("9+");
  });

  it("names the exact count in every language", () => {
    expect(badgeLabel(12, createTranslator("de"))).toBe("Laufende Sub-Agents: 12");
    expect(badgeLabel(3, createTranslator("en"))).toBe("Running sub-agents: 3");
    for (const language of LANGUAGES) {
      expect(translate(language, "subAgents.runningCount", { count: 3 })).toContain("3");
    }
  });

  it("counts only running sub-agents, as the tree shows them", () => {
    expect(
      runningIn([
        agent("a1", "running"),
        agent("a2", "finished"),
        agent("a3", "stale"),
        agent("a4", "running"),
      ]),
    ).toBe(2);
    expect(runningIn([])).toBe(0);
  });
});

describe("SubAgentCountMark and SubAgentCountLabel", () => {
  it("show nothing while no sub-agent runs", () => {
    expect(render(<SubAgentCountMark running={0} place="badge" />)).toBe("");
    expect(render(<SubAgentCountLabel running={0} />)).toBe("");
  });

  it("show a decorative dot with a hover text and a spoken text", () => {
    expect(render(<SubAgentCountMark running={3} place="badge" />)).toBe(
      '<span class="subagent-count badge-count" title="Laufende Sub-Agents: 3" aria-hidden="true">3</span>',
    );
    expect(render(<SubAgentCountMark running={3} place="corner" />)).toContain(
      'class="subagent-count row-corner"',
    );
    expect(render(<SubAgentCountLabel running={3} />)).toBe(
      '<span class="sr-only">Laufende Sub-Agents: 3</span>',
    );
  });

  it("cap the dot at 9+ but say the exact count", () => {
    const html = render(<SubAgentCountMark running={14} place="rail" />);
    expect(html).toContain('class="subagent-count rail-subagents"');
    expect(html).toContain(">9+</span>");
    expect(html).toContain('title="Laufende Sub-Agents: 14"');
    expect(render(<SubAgentCountLabel running={14} id="x" />)).toContain(
      'id="x">Laufende Sub-Agents: 14',
    );
  });
});

describe("RailSubAgentCount", () => {
  beforeEach(() => {
    counted = 0;
    watched = null;
  });

  function rail(exited = false): string {
    return render(<RailSubAgentCount sessionId={2} cliSessionId="c" exited={exited} />);
  }

  it("takes the backend's count while the session is not watched", () => {
    counted = 2;
    expect(rail()).toContain('aria-hidden="true">2</span>');
    expect(rail()).toContain('id="rail-subagents-2"');
  });

  it("takes the watch's count while the tree follows the session", () => {
    counted = 2;
    watched = 0;
    expect(rail()).toBe("");
    watched = 5;
    expect(rail()).toContain('aria-hidden="true">5</span>');
  });

  it("exited shows nothing, even while sub-agents still count as running", () => {
    counted = 3;
    watched = 3;
    expect(rail(true)).toBe("");
  });
});
