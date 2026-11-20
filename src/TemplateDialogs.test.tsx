import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { SaveTemplateDialog, type TemplateCandidate } from "./TemplateDialogs";

const LOOK = { kind: "letters", text: "C", color: "#d97757" } as const;

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator("en")}>{element}</I18nContext.Provider>,
  );
}

function candidate(id: number, profileId: string): TemplateCandidate {
  return {
    id,
    program: profileId,
    entry: { profileId, folder: `C:\\work\\${String(id)}`, mode: "private" },
  };
}

const base = {
  candidates: [candidate(1, "claude"), candidate(2, "codex"), candidate(3, "pwsh")],
  templates: [],
  badgeFor: () => LOOK,
  askFirst: false,
  onSave: () => undefined,
  onClose: () => undefined,
};

describe("SaveTemplateDialog and link groups", () => {
  it("offers linking checked when the sessions are linked now", () => {
    const html = render(
      <SaveTemplateDialog
        {...base}
        linkable={new Set([1, 2])}
        members={
          new Map([
            [1, { memberName: "boss", role: "lead" as const }],
            [2, { memberName: "web", role: "member" as const }],
          ])
        }
      />,
    );
    expect(html).toContain("Link on start");
    expect(html).toMatch(/<input type="checkbox" checked=""\/><span>Link on start/);
  });

  it("offers it unchecked when two sessions could be linked but are not", () => {
    const html = render(
      <SaveTemplateDialog {...base} linkable={new Set([1, 2])} members={new Map()} />,
    );
    expect(html).toMatch(/<input type="checkbox"\/><span>Link on start/);
  });

  it("does not offer it with fewer than two agent sessions", () => {
    const html = render(
      <SaveTemplateDialog {...base} linkable={new Set([1])} members={new Map()} />,
    );
    expect(html).not.toContain("Link on start");
  });
});
