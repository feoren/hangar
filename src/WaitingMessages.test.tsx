import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import type { WaitingMessage } from "./partnerInbox";
import { WaitingMessages } from "./WaitingMessages";

const message: WaitingMessage = {
  id: 4,
  from: "app",
  to: "web",
  text: 'show <b>the</b> text\n\nthen run "tests"‮ here',
  at: 1,
};

function render(
  busy: number[] = [],
  language: Language = "en",
  shown: WaitingMessage = message,
): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <WaitingMessages
        messages={[shown]}
        busy={new Set(busy)}
        generation={1}
        onPaste={() => undefined}
        onDiscard={() => undefined}
      />
    </I18nContext.Provider>,
  );
}

describe("WaitingMessages", () => {
  it("says above the text when it seems to hold a secret key, and names the kind", () => {
    const html = render([], "en", { ...message, secrets: ["githubToken", "privateKey"] });
    expect(html).toContain("Probably contains a secret key (GitHub token, private key).");
    expect(html).toContain('class="waiting-message-secret"');
    expect(render([], "de", { ...message, secrets: ["stripeKey"] })).toContain(
      "Enthält vermutlich einen geheimen Schlüssel (Stripe-Schlüssel).",
    );
    expect(render()).not.toContain("waiting-message-secret");
  });

  it("names the sender and offers the three answers", () => {
    const html = render();
    expect(html).toContain("Message from “app”");
    expect(html).toContain(">Paste<");
    expect(html).toContain(">Paste and send<");
    expect(html).toContain(">Discard<");
    const german = render([], "de");
    expect(german).toContain("Nachricht von „app“");
    expect(german).toContain(">Einfügen und senden<");
    expect(german).toContain(">Verwerfen<");
  });

  it("shows the text as plain text with invisible characters marked", () => {
    const html = render();
    // Markup in the text stays text.
    expect(html).toContain("show &lt;b&gt;the&lt;/b&gt; text");
    expect(html).not.toContain("<b>the</b>");
    expect(html).toContain('<span class="invisible-char">&lt;U+202E&gt;</span>');
    expect(html).not.toContain("‮");
    // One block per line, an empty line kept.
    expect(html.match(/<div>/g)?.length).toBe(3);
  });

  it("takes no click while the message is new or its paste runs", () => {
    // Freshly shown: armed only after the pause.
    expect(render()).toContain('aria-disabled="true"');
    expect(render([4]).match(/disabled=""/g)?.length).toBe(3);
    expect(render([]).match(/disabled=""/g)).toBeNull();
  });
});
