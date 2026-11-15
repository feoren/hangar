import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { SubAgent, SubAgentStatus } from "./subAgents";
import { SubAgentItem } from "./SubAgentTree";

function agent(status: SubAgentStatus): SubAgent {
  return {
    id: "a1",
    description: "Find the parser",
    agentType: "Explore",
    spawnDepth: 1,
    status,
    startedAt: 0,
    lastActivity: 60_000,
    tool: "Grep",
    target: "parseLayout",
    note: null,
    tokens: 12_300,
    excerpt: null,
  };
}

function render(element: ReactElement, language: "de" | "en" = "de"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>{element}</I18nContext.Provider>,
  );
}

function item(status: SubAgentStatus, language: "de" | "en" = "de"): string {
  return render(
    <SubAgentItem
      agent={agent(status)}
      cwd={null}
      now={60_000}
      picked={false}
      onShow={() => undefined}
      onClose={() => undefined}
    />,
    language,
  );
}

/** The markup of the element with class `name`, from its opening tag on. */
function from(html: string, name: string): string {
  const start = html.indexOf(`class="${name}"`);
  return start < 0 ? "" : html.slice(start);
}

describe("SubAgentItem", () => {
  it("closes a finished sub-agent with a text button at the end of its second line", () => {
    const html = item("finished");
    expect(html).toContain('class="subagent-tree-item closable"');
    // The activity line keeps the room the button lies over.
    expect(from(html, "subagent-node-activity")).toMatch(
      /^class="subagent-node-activity"><span class="subagent-activity-text">[^<]*<\/span><span class="subagent-close-room">schließen<\/span><\/span>/,
    );
    const button = from(html, "subagent-close-text");
    expect(button).toContain('aria-label="Explore schließen"');
    expect(button).toContain(
      'title="Explore schließen\nBlendet fertige Sub-Agents hier aus. Ihre Protokolle bleiben erhalten."',
    );
    expect(button).toMatch(/>schließen<\/button>/);
  });

  it("keeps run time and tokens in the first line of a finished row", () => {
    expect(item("finished")).toContain('class="subagent-node-time"');
  });

  it("speaks the close button's name in English too", () => {
    const html = item("finished", "en");
    expect(html).toContain('aria-label="Close Explore"');
    expect(html).toMatch(/>close<\/button>/);
  });

  it("offers no close button while the sub-agent runs", () => {
    const html = item("running");
    expect(html).toContain('class="subagent-tree-item"');
    expect(html).not.toContain("subagent-close");
    expect(html).not.toContain("schließen");
  });
});
