import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AttentionSettings } from "./AttentionSettings";
import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings } from "./settings";

// A machine on battery, so the power saving switch and its GPU wording show as well.
vi.mock("./usePower", () => ({ usePowerState: () => "battery" }));

const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/140";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";

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

  it("offers desktop notifications and the GPU on Windows", () => {
    const t = createTranslator("en");
    const html = render(WINDOWS);
    expect(html).toContain('id="notify-desktop-help"');
    expect(html).toContain(t("settings.showResources"));
    expect(html).toContain(t("settings.powerSavingHelp"));
  });

  it("hides what Linux cannot do", () => {
    const t = createTranslator("en");
    const html = render(LINUX);
    expect(html).not.toContain('id="notify-desktop-help"');
    expect(html).not.toContain(t("settings.showResources"));
    expect(html).toContain(t("settings.showResourcesNoGpu"));
    expect(html).toContain(t("settings.powerSavingHelpNoGpu"));
  });
});
