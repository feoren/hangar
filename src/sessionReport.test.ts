import { describe, expect, it } from "vitest";

import { DICTIONARIES, LANGUAGES, createTranslator } from "./i18n";
import { exportFileName } from "./sessionExport";
import { canExportReport, reportLabels, utcOffsetMinutes } from "./sessionReport";

describe("session report", () => {
  it("is offered only where the backend reads the transcript", () => {
    expect(canExportReport({ subAgents: true })).toBe(true);
    expect(canExportReport({ subAgents: false })).toBe(false);
    expect(canExportReport({})).toBe(false);
  });

  it("sends every heading, with the token count placeholder in each language", () => {
    for (const language of LANGUAGES) {
      const labels = reportLabels(createTranslator(language));
      for (const [key, text] of Object.entries(labels)) {
        expect(text, `${language} ${key}`).not.toBe("");
      }
      expect(labels.tokens).toContain("{count}");
      expect(DICTIONARIES[language]["session.exportReport"]).not.toBe("");
    }
    expect(Object.keys(reportLabels(createTranslator("en")))).toHaveLength(14);
  });

  it("names the file as Markdown and counts the offset east of UTC", () => {
    const date = new Date(2026, 10, 14, 15, 30);
    expect(exportFileName("Claude Code", date, "report")).toBe("Claude Code 2026-11-14 15-30.md");
    // `+ 0` turns the -0 of a UTC machine into 0, as utcOffsetMinutes does.
    expect(utcOffsetMinutes(date)).toBe(-date.getTimezoneOffset() + 0);
  });
});
