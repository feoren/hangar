// How a sub-agent definition's origin is put in words: global, from a project, replacing the
// global one of its name, and what a project definition brings beyond its prompt.

import { describe, expect, it } from "vitest";

import { agentOrigin, originText, withExtras } from "./agentStartMenu";
import { createTranslator } from "./i18n";
import { BUILTIN_FIGURES, EMPTY_LIBRARY, figureName, type AgentDef } from "./subAgentLibrary";

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

describe("a definition's origin in words", () => {
  const global = def("hangar-backend");
  const project = def("hangar-backend", { scope: "project", folder: PROJECT });
  const own = def("reviewer", { scope: "project", folder: PROJECT });
  const library = { ...EMPTY_LIBRARY, defs: [global, project, own] };

  it("says global, from a project, and when a project one replaces the global one", () => {
    expect(originText(global, library, t)).toBe("Global");
    expect(agentOrigin(global, library, t)).toBeNull();
    expect(originText(project, library, t)).toBe("from project hangar, replaces the global one");
    expect(originText(own, library, t)).toBe("from project hangar");
  });

  it("names what a project definition brings beyond its prompt", () => {
    const powered = def("ops", {
      scope: "project",
      folder: PROJECT,
      extras: ["hooks", "mcpServers"],
    });
    expect(withExtras("from project hangar", powered, t)).toBe(
      "from project hangar · brings hooks, mcpServers",
    );
    expect(withExtras("Global", global, t)).toBe("Global");
  });

  it("names the built-in figures in words", () => {
    expect(figureName("developer-02", t)).toBe("Developer 2");
    expect(figureName("security-01", createTranslator("de"))).toBe("Sicherheit 1");
    expect(figureName("unknown-01", t)).toBe("unknown-01");
    expect(figureName("social-media-02", t)).toBe("Social media 2");
    expect(figureName("sales-01", createTranslator("de"))).toBe("Vertrieb 1");
    expect(figureName("n8n-03", t)).toBe("n8n 3");
    expect(figureName("contracts-02", createTranslator("de"))).toBe("Verträge 2");
    // Every bundled figure has a name in words.
    for (const id of BUILTIN_FIGURES) expect(figureName(id, t)).not.toBe(id);
  });
});
