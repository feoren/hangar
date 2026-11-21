import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AttentionSettings } from "./AttentionSettings";
import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings } from "./settings";

const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/140";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15";

function render(userAgent: string): string {
  vi.stubGlobal("navigator", { userAgent });
  const t = createTranslator("en");
  return renderToStaticMarkup(
    <I18nContext.Provider value={t}>
      <AttentionSettings settings={defaultSettings("en")} theme="dark" onChange={() => undefined} />
    </I18nContext.Provider>,
  );
}

describe("AttentionSettings per platform", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it.each([
    ["Windows", WINDOWS],
    ["Linux", LINUX],
    ["macOS", MAC],
  ])("offers desktop notifications on %s", (_name, userAgent) => {
    const html = render(userAgent);
    expect(html).toContain('id="notify-desktop-help"');
  });

  it("leaves resources, budget and the morning summary to General", () => {
    const t = createTranslator("en");
    const html = render(WINDOWS);
    expect(html).not.toContain(t("settings.showResources"));
    expect(html).not.toContain('id="daily-budget"');
    expect(html).not.toContain(t("morning.setting"));
  });
});

describe("AttentionSettings wait hint", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("offers the wait threshold with ten minutes preset", () => {
    const t = createTranslator("en");
    const html = render(WINDOWS);
    expect(html).toContain('id="wait-minutes"');
    expect(html).toContain(t("settings.waitTime"));
    expect(html).toContain(t("settings.minutes", { count: 10 }));
  });
});
