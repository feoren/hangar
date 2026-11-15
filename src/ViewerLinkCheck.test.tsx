import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { LinkAddress } from "./LinkAddress";
import { ViewerLinkCheck } from "./ViewerLinkCheck";

const noop = () => undefined;

describe("ViewerLinkCheck", () => {
  it("shows the real address with its host in bold, and asks before opening", () => {
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={createTranslator("en")}>
        <ViewerLinkCheck url="https://evil.example/x?a=1" onOpen={noop} onCancel={noop} />
      </I18nContext.Provider>,
    );
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("This link’s text does not match where it leads.");
    expect(html).toContain('https://<strong class="link-host">evil.example</strong>/x?a=1</span>');
    expect(html).toContain(">Open</button>");
    expect(html).toContain(">Cancel</button>");
  });
});

describe("LinkAddress", () => {
  it("escapes the address and keeps it whole without a host", () => {
    const html = renderToStaticMarkup(<LinkAddress url="not <b>a</b> link" />);
    expect(html).toBe('<span class="link-url selectable">not &lt;b&gt;a&lt;/b&gt; link</span>');
  });
});
