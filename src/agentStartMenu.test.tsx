import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { agentStartItems, originText } from "./agentStartMenu";
import type { Profile } from "./api";
import { createTranslator } from "./i18n";
import type { PopupItem } from "./PopupMenu";
import {
  EMPTY_LIBRARY,
  figureName,
  NEUTRAL_FIGURE_COLOR,
  parseFigures,
  type AgentDef,
  type Library,
} from "./subAgentLibrary";

const t = createTranslator("en");
const PROJECT = "C:\\src\\hangar";

function def(name: string, patch: Partial<AgentDef> = {}): AgentDef {
  return {
    name,
    description: `${name} work.\nSecond line.`,
    model: null,
    color: null,
    tools: [],
    scope: "global",
    folder: null,
    file: `${name}.md`,
    ...patch,
  };
}

const claude: Profile = {
  id: "claude",
  name: "Claude Code",
  command: "claude",
  args: [],
  cwd: null,
  icon: null,
  source: "catalog",
  installed: true,
  homepage: null,
  installHint: null,
  takesAgent: true,
};

function actions(items: PopupItem[]) {
  return items.flatMap((item) => (item.type === "action" ? [item] : []));
}

describe("the start as sub-agent menu", () => {
  const global = def("hangar-backend");
  const project = def("hangar-backend", { scope: "project", folder: PROJECT });
  const own = def("reviewer", { scope: "project", folder: PROJECT });
  const library: Library = { ...EMPTY_LIBRARY, defs: [global, project, own] };
  const figures = parseFigures({ "hangar-backend": { kind: "builtin", id: "security-01" } });

  it("lists face, name, origin and the first line of the description", () => {
    const start = vi.fn();
    const items = agentStartItems(claude, [project, own], library, figures, t, start);
    expect(items[0]).toEqual({
      type: "heading",
      key: "heading",
      label: "Start Claude Code as sub-agent",
    });
    const entries = actions(items);
    expect(entries.map((item) => item.label)).toEqual(["hangar-backend", "reviewer"]);
    expect(entries[0]?.detail).toBe(
      "from project hangar, replaces the global one · hangar-backend work.",
    );
    expect(entries[1]?.detail).toBe("from project hangar · reviewer work.");
    expect(entries[1]?.title).toContain("Start Claude Code as reviewer");
    entries[1]?.run();
    expect(start).toHaveBeenCalledWith(claude, "reviewer");
  });

  it("never gives a project definition the global figure of its name", () => {
    const overriding = actions(agentStartItems(claude, [project], library, figures, t, vi.fn()))[0];
    const globalOnly = actions(agentStartItems(claude, [global], library, figures, t, vi.fn()))[0];
    const neutral = renderToStaticMarkup(<>{overriding?.icon}</>);
    expect(neutral).toContain(`background:${NEUTRAL_FIGURE_COLOR}`);
    const trusted = renderToStaticMarkup(<>{globalOnly?.icon}</>);
    expect(trusted).toContain("<img");
    expect(originText(global, library, t)).toBe("Global");
    // The user's own agents wear no figure, only their initial.
    const mine = def("writer");
    const plain = actions(
      agentStartItems(claude, [mine], { ...EMPTY_LIBRARY, defs: [mine] }, figures, t, vi.fn()),
    )[0];
    expect(renderToStaticMarkup(<>{plain?.icon}</>)).not.toContain("<img");
  });

  it("names what a project definition brings beyond its prompt", () => {
    const powered = def("ops", {
      scope: "project",
      folder: PROJECT,
      extras: ["hooks", "mcpServers"],
    });
    const entry = actions(
      agentStartItems(
        claude,
        [powered],
        { ...EMPTY_LIBRARY, defs: [powered] },
        figures,
        t,
        vi.fn(),
      ),
    )[0];
    expect(entry?.detail).toBe("from project hangar · brings hooks, mcpServers · ops work.");
  });

  it("names the built-in figures in words", () => {
    expect(figureName("developer-02", t)).toBe("Developer 2");
    expect(figureName("security-01", createTranslator("de"))).toBe("Sicherheit 1");
    expect(figureName("unknown-01", t)).toBe("unknown-01");
    expect(figureName("social-media-02", t)).toBe("Social media 2");
    expect(figureName("sales-01", createTranslator("de"))).toBe("Vertrieb 1");
  });
});
