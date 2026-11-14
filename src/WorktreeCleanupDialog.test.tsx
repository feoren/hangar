import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { WorktreeCleanupDialog } from "./WorktreeCleanupDialog";

describe("WorktreeCleanupDialog", () => {
  it("opens with its title, the rule and a calm loading line, without a bulk button", () => {
    const t = createTranslator("de");
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={t}>
        <WorktreeCleanupDialog folders={[]} language="de" onClose={() => undefined} />
      </I18nContext.Provider>,
    );
    expect(html).toContain(t("cleanup.title"));
    expect(html).toContain(t("cleanup.intro"));
    expect(html).toContain(t("cleanup.loading"));
    expect(html).not.toContain(t("cleanup.removeAll", { count: 0 }).split(" (")[0] ?? "");
  });
});
