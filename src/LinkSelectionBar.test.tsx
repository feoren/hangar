import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import { LinkSelectionBar, type SelectionState } from "./LinkSelectionBar";

function render(element: ReactElement, language: Language = "en"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>{element}</I18nContext.Provider>,
  );
}

function bar(count: number, state: SelectionState, refused = false, language: Language = "en") {
  return render(
    <LinkSelectionBar
      count={count}
      state={state}
      refused={refused}
      clickLabel={language === "de" ? "Strg+Klick" : "Ctrl+click"}
      onLink={() => undefined}
      onClear={() => undefined}
    />,
    language,
  );
}

describe("the link selection bar", () => {
  it("offers Link with nothing else to say when it can link", () => {
    const html = bar(2, "ready");
    expect(html).toContain("2 selected");
    expect(html).toMatch(/<button type="button" class="primary-button link-selection-link">Link/);
    expect(html).toContain('<span class="link-selection-hint" role="status"></span>');
  });

  it("keeps Link visible but off and says what is missing", () => {
    const html = bar(1, "more", false, "de");
    expect(html).toContain("1 ausgewählt");
    expect(html).toContain('class="primary-button link-selection-link" disabled=""');
    expect(html).toContain("Strg+Klick auf eine weitere Session, um sie zu verknüpfen.");
  });

  it("says when the group would hold too many", () => {
    expect(bar(5, "tooMany")).toContain("A link group takes at most 8 sessions.");
  });

  it("says why a session cannot be linked, also without a selection", () => {
    const html = bar(0, "more", true);
    expect(html).toContain(
      "Only running agent sessions started with this version of Hangar can be linked.",
    );
    // Nothing selected: no count and no Link, only the way out.
    expect(html).not.toContain("selected");
    expect(html).not.toContain("link-selection-link");
    expect(html).toContain('aria-label="Clear selection"');
  });
});
