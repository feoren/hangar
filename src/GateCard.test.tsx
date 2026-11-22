import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { CommandAsk, PendingDecision } from "./gate";
import { GateCard } from "./GateCard";
import { createTranslator, I18nContext } from "./i18n";

const ask: CommandAsk = {
  rules: [
    {
      id: "rm-outside-project",
      description: "rm -r deletes a folder outside the project, a drive root or the home folder",
      builtin: true,
    },
    { id: "my-rule", description: "my own <b>rule</b>", builtin: false },
  ],
  call: "rm -rf ../other",
  text: 'cd x && rm -rf "../<b>other</b>"\nrm -rf /tmp/x‮ <U+0041>',
  hidden: 12,
  rememberable: true,
};

const held: PendingDecision = {
  id: 9,
  sessionId: 1,
  tool: "Bash",
  path: "",
  otherSessions: [],
  teammates: [],
  reserved: [],
  level: "sameFile",
  lines: [],
  createdAt: 0,
  command: ask,
  agents: [],
};

function render(decision: PendingDecision, language: "en" | "de" = "en"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>
      <GateCard
        decision={decision}
        floating={false}
        more={0}
        error={null}
        nameOf={() => "Claude Code"}
        onAnswer={() => undefined}
        onShowSession={() => undefined}
        onEscape={() => undefined}
        focusToken={0}
        onFocused={() => undefined}
      />
    </I18nContext.Provider>,
  );
}

describe("GateCard for a held shell command", () => {
  it("shows the rules, the matching call and the command verbatim, escaped", () => {
    const html = render(held);
    expect(html).toContain("Claude Code wants to run a dangerous command");
    expect(html).toContain(
      "rm -r deletes a folder outside the project, a drive root or the home folder · my own &lt;b&gt;rule&lt;/b&gt;",
    );
    expect(html).toContain('<p class="gate-command-call selectable">rm -rf ../other</p>');
    // Markup is text, line breaks stay; a real invisible character is a marked placeholder,
    // while the same letters typed by the agent stay plain text.
    expect(html).toContain("&quot;../&lt;b&gt;other&lt;/b&gt;&quot;");
    expect(html).toContain('<span class="invisible-char">&lt;U+202E&gt;</span>');
    expect(html).toContain("<span> &lt;U+0041&gt;</span>");
    expect(html).toContain("12 more characters not shown");
    expect(html).toContain(">Allow once<");
    expect(html).toContain(">Block<");
    expect(html).not.toContain("Allow for this file");
    expect(html).toContain("the command is blocked after 10 minutes");
  });

  it("shows Hangar's own rules in the person's language and the user's as written", () => {
    const html = render(held, "de");
    expect(html).toContain(
      "rm -r löscht einen Ordner außerhalb des Projekts, ein Laufwerk oder den Benutzerordner",
    );
    expect(html).toContain("my own &lt;b&gt;rule&lt;/b&gt;");
  });

  it("offers no remembered answer for Hangar's own settings", () => {
    const html = render({
      ...held,
      command: {
        ...ask,
        rules: [{ id: "hangar-config", description: "x", builtin: true }],
        rememberable: false,
      },
    });
    expect(html).not.toContain("Remember for this project");
    expect(render(held)).toContain("Remember for this project");
  });

  it("keeps the file card as it was", () => {
    const html = render({
      ...held,
      tool: "Edit",
      path: "src/x.ts",
      otherSessions: [2],
      command: null,
    });
    expect(html).toContain("Allow for this file");
    expect(html).not.toContain("gate-command");
  });

  it("shows its answers as not yet clickable right after it appears", () => {
    // The pause itself (useArmDelay.ts) is tested with fake timers; here only that the card
    // starts in it and shows that.
    const html = render(held);
    expect(html).toMatch(/aria-disabled="true"[^>]*>Allow once</u);
    expect(html).toMatch(/aria-disabled="true"[^>]*>Block</u);
  });
});
