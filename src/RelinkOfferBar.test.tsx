import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import { RelinkOfferBar } from "./RelinkOfferBar";

function bar(missing: string[], language: Language = "en"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <RelinkOfferBar offer={{ names: ["Shop", "web"], missing }} onDecide={() => undefined} />
    </I18nContext.Provider>,
  );
}

describe("the bar offering a group of the last run", () => {
  it("asks, names the members and offers Link and Discard", () => {
    const html = bar([], "de");
    expect(html).toContain("Gruppe von vor dem Neustart wieder verknüpfen?");
    expect(html).toContain("Shop, web");
    expect(html).toContain(">Verknüpfen</button>");
    expect(html).toContain(">Verwerfen</button>");
    expect(html).not.toContain("Nicht dabei");
  });

  it("names who is missing", () => {
    expect(bar(["docs", "relay"])).toContain("Not here: docs, relay");
  });
});
