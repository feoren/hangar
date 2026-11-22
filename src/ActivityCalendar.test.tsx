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

  it("frames the year with weekday names, the total and a legend", () => {
    const html = render([]);
    expect(html).toContain('class="activity-weekdays"');
    for (const name of ["Mon", "Wed", "Fri"]) expect(html).toContain(`<span>${name}</span>`);
    expect(html).not.toContain("<span>Tue</span>");
    expect(html).toContain('class="activity-footer"');
    expect(html).toMatch(
      /<span>Less<\/span>(<span class="activity-day level-\d"><\/span>){5}<span>More<\/span>/,
    );
  });

  it("drops the abbreviation dot of German weekday names", () => {
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={createTranslator("de")}>
        <ActivityCalendar folders={[]} language="de" />
      </I18nContext.Provider>,
    );
    for (const name of ["Mo", "Mi", "Fr"]) expect(html).toContain(`<span>${name}</span>`);
    expect(html).toContain("<span>Weniger</span>");
  });

  it("shows the empty year while the commits load", () => {
    expect(render(["C:\\src\\hangar"])).toContain("No commits of yours in the last year");
  });
});
