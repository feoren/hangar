/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  BUILTIN_FIGURES,
  DEF_TEMPLATE,
  EMPTY_LIBRARY,
  defaultFigure,
  defFigure,
  defsFor,
  firstLine,
  NEUTRAL_FIGURE_COLOR,
  overridesGlobal,
  runningFigure,
  sessionFigure,
  figureErrorCode,
  figureFor,
  isFigureDataUrl,
  libraryEntries,
  parseDefError,
  parseFigures,
  parseLibrary,
  pastedName,
  validAgentName,
  type AgentDef,
  type Library,
} from "./subAgentLibrary";

function def(name: string, patch: Partial<AgentDef> = {}): AgentDef {
  return {
    name,
    description: `${name} work`,
    model: null,
    color: null,
    tools: [],
    scope: "global",
    folder: null,
    file: `${name}.md`,
    ...patch,
  };
}

const PROJECT = "C:\\src\\hangar";

describe("names", () => {
  it("follow the backend's rule", () => {
    for (const good of ["backend", "code-reviewer", "a", "agent-2"]) {
      expect(validAgentName(good), good).toBe(true);
    }
    for (const bad of ["", "Backend", "-x", "x-", "a b", "a_b", "a & b", 7, null, "a".repeat(65)]) {
      expect(validAgentName(bad), String(bad)).toBe(false);
    }
  });

  it("are read from a pasted text for the replace question", () => {
    expect(pastedName(DEF_TEMPLATE)).toBe("code-reviewer");
    expect(pastedName('\uFEFF---\r\nname: "tester"\r\ndescription: d\r\n---\r\n')).toBe("tester");
    expect(pastedName("name: tester")).toBeNull();
    expect(pastedName("---\nname: Big Name\n---\n")).toBeNull();
  });
});

describe("figures", () => {
  it("match the Rust list and the bundled files", () => {
    const rust = readFileSync(
      new URL("../src-tauri/src/agent_figures.rs", import.meta.url),
      "utf8",
    );
    const list = /pub const BUILTIN: &\[&str\] = &\[([\s\S]*?)\];/.exec(rust)?.[1] ?? "";
    const ids = [...list.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
    expect(ids).toEqual([...BUILTIN_FIGURES]);
  });

  it("default to the role in the name, then the color, then a fixed pick", () => {
    expect(defaultFigure("backend", null)).toEqual({ kind: "builtin", id: "backend-01" });
    expect(defaultFigure("code-reviewer", "red")).toEqual({ kind: "builtin", id: "manager-01" });
    expect(defaultFigure("qa-bot", null)).toEqual({ kind: "builtin", id: "tester-01" });
    expect(defaultFigure("writer", "purple")).toEqual({
      kind: "initial",
      letter: "W",
      color: "#8b5cc8",
    });
    const picked = defaultFigure("writer", null);
    expect(picked.kind).toBe("builtin");
    expect(defaultFigure("writer", null)).toEqual(picked);
    expect(defaultFigure("writer", "no-such-color")).toEqual(picked);
  });

  it("take the chosen one first", () => {
    const chosen = parseFigures({
      backend: { kind: "builtin", id: "tester-01" },
      tester: { kind: "image", dataUrl: "data:image/gif;base64,R0lGODlh" },
    });
    expect(figureFor("backend", null, chosen)).toEqual({ kind: "builtin", id: "tester-01" });
    expect(figureFor("tester", null, chosen)).toEqual({
      kind: "image",
      url: "data:image/gif;base64,R0lGODlh",
    });
    expect(figureFor("manager", null, chosen)).toEqual({ kind: "builtin", id: "manager-01" });
  });

  it("drop what the backend would never send", () => {
    const figures = parseFigures({
      ok: { kind: "builtin", id: "security-02" },
      unknown: { kind: "builtin", id: "../x" },
      svg: { kind: "image", dataUrl: "data:image/svg+xml;base64,PHN2Zz4=" },
      remote: { kind: "image", dataUrl: "https://example.test/a.png" },
      "Bad Name": { kind: "builtin", id: "tester-01" },
    });
    expect([...figures.keys()]).toEqual(["ok"]);
    expect(parseFigures(null).size).toBe(0);
    expect(parseFigures([1]).size).toBe(0);
    expect(isFigureDataUrl("data:image/webp;base64,AA")).toBe(true);
    expect(isFigureDataUrl("data:text/html;base64,AA")).toBe(false);
  });
});

describe("the listing", () => {
  const library: Library = {
    ...EMPTY_LIBRARY,
    defs: [
      def("backend"),
      def("tester"),
      def("backend", { scope: "project", folder: PROJECT, description: "project backend" }),
      def("reviewer", { scope: "project", folder: PROJECT }),
      def("other", { scope: "project", folder: "C:\\src\\other" }),
    ],
    problems: [],
  };

  it("groups one name from global and project files into one entry", () => {
    const entries = libraryEntries(library);
    expect(entries.map((entry) => entry.name)).toEqual(["backend", "tester", "reviewer", "other"]);
    const backend = entries[0];
    expect(backend?.global?.scope).toBe("global");
    expect(backend?.projects.map((entry) => entry.folder)).toEqual([PROJECT]);
    expect(backend?.def.description).toBe("backend work");
    expect(entries[2]?.global).toBeNull();
    expect(entries[2]?.def.scope).toBe("project");
  });

  it("lets a project definition override the global one of its name", () => {
    const here = defsFor(library, "c:/src/hangar/");
    expect(here.map((entry) => [entry.name, entry.scope])).toEqual([
      ["backend", "project"],
      ["reviewer", "project"],
      ["tester", "global"],
    ]);
    expect(defsFor(library, null).map((entry) => entry.name)).toEqual(["backend", "tester"]);
  });

  it("is checked when it arrives", () => {
    const parsed = parseLibrary({
      defs: [
        { ...def("backend"), tools: ["Read", 3] },
        { ...def("x"), name: "Bad Name" },
        { ...def("p"), scope: "project", folder: null },
        { ...def("q"), scope: "elsewhere" },
      ],
      problems: [
        { scope: "global", folder: null, file: "broken.md", problem: "noFrontmatter" },
        { scope: "global", folder: null, file: "x.md", problem: "somethingElse" },
      ],
    });
    expect(parsed?.defs.map((entry) => entry.name)).toEqual(["backend"]);
    expect(parsed?.defs[0]?.tools).toEqual(["Read"]);
    // Without a program (an older backend) a problem is Claude Code's.
    expect(parsed?.problems).toEqual([
      {
        program: "claude",
        scope: "global",
        folder: null,
        file: "broken.md",
        problem: "noFrontmatter",
      },
    ]);
    expect(parseLibrary({ defs: "no" })).toBeNull();
    expect(parseLibrary(null)).toBeNull();
  });
});

describe("project definitions that override a global one", () => {
  const chosen = parseFigures({ backend: { kind: "builtin", id: "security-02" } });
  const global = def("backend");
  const project = def("backend", { scope: "project", folder: PROJECT, color: "green" });
  const onlyProject = def("reviewer", { scope: "project", folder: PROJECT });
  const library: Library = { ...EMPTY_LIBRARY, defs: [global, project, onlyProject] };

  it("never wear the global figure", () => {
    expect(overridesGlobal(library, project)).toBe(true);
    expect(overridesGlobal(library, global)).toBe(false);
    expect(overridesGlobal(library, onlyProject)).toBe(false);
    expect(defFigure(global, library, chosen)).toEqual({ kind: "builtin", id: "security-02" });
    expect(defFigure(project, library, chosen)).toEqual({
      kind: "initial",
      letter: "B",
      color: NEUTRAL_FIGURE_COLOR,
    });
    expect(defFigure(onlyProject, library, chosen)).toEqual({
      kind: "builtin",
      id: "manager-01",
    });
  });

  it("decide a session's figure by its folder", () => {
    const here = sessionFigure(library, chosen, "backend", PROJECT);
    expect(here.figure.kind).toBe("initial");
    expect(here.project).toBe(PROJECT);
    const elsewhere = sessionFigure(library, chosen, "backend", "C:\\src\\other");
    expect(elsewhere).toEqual({ figure: { kind: "builtin", id: "security-02" }, project: null });
    // A name no listed definition has keeps its own figure.
    expect(sessionFigure(library, chosen, "tester", PROJECT)).toEqual({
      figure: { kind: "builtin", id: "tester-01" },
      project: null,
    });
  });

  it("name the first line of a description", () => {
    expect(firstLine("One.\nTwo.")).toBe("One.");
    expect(firstLine("")).toBe("");
  });
});

describe("errors and starters", () => {
  it("read the backend's error kinds", () => {
    expect(parseDefError({ kind: "invalid", message: "m", problem: "missingName" })).toEqual({
      kind: "invalid",
      message: "m",
      problem: "missingName",
    });
    expect(parseDefError({ kind: "exists", message: "m", problem: null })?.kind).toBe("exists");
    expect(parseDefError("text")).toBeNull();
    expect(parseDefError({ kind: "other", message: "m" })).toBeNull();
  });
});

describe("figure errors", () => {
  it("are recognised by their stable code, not their English text", () => {
    expect(figureErrorCode("[no_image] The clipboard holds no image")).toBe("no_image");
    expect(figureErrorCode("[not_an_image] Not a PNG, GIF, WebP or JPEG image")).toBe(
      "not_an_image",
    );
    expect(figureErrorCode("[too_large] Image too large: at most 2 MiB")).toBe("too_large");
    expect(figureErrorCode("[too_many] Too many figures: at most 256")).toBeNull();
    expect(figureErrorCode("The clipboard holds no image")).toBeNull();
    expect(figureErrorCode({ kind: "io" })).toBeNull();
  });
});

describe("runningFigure", () => {
  const OTHER = "E:\\work\\website";
  const library: Library = {
    ...EMPTY_LIBRARY,
    defs: [
      def("backend", { scope: "project", folder: PROJECT, file: "backend.md" }),
      def("backend"),
    ],
    problems: [],
  };
  const chosen = new Map([["backend", { kind: "builtin" as const, id: "backend-01" }]]);

  it("wears the chosen figure in a project that does not override the name", () => {
    // Regression: a project override elsewhere turned every session's figure neutral.
    expect(runningFigure(library, chosen, "backend", OTHER)).toEqual({
      kind: "builtin",
      id: "backend-01",
    });
  });

  it("stays neutral in the project that overrides the name", () => {
    expect(runningFigure(library, chosen, "backend", PROJECT)).toEqual({
      kind: "initial",
      letter: "B",
      color: NEUTRAL_FIGURE_COLOR,
    });
  });

  it("stays neutral without a folder while any project overrides the name", () => {
    expect(runningFigure(library, chosen, "backend", null)).toEqual({
      kind: "initial",
      letter: "B",
      color: NEUTRAL_FIGURE_COLOR,
    });
  });

  it("is null for a name no definition has", () => {
    expect(runningFigure(library, chosen, "Explore", OTHER)).toBeNull();
  });
});
