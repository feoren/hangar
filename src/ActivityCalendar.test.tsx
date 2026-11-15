import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { ActivityCalendar } from "./ActivityCalendar";
import { createTranslator, I18nContext } from "./i18n";

vi.mock("./api", () => ({ api: { gitActivity: vi.fn(() => Promise.resolve({ commits: [] })) } }));

function render(folders: readonly string[]): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator("en")}>
      <ActivityCalendar folders={folders} language="en" />
    </I18nContext.Provider>,
  );
}

describe("ActivityCalendar", () => {
  it("shows the empty year without any known folder", () => {
    const html = render([]);
    expect(html).toContain('class="activity"');
    expect(html).toContain("activity-day level-0");
    expect(html).toContain("No commits of yours in the last year");
  });

  it("shows the empty year while the commits load", () => {
    expect(render(["C:\\src\\hangar"])).toContain("No commits of yours in the last year");
  });
});
