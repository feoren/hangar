import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { DiscardDialog } from "./ChangesPanel";
import { createTranslator, I18nContext } from "./i18n";

describe("DiscardDialog", () => {
  it("names the folder the question was asked for", () => {
    const t = createTranslator("de");
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={t}>
        <DiscardDialog
          entry={{
            path: "src/app.ts",
            status: " M",
            additions: 1,
            deletions: 0,
            kind: "modified",
            diff: null,
            hidden: false,
          }}
          cwd={"C:\\Work\\shop"}
          onCancel={() => undefined}
          onConfirm={() => undefined}
        />
      </I18nContext.Provider>,
    );
    expect(html).toContain(t("changes.inFolder", { folder: "shop" }));
    expect(html).toContain('title="C:\\Work\\shop"');
  });
});
