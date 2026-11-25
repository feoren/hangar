/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  BUILTIN_FIGURES,
  DEF_TEMPLATE,
  EMPTY_LIBRARY,
  defaultFigure,
  defEditable,
  defFigure,
  defsFor,
  firstLine,
  NAME_FIGURES,
  NEUTRAL_FIGURE_COLOR,
  overridesGlobal,
  runningFigure,
  sessionFigure,
  figureErrorCode,
  figureFor,
  isFigureDataUrl,
  isFromHangar,
  isHangarAgent,
  initialFigure,
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

  it("default to the role after Hangar's prefix, else a pick by name", () => {
    expect(defaultFigure("hangar-backend", null)).toEqual({ kind: "builtin", id: "backend-01" });
    expect(defaultFigure("hangar-tester", "red")).toEqual({ kind: "builtin", id: "tester-01" });
    expect(defaultFigure("hangar-marketing", "orange")).toEqual({
      kind: "builtin",
      id: "marketing-01",
    });
    expect(defaultFigure("hangar-sales", null)).toEqual({ kind: "builtin", id: "sales-01" });
    expect(defaultFigure("hangar-social-media", "pink")).toEqual({
      kind: "builtin",
      id: "social-media-01",
    });
    // No role word: a bundled figure picked from the name, whatever the color.
    const picked = defaultFigure("hangar-writer", "orange");
    expect(picked.kind).toBe("builtin");
    expect(defaultFigure("hangar-writer", null)).toEqual(picked);
    // The newer bundled agents carry their own role word.
    expect(defaultFigure("hangar-comfyui", "purple")).toEqual({
      kind: "builtin",
      id: "comfyui-01",
    });
    expect(defaultFigure("hangar-n8n", "orange")).toEqual({ kind: "builtin", id: "n8n-01" });
    expect(defaultFigure("hangar-finance", "green")).toEqual({ kind: "builtin", id: "finance-01" });
    expect(defaultFigure("hangar-contracts", null)).toEqual({
      kind: "builtin",
      id: "contracts-01",
    });
    expect(defaultFigure("hangar-sysadmin", "blue")).toEqual({
      kind: "builtin",
      id: "sysadmin-01",
    });
    // Synonyms point to the same role, but only after Hangar's prefix and only as a whole word.
    const synonyms: [string, string][] = [
      ["admin", "sysadmin-01"],
      ["ops", "sysadmin-01"],
      ["devops", "sysadmin-01"],
      ["automation", "n8n-01"],
      ["accounting", "finance-01"],
      ["tax", "finance-01"],
      ["contract", "contracts-01"],
      ["legal", "contracts-01"],
    ];
    for (const [word, id] of synonyms) {
      expect(defaultFigure(`hangar-${word}`, null)).toEqual({ kind: "builtin", id });
      expect(defaultFigure(word, null).kind).toBe("initial");
    }
    expect(defaultFigure("hangar-taxi", null)).toEqual(defaultFigure("hangar-taxi", "red"));
    expect(defaultFigure("hangar-taxi", null)).not.toEqual({ kind: "builtin", id: "finance-01" });
  });

  it("differ for every bundled agent", () => {
    const names = readdirSync(new URL("../src-tauri/bundled-agents/", import.meta.url))
      .filter((file) => file.endsWith(".md"))
      .map((file) => file.slice(0, -".md".length));
    expect(names.length).toBeGreaterThanOrEqual(15);
    const figures = names.map((name) => {
      const figure = defaultFigure(name, null);
      expect(figure.kind).toBe("builtin");
      return figure.kind === "builtin" ? figure.id : name;
    });
    expect(new Set(figures).size).toBe(names.length);
  });

  it("are Hangar's own: every other agent shows its initial", () => {
    expect(isHangarAgent("hangar-tester")).toBe(true);
    expect(isHangarAgent("hangar-")).toBe(false);
    expect(isHangarAgent("hangar")).toBe(false);
    expect(isHangarAgent("my-hangar-tester")).toBe(false);
    // A role word no longer earns a figure outside Hangar's agents.
    expect(defaultFigure("backend", null)).toEqual(initialFigure("backend", null));
    expect(defaultFigure("code-reviewer", "red")).toEqual({
      kind: "initial",
      letter: "C",
      color: "#d9534f",
    });
    expect(defaultFigure("Explore", null)).toMatchObject({ kind: "initial", letter: "E" });
    // Without a known color the name picks one, the same every time.
    expect(initialFigure("writer", null)).toEqual(initialFigure("writer", "no-such-color"));
    // A choice stored for such a name is ignored, not shown.
    const chosen = parseFigures({
      backend: { kind: "builtin", id: "tester-01" },
      writer: { kind: "image", dataUrl: "data:image/gif;base64,R0lGODlh" },
    });
    expect(figureFor("backend", "green", chosen)).toEqual(initialFigure("backend", "green"));
    expect(figureFor("writer", null, chosen).kind).toBe("initial");
  });

  it("keep a name's figure when new figures are bundled", () => {
    // Pinned: new bundled figures must not move it.
    expect(defaultFigure("hangar-writer", null)).toEqual({ kind: "builtin", id: "manager-02" });
    for (const id of NAME_FIGURES) expect(BUILTIN_FIGURES).toContain(id);
  });

  it("take the chosen one first", () => {
    const chosen = parseFigures({
      "hangar-backend": { kind: "builtin", id: "tester-01" },
      "hangar-tester": { kind: "image", dataUrl: "data:image/gif;base64,R0lGODlh" },
    });
    expect(figureFor("hangar-backend", null, chosen)).toEqual({ kind: "builtin", id: "tester-01" });
    expect(figureFor("hangar-tester", null, chosen)).toEqual({
      kind: "image",
      url: "data:image/gif;base64,R0lGODlh",
    });
    expect(figureFor("hangar-manager", null, chosen)).toEqual({
      kind: "builtin",
      id: "manager-01",
    });
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

describe("Hangar's bundled definitions", () => {
  it("are named only for an unchanged global copy", () => {
    const parsed = parseLibrary({
      defs: [def("backend"), def("tester"), def("manager", { scope: "project", folder: PROJECT })],
      problems: [],
      bundled: ["backend", "manager", "Bad Name", 7],
    });
    expect(parsed?.bundled).toEqual(["backend", "manager"]);
    const library = parsed ?? EMPTY_LIBRARY;
    const marked = libraryEntries(library)
      .filter((entry) => isFromHangar(library, entry))
      .map((entry) => entry.name);
    // A project file of a bundled name is the repository's, never Hangar's.
    expect(marked).toEqual(["backend"]);
    // An older backend sends no list: nothing is marked.
    expect(parseLibrary({ defs: [], problems: [] })?.bundled).toEqual([]);
  });
});

describe("definitions the backend opens and changes", () => {
  it("are the global ones and those of the folders it knows", () => {
    const parsed = parseLibrary({
      defs: [],
      problems: [],
      editable: ["C:\\Work\\Session", 7, "x".repeat(5000)],
    });
    expect(parsed?.editable).toEqual(["C:\\Work\\Session"]);
    const library = parsed ?? EMPTY_LIBRARY;
    expect(defEditable(library, null)).toBe(true);
    expect(defEditable(library, "c:/work/session/")).toBe(true);
    // Chosen for the next start, not started yet: listed, not editable.
    expect(defEditable(library, "C:/Work/Chosen")).toBe(false);
    // An older backend names none: project files offer neither.
    expect(parseLibrary({ defs: [], problems: [] })?.editable).toEqual([]);
  });
});

describe("project definitions that override a global one", () => {
  const chosen = parseFigures({ "hangar-backend": { kind: "builtin", id: "security-02" } });
  const global = def("hangar-backend");
  const project = def("hangar-backend", { scope: "project", folder: PROJECT, color: "green" });
  const onlyProject = def("hangar-reviewer", { scope: "project", folder: PROJECT });
  const library: Library = { ...EMPTY_LIBRARY, defs: [global, project, onlyProject] };

  it("never wear the global figure", () => {
    expect(overridesGlobal(library, project)).toBe(true);
    expect(overridesGlobal(library, global)).toBe(false);
    expect(overridesGlobal(library, onlyProject)).toBe(false);
    expect(defFigure(global, library, chosen)).toEqual({ kind: "builtin", id: "security-02" });
    expect(defFigure(project, library, chosen)).toEqual({
      kind: "initial",
      letter: "H",
      color: NEUTRAL_FIGURE_COLOR,
    });
    // A project's own definition is the repository's: no figure, even under Hangar's prefix.
    expect(defFigure(onlyProject, library, chosen)).toEqual(initialFigure("hangar-reviewer", null));
  });

  it("give the user's own and a project's agents no figure, whatever is stored", () => {
    const own = def("writer", { color: "blue" });
    const stored = parseFigures({ writer: { kind: "builtin", id: "tester-01" } });
    const lib: Library = { ...EMPTY_LIBRARY, defs: [own] };
    expect(defFigure(own, lib, stored)).toEqual({ kind: "initial", letter: "W", color: "#3b82d6" });
    expect(sessionFigure(lib, stored, "writer", PROJECT, [PROJECT]).figure.kind).toBe("initial");
    expect(runningFigure(lib, stored, "writer", null, null)?.kind).toBe("initial");
  });

  it("wear the global figure when they are the same agent (equal fingerprint)", () => {
    const same = def("hangar-backend", { fingerprint: "a1" });
    const copy = def("hangar-backend", { scope: "project", folder: PROJECT, fingerprint: "a1" });
    const changed = def("hangar-backend", {
      scope: "project",
      folder: "C:\\other",
      fingerprint: "b2",
    });
    const lib: Library = { ...EMPTY_LIBRARY, defs: [same, copy, changed] };
    expect(overridesGlobal(lib, copy)).toBe(false);
    expect(defFigure(copy, lib, chosen)).toEqual({ kind: "builtin", id: "security-02" });
    expect(overridesGlobal(lib, changed)).toBe(true);
    // Without a fingerprint on either side nothing proves they are the same.
    const unknown = def("hangar-backend", { scope: "project", folder: PROJECT });
    expect(overridesGlobal({ ...EMPTY_LIBRARY, defs: [same, unknown] }, unknown)).toBe(true);
  });

  it("read the fingerprint from the backend's listing", () => {
    const listed = parseLibrary({
      defs: [
        { ...def("hangar-backend"), fingerprint: "ab12" },
        { ...def("hangar-tester"), fingerprint: 7 },
      ],
      problems: [],
    });
    expect(listed?.defs.map((entry) => entry.fingerprint)).toEqual(["ab12", undefined]);
  });

  it("decide a session's figure by its folder", () => {
    const read = [PROJECT, "C:\\src\\other"];
    const here = sessionFigure(library, chosen, "hangar-backend", PROJECT, read);
    expect(here.figure.kind).toBe("initial");
    expect(here.project).toBe(PROJECT);
    const elsewhere = sessionFigure(library, chosen, "hangar-backend", "C:\\src\\other", read);
    expect(elsewhere).toEqual({ figure: { kind: "builtin", id: "security-02" }, project: null });
    // A name no listed definition has keeps its own figure.
    expect(sessionFigure(library, chosen, "hangar-tester", PROJECT, read)).toEqual({
      figure: { kind: "builtin", id: "tester-01" },
      project: null,
    });
  });

  it("keep a session's chosen figure off until the listing covers its folder", () => {
    const mine = parseFigures({
      "hangar-backend": { kind: "builtin", id: "security-02" },
      "hangar-tester": { kind: "builtin", id: "manager-01" },
    });
    const neutral = { figure: defaultFigure("hangar-backend", null), project: null };
    // Before the first listing arrives: neither the listed name nor an unknown one wears the
    // chosen figure, as a project definition not yet listed could override it.
    expect(sessionFigure(EMPTY_LIBRARY, mine, "hangar-backend", PROJECT, null)).toEqual(neutral);
    expect(sessionFigure(library, mine, "hangar-backend", "C:\\src\\other", null)).toEqual(neutral);
    expect(sessionFigure(EMPTY_LIBRARY, mine, "hangar-tester", PROJECT, null)).toEqual({
      figure: { kind: "builtin", id: "tester-01" },
      project: null,
    });
    // A folder the listing was not read for (a new one, or beyond the folder limit of 32).
    const first32 = Array.from({ length: 32 }, (_, i) => `C:\\src\\p${String(i)}`);
    expect(sessionFigure(library, mine, "hangar-backend", "C:\\src\\p32", first32)).toEqual(
      neutral,
    );
    // Once it covers the folder (compared like the file system does), the choice counts again.
    expect(sessionFigure(library, mine, "hangar-backend", "c:/src/p3/", first32).figure).toEqual({
      kind: "builtin",
      id: "security-02",
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
      def("hangar-backend", { scope: "project", folder: PROJECT, file: "hangar-backend.md" }),
      def("hangar-backend"),
    ],
    problems: [],
  };
  const chosen = new Map([["hangar-backend", { kind: "builtin" as const, id: "backend-01" }]]);
  const READ = [PROJECT, OTHER];

  describe("before the listing covers the session's folder", () => {
    // Only the global definition is listed so far; the project's overriding one is not read yet.
    const globalOnly: Library = { ...EMPTY_LIBRARY, defs: [def("hangar-backend")] };
    const mine = new Map([["hangar-backend", { kind: "builtin" as const, id: "security-02" }]]);
    const neutral = defaultFigure("hangar-backend", null);

    it("keeps the chosen figure off while no listing has arrived", () => {
      expect(runningFigure(globalOnly, mine, "hangar-backend", PROJECT, null)).toEqual(neutral);
      expect(neutral).not.toEqual({ kind: "builtin", id: "security-02" });
    });

    it("keeps the chosen figure off in a folder beyond the first 32", () => {
      const first32 = Array.from({ length: 32 }, (_, i) => `C:\\src\\p${String(i)}`);
      expect(runningFigure(globalOnly, mine, "hangar-backend", PROJECT, first32)).toEqual(neutral);
    });

    it("wears the chosen figure once the folder is covered, in any spelling", () => {
      expect(
        runningFigure(globalOnly, mine, "hangar-backend", PROJECT, ["c:/SRC/hangar/"]),
      ).toEqual({ kind: "builtin", id: "security-02" });
    });
  });

  it("wears the chosen figure in a project that does not override the name", () => {
    // Regression: a project override elsewhere turned every session's figure neutral.
    expect(runningFigure(library, chosen, "hangar-backend", OTHER, READ)).toEqual({
      kind: "builtin",
      id: "backend-01",
    });
  });

  it("stays neutral in the project that overrides the name", () => {
    expect(runningFigure(library, chosen, "hangar-backend", PROJECT, READ)).toEqual({
      kind: "initial",
      letter: "H",
      color: NEUTRAL_FIGURE_COLOR,
    });
  });

  it("stays neutral without a folder while any project overrides the name", () => {
    expect(runningFigure(library, chosen, "hangar-backend", null, null)).toEqual({
      kind: "initial",
      letter: "H",
      color: NEUTRAL_FIGURE_COLOR,
    });
  });

  it("is null for a name no definition has", () => {
    expect(runningFigure(library, chosen, "Explore", OTHER, READ)).toBeNull();
  });
});
