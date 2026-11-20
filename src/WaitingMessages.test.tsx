import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import type { CardSend } from "./messageCard";
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
  sends: ReadonlyMap<number, CardSend> = new Map(),
  language: Language = "en",
  shown: WaitingMessage = message,
): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <WaitingMessages
        messages={[shown]}
        sends={sends}
        generation={1}
        language={language}
        onSend={() => undefined}
        onCancel={() => undefined}
        onDiscard={() => undefined}
      />
    </I18nContext.Provider>,
  );
}

const waiting = (submit: boolean): ReadonlyMap<number, CardSend> =>
  new Map([[4, { phase: "waiting", submit, target: "web 1" }]]);

describe("WaitingMessages", () => {
  it("says above the text when it seems to hold a secret key, and names the kind", () => {
    const html = render(new Map(), "en", { ...message, secrets: ["githubToken", "privateKey"] });
    expect(html).toContain("Probably contains a secret key (GitHub token, private key).");
    expect(html).toContain('class="waiting-message-secret"');
    expect(render(new Map(), "de", { ...message, secrets: ["stripeKey"] })).toContain(
      "Enthält vermutlich einen geheimen Schlüssel (Stripe-Schlüssel).",
    );
    expect(render()).not.toContain("waiting-message-secret");
  });

  it("names sender and time in one line and offers one clear Send", () => {
    const html = render();
    expect(html).toMatch(
      /class="waiting-message-head"><span class="waiting-message-from">Message from “app”<\/span><time/,
    );
    expect(html).toContain('class="primary-button waiting-message-send"');
    expect(html).toContain(">Send<");
    // The quieter ways: a text button and a close mark with its name.
    expect(html).toContain('class="link-button"');
    expect(html).toContain(">Paste only<");
    expect(html).toContain('aria-label="Discard" title="Discard"');
    const german = render(new Map(), "de");
    expect(german).toContain("Nachricht von „app“");
    expect(german).toContain(">Senden<");
    expect(german).toContain(">Nur einfügen<");
    expect(german).toContain('aria-label="Verwerfen"');
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

  it("cuts a long text and offers More", () => {
    const long = { ...message, text: "line\n".repeat(10) };
    const html = render(new Map(), "en", long);
    expect(html.match(/<div>/g)?.length).toBe(4);
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain(">More<");
    expect(render()).not.toContain(">More<");
  });

  it("says that a send waits for its target, with Cancel instead of the buttons", () => {
    const html = render(waiting(true));
    expect(html).toContain('role="status"');
    expect(html).toContain("Sends once web 1 waits");
    expect(html).toContain(">Cancel<");
    expect(html).not.toContain(">Send<");
    expect(html).not.toContain('aria-label="Discard"');
    expect(render(waiting(false))).toContain("Pastes once web 1 waits");
    expect(render(waiting(true), "de")).toContain("Wird gesendet, sobald web 1 wartet");
    // Once the paste begins, a cancel comes too late and is not offered.
    const sending = render(new Map([[4, { phase: "sending", submit: true, target: "web 1" }]]));
    expect(sending).toContain("Sending …");
    expect(sending).not.toContain(">Cancel<");
  });

  it("takes no click while the message is new", () => {
    // Freshly shown: armed only after the pause.
    expect(render().match(/aria-disabled="true"/g)?.length).toBe(3);
  });
});
