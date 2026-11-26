import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import type { IntroSummary } from "./linkIntro";
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
  session = 1,
): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <WaitingMessages
        messages={[shown]}
        sends={sends}
        generation={1}
        language={language}
        session={session}
        badgeFor={(profileId) => ({ kind: "letters", text: profileId.slice(0, 1), color: "#123" })}
        onSend={() => undefined}
        onCancel={() => undefined}
        onDiscard={() => undefined}
      />
    </I18nContext.Provider>,
  );
}

const waiting = (submit: boolean, to = 1): ReadonlyMap<number, CardSend> =>
  new Map([[4, { phase: "waiting", submit, target: "ich habe so viele Agenten nun", to }]]);

const summary: IntroSummary = {
  kind: "linked",
  leads: false,
  members: [
    { session: 2, label: "website", detail: "website · Codex CLI", profileId: "codex", lead: true },
    {
      session: 3,
      label: "Fix the relay",
      detail: "Fix the relay",
      profileId: "claude",
      lead: false,
    },
  ],
};

const intro: WaitingMessage = {
  id: 4,
  from: "hangar",
  to: "1",
  text: 'You are linked with other agents in Hangar.\nYour member name is "hangar".',
  at: 1,
  title: "Joined a link group",
  intro: summary,
};

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

  it("says why a message that goes on its own waits, and still offers Send", () => {
    const html = render(new Map(), "de", { ...message, note: "Wartet, bis du fertig bist." });
    expect(html).toContain('<div class="waiting-message-note" role="note">');
    expect(html).toContain("Wartet, bis du fertig bist.");
    expect(html).toContain(">Senden<");
    expect(render()).not.toContain("waiting-message-note");
    // While a send runs, the status line says what happens instead.
    expect(render(waiting(true), "en", { ...message, note: "Waits." })).not.toContain(
      "waiting-message-note",
    );
  });

  it("names sender and time in one line and offers one clear Send", () => {
    const html = render();
    expect(html).toMatch(
      /class="waiting-message-head"><button[^>]*class="waiting-message-fold" aria-expanded="true"[^>]*><svg.*?<\/svg><span class="waiting-message-from">Message from “app”<\/span><\/button><time/,
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
    expect(html).toContain('role="status">Sends when the session is ready<');
    expect(html).toContain(">Cancel<");
    expect(html).not.toContain(">Send<");
    expect(html).not.toContain('aria-label="Discard"');
    expect(render(waiting(false))).toContain("Pastes when the session is ready");
    expect(render(waiting(true), "de")).toContain("Wird gesendet, sobald die Session bereit ist");
    expect(render(waiting(false), "de")).toContain("Wird eingefügt, sobald die Session bereit ist");
    // Once the paste begins, a cancel comes too late and is not offered.
    const sending = render(new Map([[4, { phase: "sending", submit: true, target: "web 1" }]]));
    expect(sending).toContain("Sending …");
    expect(sending).not.toContain(">Cancel<");
  });

  it("folds a card whose send waits to its head line, and keeps announcing", () => {
    const idle = render();
    expect(idle).toContain('data-folded="false"');
    expect(idle).not.toMatch(/class="waiting-message-body"[^>]*hidden/);
    const folded = render(waiting(true));
    expect(folded).toContain('data-folded="true"');
    expect(folded).toMatch(/class="waiting-message-fold" aria-expanded="false"/);
    expect(folded).toMatch(/class="waiting-message-body"[^>]*hidden=""/);
    // The live region sits outside the folded body, the tooltip says the same.
    expect(folded).toContain(
      '<span class="sr-only" role="status">Sends when the session is ready</span>',
    );
    expect(folded).toContain('title="Sends when the session is ready"');
    // A secret hint stays visible on a folded card.
    const secret = render(waiting(true), "en", { ...message, secrets: ["githubToken"] });
    expect(secret.indexOf("waiting-message-secret")).toBeLessThan(
      secret.indexOf("waiting-message-body"),
    );
  });

  it("never puts the session's name into the status sentence", () => {
    // Below its own target the card names no session at all.
    const below = render(waiting(true));
    expect(below).not.toContain("ich habe so viele");
    expect(below).not.toContain("waiting-message-target");
    // Below another row it names the target as a label of its own, with the full name as tooltip.
    const elsewhere = render(waiting(true, 9));
    expect(elsewhere).toContain('role="status">Sends when the session is ready</span>');
    expect(elsewhere).toContain(
      '<span class="waiting-message-target" title="ich habe so viele Agenten nun">To: ich habe so viele Agenten nun</span>',
    );
    expect(render(waiting(true, 9), "de")).toContain(">Ziel: ich habe so viele Agenten nun<");
  });

  it("shows Hangar's introduction as whom the session works with, the agent's text on request", () => {
    const html = render(new Map(), "en", intro);
    expect(html).toContain('<span class="waiting-message-from">Joined a link group</span>');
    expect(html).toContain("This session now works with:");
    expect(html).toContain('<span class="link-intro-name">website</span>');
    expect(html).toContain('<span class="link-intro-lead">Lead</span>');
    expect(html).toContain('title="website · Codex CLI · Lead"');
    expect(html).toContain('<span class="link-intro-name">Fix the relay</span>');
    expect(html).toContain('class="badge"');
    // The agent's English text stays hidden behind a toggle; Send and Paste only stay.
    expect(html).not.toContain("You are linked");
    expect(html).toContain(">Show the agent&#x27;s message<");
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain(">Send<");
    expect(html).toContain(">Paste only<");
    const leads = render(new Map(), "de", { ...intro, intro: { ...summary, leads: true } });
    expect(leads).toContain("Diese Session leitet den Verbund mit:");
    expect(leads).toContain(">Text für den Agenten anzeigen<");
    expect(leads).toContain(">Leitung<");
  });

  it("takes no click while the message is new", () => {
    // Freshly shown: armed only after the pause.
    expect(render().match(/aria-disabled="true"/g)?.length).toBe(3);
  });
});
