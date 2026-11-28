import { describe, expect, it } from "vitest";

// The catalog in the backend is the source of the agent IDs. It is read as text, so a new entry
// without a description shows up here immediately, without the backend having to export anything.
import catalogText from "../src-tauri/src/agent_catalog.toml?raw";
import {
  agentDescriptionKey,
  DICTIONARIES,
  detectLanguage,
  isLanguage,
  LANGUAGE_NAMES,
  LANGUAGES,
  placeholders,
  translate,
} from "./i18n";

const de = DICTIONARIES.de;
const deKeys = Object.keys(de).sort();

/** Texts with little room as a button, status or sidebar entry (sidebar from 305 px). */
const SHORT_KEYS = [
  "sidebar.sessions",
  "sidebar.newSession",
  "sidebar.pickCwd",
  "session.rename",
  "session.restart",
  "session.close",
  "status.starting",
  "status.idle",
  "status.working",
  "status.attention",
  "status.exited",
  "status.exitedByUser",
  "orphans.end",
  "update.pill",
  "update.later",
  "settings.checkNow",
  "common.ok",
  "common.cancel",
  "common.close",
  "sidebar.signedIn",
  "teamPill.off",
  "teamWindow.manage",
  "teamWindow.back",
  "teamWindow.connect",
  "changes.viewOpen",
  "changes.viewCommits",
  "history.more",
] as const;

describe("dictionaries", () => {
  it("exist for every offered language and only for those", () => {
    expect(Object.keys(DICTIONARIES).sort()).toEqual([...LANGUAGES].sort());
    expect(Object.keys(LANGUAGE_NAMES).sort()).toEqual([...LANGUAGES].sort());
  });

  it("have exactly the keys of the German one in every language", () => {
    for (const language of LANGUAGES) {
      expect(Object.keys(DICTIONARIES[language]).sort(), language).toEqual(deKeys);
    }
  });

  it("contain no empty texts", () => {
    for (const language of LANGUAGES) {
      for (const [key, text] of Object.entries(DICTIONARIES[language])) {
        expect(text.trim(), `${language}:${key}`).not.toBe("");
      }
    }
  });

  it("contain no en or em dashes", () => {
    for (const language of LANGUAGES) {
      for (const [key, text] of Object.entries(DICTIONARIES[language])) {
        expect(/[\u2013\u2014]/.test(text), `${language}:${key}`).toBe(false);
      }
    }
  });

  // One lookup per key and one assertion at the end: a search through the German list per key
  // grew with the square of the key count and ran past the test timeout on slower CI runners.
  it("use the same placeholders as German in every language", () => {
    const german = new Map<string, string>(Object.entries(de));
    const mismatches: string[] = [];
    for (const language of LANGUAGES) {
      for (const [key, text] of Object.entries(DICTIONARIES[language])) {
        const expected = placeholders(german.get(key) ?? "");
        const actual = placeholders(text);
        if (JSON.stringify(actual) !== JSON.stringify(expected)) {
          mismatches.push(
            `${language}:${key} ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`,
          );
        }
      }
    }
    expect(mismatches).toEqual([]);
  });

  // SettingsDialog puts the version after this text as an element with a beta badge.
  it("end with the placeholder for the installed version", () => {
    for (const language of LANGUAGES) {
      expect(DICTIONARIES[language]["settings.installedVersion"], language).toMatch(/\{version\}$/);
    }
  });

  it("keep button, status and sidebar texts short", () => {
    for (const language of LANGUAGES) {
      for (const key of SHORT_KEYS) {
        const text = translate(language, key, { code: 255 });
        expect(text.length, `${language}:${key} "${text}"`).toBeLessThanOrEqual(24);
      }
    }
  });

  // The privacy help must name what the setting reveals: up to three result lines (not only the
  // first) and the newest call's target as the sidebar's activity line.
  it("describe the sub-agent excerpts setting as it behaves", () => {
    const english = translate("en", "settings.subAgentExcerptsHelp");
    expect(english).toContain("the first lines of each result");
    expect(english).toContain("in the sidebar what the newest call works on");
    const german = translate("de", "settings.subAgentExcerptsHelp");
    expect(german).toContain("die ersten Zeilen jedes Ergebnisses");
    expect(german).toContain("Seitenleiste");
  });
});

describe("language selection", () => {
  it("has a distinct, non-empty name for every language", () => {
    const names = LANGUAGES.map((language) => LANGUAGE_NAMES[language].trim());
    expect(names.every((name) => name.length > 0)).toBe(true);
    expect(new Set(names).size).toBe(LANGUAGES.length);
  });

  it("recognizes exactly the offered languages", () => {
    expect(LANGUAGES.every(isLanguage)).toBe(true);
    expect(isLanguage("xx")).toBe(false);
    expect(isLanguage("pt-BR")).toBe(false);
    expect(isLanguage(undefined)).toBe(false);
    expect(isLanguage(42)).toBe(false);
  });
});

describe("translate", () => {
  it("fills in placeholders", () => {
    expect(translate("de", "sidebar.waitingCount", { count: 3 })).toBe("Wartend: 3");
    expect(translate("en", "status.exitCode", { code: 1 })).toBe("Exit code 1");
    expect(translate("ru", "sidebar.waitingCount", { count: 5 })).toBe("Ожидают: 5");
    expect(translate("tr", "update.installingProgress", { version: "1.2.0", percent: 40 })).toBe(
      "Güncelleme yükleniyor: 1.2.0 (%40)",
    );
  });

  it("leaves missing placeholders visible", () => {
    expect(translate("de", "update.current")).toBe("Hangar {version} ist aktuell.");
  });
});

describe("detectLanguage", () => {
  it("detects German in all variants", () => {
    expect(detectLanguage("de")).toBe("de");
    expect(detectLanguage("de-AT")).toBe("de");
    expect(detectLanguage("DE-de")).toBe("de");
    expect(detectLanguage("en-US")).toBe("en");
  });

  it("detects every offered language by the primary subtag", () => {
    for (const language of LANGUAGES) {
      expect(detectLanguage(language)).toBe(language);
      expect(detectLanguage(`${language.toUpperCase()}-XX`)).toBe(language);
    }
    expect(detectLanguage("tr-TR")).toBe("tr");
    expect(detectLanguage("fr-CA")).toBe("fr");
    expect(detectLanguage("es-419")).toBe("es");
    expect(detectLanguage("it-CH")).toBe("it");
    expect(detectLanguage("el-GR")).toBe("el");
    expect(detectLanguage("ru-RU")).toBe("ru");
    expect(detectLanguage("pl-PL")).toBe("pl");
    expect(detectLanguage("ja-JP")).toBe("ja");
    expect(detectLanguage("de_CH")).toBe("de");
  });

  it("maps Portuguese and Chinese in every variant", () => {
    expect(detectLanguage("pt-BR")).toBe("pt");
    expect(detectLanguage("pt-PT")).toBe("pt");
    expect(detectLanguage("zh")).toBe("zh");
    expect(detectLanguage("zh-CN")).toBe("zh");
    expect(detectLanguage("zh-Hans")).toBe("zh");
    expect(detectLanguage("zh-Hans-CN")).toBe("zh");
    expect(detectLanguage("zh-TW")).toBe("zh");
    expect(detectLanguage("zh-Hant-HK")).toBe("zh");
  });

  it("falls back to English for other languages and nonsense", () => {
    expect(detectLanguage("nl-NL")).toBe("en");
    expect(detectLanguage("sv")).toBe("en");
    expect(detectLanguage("")).toBe("en");
    expect(detectLanguage("d")).toBe("en");
    expect(detectLanguage("deu")).toBe("en");
    expect(detectLanguage("zhx")).toBe("en");
    expect(detectLanguage(undefined)).toBe("en");
  });
});

/** IDs of the `[[agent]]` entries: every line that starts with `id = "…"`. */
function catalogIds(text: string): string[] {
  return Array.from(text.matchAll(/^id\s*=\s*"([^"]+)"/gm), (m) => m[1] ?? "");
}

describe("catalog agent descriptions", () => {
  const ids = catalogIds(catalogText);

  it("finds the entries in the catalog", () => {
    expect(ids.length).toBeGreaterThanOrEqual(22);
    expect(ids).toContain("claude");
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("exist for every catalog ID in every language", () => {
    for (const id of ids) {
      const key = agentDescriptionKey(id);
      expect(key, id).toBe(`agent.${id}.description`);
      if (!key) continue;
      for (const language of LANGUAGES) {
        expect(translate(language, key).trim(), `${language}:${id}`).not.toBe("");
      }
    }
  });

  it("exist only for catalog IDs", () => {
    const described = Object.keys(de).filter((key) => /^agent\.[^.]+\.description$/.test(key));
    expect(described.sort()).toEqual(ids.map((id) => `agent.${id}.description`).sort());
  });

  it("stay one or two short sentences in every language", () => {
    for (const id of ids) {
      const key = agentDescriptionKey(id);
      if (!key) continue;
      for (const language of LANGUAGES) {
        const text = translate(language, key);
        const where = `${language}:${id}`;
        expect(text.length, where).toBeLessThanOrEqual(110);
        expect(text, where).toBe(text.trim());
        expect(text, where).toMatch(/[.。]$/);
        expect(text, where).not.toMatch(/[\u2013\u2014]/);
      }
    }
  });

  it("name exactly the evidenced open source license in every language, or none", () => {
    // The license (an SPDX id in round brackets) is only named where the wiki page
    // agent-vendors-licenses has the evidence. A new catalog entry needs a row here,
    // which forces that research before its description claims anything.
    const evidenced: Record<string, string | null> = {
      claude: null,
      codex: "Apache-2.0",
      gemini: "Apache-2.0",
      antigravity: null,
      copilot: null,
      cursor: null,
      kimi: "MIT",
      qwen: "Apache-2.0",
      kiro: null,
      opencode: "MIT",
      aider: "Apache-2.0",
      goose: "Apache-2.0",
      crush: null,
      pi: "MIT",
      omp: "MIT",
      forge: "Apache-2.0",
      kode: "Apache-2.0",
      codebuff: "Apache-2.0",
      cline: "Apache-2.0",
      vibe: "Apache-2.0",
      droid: null,
      amp: null,
      hermes: null,
      openclaw: null,
    };
    const license = (text: string): string | null =>
      /[(（]([A-Za-z0-9.-]+)[)）][.。]$/.exec(text)?.[1] ?? null;
    for (const id of ids) {
      const key = agentDescriptionKey(id);
      if (!key) continue;
      expect(Object.keys(evidenced), `no license evidence for ${id}`).toContain(id);
      for (const language of LANGUAGES) {
        expect(license(translate(language, key)), `${language}:${id}`).toBe(evidenced[id]);
      }
    }
  });

  it("are missing for unknown IDs", () => {
    expect(agentDescriptionKey("my-script")).toBeUndefined();
    expect(agentDescriptionKey("")).toBeUndefined();
  });
});
