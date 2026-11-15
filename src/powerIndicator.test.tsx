import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { PowerIndicator } from "./SystemBar";

function render(active: boolean, language: "de" | "en" = "en"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <PowerIndicator active={active} />
    </I18nContext.Provider>,
  );
}

describe("PowerIndicator", () => {
  it("shows nothing while not saving", () => {
    expect(render(false)).toBe("");
  });

  it("shows a labelled leaf while saving", () => {
    const html = render(true);
    expect(html).toContain('class="power-indicator"');
    expect(html).toContain('role="img"');
    expect(html).toContain('aria-label="Energy saving on"');
    expect(html).toContain("<svg");
  });

  it("names it in German as asked", () => {
    expect(render(true, "de")).toContain('aria-label="Energiesparen aktiv"');
  });
});
