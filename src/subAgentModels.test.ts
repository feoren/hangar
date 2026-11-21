import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
import { describe, expect, it } from "vitest";

import { I18nContext, createTranslator } from "./i18n";
import { EMPTY_LIBRARY, parseLibrary, type CodexDef, type Library } from "./subAgentLibrary";
import { ModelField } from "./SubAgentModelField";
import { IssueList } from "./SubAgentSettings";
import {
  backupsOf,
  claudeOptions,
  failures,
  issuesOf,
  codexSuggestions,
  copiesOf,
  effortOptions,
  OTHER_MODEL,
  ownCodexDefs,
  parseSubAgentSettings,
  provides,
  validModel,
  withBundled,
  withModel,
  withProvide,
  DEFAULT_SETTINGS,
} from "./subAgentModels";

function codex(name: string, patch: Partial<CodexDef> = {}): CodexDef {
  return {
    name,
    description: `${name} work`,
    model: null,
    effort: null,
    scope: "global",
    folder: null,
    file: `${name}.toml`,
    managed: false,
    backup: null,
    backups: [],
    ...patch,
  };
}

describe("model ids", () => {
  it("follow the backend's rule", () => {
    for (const good of ["opus", "claude-opus-5-5", "gpt-6.1-sol", "o3", "vendor:model_1"]) {
      expect(validModel(good), good).toBe(true);
    }
    for (const bad of [
      "",
      "inherit",
      "INHERIT",
      "a b",
      "opus[1m]",
      'x"y',
      "-x",
      "a;b",
      "a".repeat(65),
    ]) {
      expect(validModel(bad), bad).toBe(false);
    }
  });
});

describe("the fields", () => {
  it("offer the aliases and keep a value they do not know", () => {
    expect(claudeOptions(null)).toEqual(["", "opus", "sonnet", "haiku", "fable", OTHER_MODEL]);
    expect(claudeOptions("sonnet")).toEqual(["", "opus", "sonnet", "haiku", "fable", OTHER_MODEL]);
    expect(claudeOptions("opusplan")).toEqual([
      "",
      "opus",
      "sonnet",
      "haiku",
      "fable",
      "opusplan",
      OTHER_MODEL,
    ]);
    expect(effortOptions(null)).toEqual(["", "minimal", "low", "medium", "high", "xhigh"]);
    expect(effortOptions("max")).toContain("max");
  });

  it("render the Claude aliases with the cost note and Codex with model and effort", () => {
    const t = createTranslator("en");
    const html = (program: "claude" | "codex") =>
      renderToStaticMarkup(
        createElement(
          I18nContext.Provider,
          { value: t },
          createElement(ModelField, {
            program,
            value: { model: program === "claude" ? "fable" : "gpt-6.1-sol", effort: null },
            suggestions: ["gpt-6.1-sol"],
            label: "Model of backend",
            onChange: () => undefined,
          }),
        ),
      );
    const claude = html("claude");
    expect(claude).toContain("Fable · much more expensive");
    expect(claude).toContain("Same as main session");
    expect(claude).toContain('aria-label="Model of backend"');
    const codexHtml = html("codex");
    expect(codexHtml).toContain('value="gpt-6.1-sol"');
    expect(codexHtml).toContain("Very high");
    expect(codexHtml).toContain("Effort as main session");
  });
});

describe("settings", () => {
  it("are checked and changed per program", () => {
    expect(parseSubAgentSettings(null)).toEqual(DEFAULT_SETTINGS);
    const parsed = parseSubAgentSettings({
      models: { claude: { model: "sonnet", effort: null }, codex: { model: 7 }, other: {} },
      provideOff: ["codex", "gemini"],
    });
    expect(parsed).toEqual({
      models: { claude: { model: "sonnet", effort: null } },
      provideOff: ["codex"],
      bundledOff: false,
    });
    const cleared = withModel(parsed, "claude", { model: null, effort: null });
    expect(cleared.models).toEqual({});
    const codex = withModel(cleared, "codex", { model: "o3", effort: "high" });
    expect(codex.models.codex).toEqual({ model: "o3", effort: "high" });
    expect(withProvide(parsed, "codex", true).provideOff).toEqual([]);
    expect(withProvide(DEFAULT_SETTINGS, "codex", false).provideOff).toEqual(["codex"]);
  });

  it("switch Hangar's bundled sub-agents, on by default", () => {
    expect(DEFAULT_SETTINGS.bundledOff).toBe(false);
    expect(parseSubAgentSettings({ bundledOff: true }).bundledOff).toBe(true);
    expect(parseSubAgentSettings({ bundledOff: "yes" }).bundledOff).toBe(false);
    const off = withBundled(DEFAULT_SETTINGS, false);
    expect(off.bundledOff).toBe(true);
    expect(withBundled(off, true)).toEqual(DEFAULT_SETTINGS);
  });
});

describe("copies", () => {
  const library: Library = {
    ...EMPTY_LIBRARY,
    codexDefs: [
      codex("manager", { model: "gpt-5" }),
      codex("backend", {
        managed: true,
        scope: "project",
        folder: "C:\\src\\x",
        backup: "backend.toml.bak",
      }),
      codex("backend", { managed: true, model: "o3" }),
    ],
    targets: [{ program: "codex", enabled: true }],
    codexModels: ["gpt-6.1-sol"],
  };

  it("are found per name, the global one first, and own files apart", () => {
    const copies = copiesOf(library, "backend");
    expect(copies.map((copy) => copy.scope)).toEqual(["global", "project"]);
    expect(ownCodexDefs(library).map((def) => def.name)).toEqual(["manager"]);
    expect(provides(library, "codex")).toBe(true);
    expect(provides({ ...library, targets: [{ program: "codex", enabled: false }] }, "codex")).toBe(
      false,
    );
    expect(codexSuggestions(library, library.codexModels)).toEqual(["gpt-6.1-sol", "gpt-5", "o3"]);
  });

  it("come from the backend's answer, checked", () => {
    const parsed = parseLibrary({
      defs: [],
      problems: [
        { program: "codex", scope: "global", folder: null, file: "x.toml", problem: "notToml" },
      ],
      codexDefs: [
        { ...codex("manager"), managed: true, backup: "manager.toml.bak" },
        { ...codex("Bad Name") },
      ],
      installed: ["claude", "codex", "gemini"],
      targets: [
        { program: "codex", enabled: true },
        { program: "x", enabled: true },
      ],
      codexModels: ["gpt-6.1-sol", 5],
    });
    expect(parsed?.codexDefs.map((def) => [def.name, def.managed, def.backup])).toEqual([
      ["manager", true, "manager.toml.bak"],
    ]);
    expect(parsed?.problems[0]?.program).toBe("codex");
    expect(parsed?.installed).toEqual(["claude", "codex"]);
    expect(parsed?.targets).toEqual([{ program: "codex", enabled: true }]);
    expect(parsed?.codexModels).toEqual(["gpt-6.1-sol"]);
    // An older backend sends none of the new fields.
    expect(parseLibrary({ defs: [], problems: [] })).toEqual(EMPTY_LIBRARY);
  });
});

describe("copy issues", () => {
  const library: Library = {
    ...EMPTY_LIBRARY,
    syncIssues: [
      {
        program: "codex",
        scope: "global",
        folder: null,
        file: "manager.toml",
        name: "manager",
        kind: "unreadable",
      },
      {
        program: "codex",
        scope: "project",
        folder: "C:\\src\\hangar",
        file: "backend.toml",
        name: "backend",
        kind: "tracked",
      },
    ],
  };

  it("are found per definition and failures listed apart from tracked files", () => {
    expect(issuesOf(library, "backend").map((issue) => issue.kind)).toEqual(["tracked"]);
    expect(failures(library, "codex").map((issue) => issue.file)).toEqual(["manager.toml"]);
    expect(
      backupsOf(codex("x", { backup: "x.toml.bak", backups: ["x.toml.bak", "x.toml.bak.2"] })),
    ).toEqual(["x.toml.bak", "x.toml.bak.2"]);
  });

  it("are rendered with the file and the reason", () => {
    const t = createTranslator("de");
    const html = renderToStaticMarkup(
      createElement(
        I18nContext.Provider,
        { value: t },
        createElement(IssueList, { issues: library.syncIssues }),
      ),
    );
    expect(html).toContain("<code>manager.toml</code>");
    expect(html).toContain("Datei nicht lesbar");
    expect(html).toContain("Projektdatei in hangar überdeckt deine Kopie");
    expect(renderToStaticMarkup(createElement(IssueList, { issues: [] }))).toBe("");
  });

  it("come from the backend's answer, checked", () => {
    const parsed = parseLibrary({
      defs: [],
      problems: [],
      syncIssues: [
        ...library.syncIssues,
        { program: "codex", scope: "global", file: "x.toml", kind: "boom" },
      ],
    });
    expect(parsed?.syncIssues).toEqual(library.syncIssues);
  });
});
