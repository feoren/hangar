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

/** Texts with little room as a button, status or sidebar entry (sidebar from 200 px). */
const SHORT_KEYS = [
  "sidebar.sessions",
  "sidebar.newSession",
  "sidebar.pickCwd",
  "session.rename",
  "session.restart",
  "session.close",
  "status.starting",
  "status.running",
  "status.working",
  "status.attention",
  "status.exited",
  "status.exitedByUser",
  "orphans.end",
  "update.install",
  "update.installNow",
  "update.later",
  "settings.checkNow",
  "common.ok",
  "common.cancel",
  "common.close",
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

  it("use the same placeholders as German in every language", () => {
    for (const language of LANGUAGES) {
      for (const [key, text] of Object.entries(DICTIONARIES[language])) {
        expect(placeholders(text), `${language}:${key}`).toEqual(
          placeholders(Object.entries(de).find(([k]) => k === key)?.[1] ?? ""),
        );
      }
    }
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
    expect(translate("de", "update.endsMany", { count: 3 })).toBe(
      "3 laufende Sessions werden dabei beendet.",
    );
    expect(translate("en", "status.exitCode", { code: 1 })).toBe("Exit code 1");
    expect(translate("ru", "update.endsMany", { count: 5 })).toBe(
      "Будут закрыты запущенные сеансы: 5.",
    );
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

  it("are missing for unknown IDs", () => {
    expect(agentDescriptionKey("my-script")).toBeUndefined();
    expect(agentDescriptionKey("")).toBeUndefined();
  });
});
