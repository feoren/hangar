import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import { LinkDialog, LinkGroupDialog } from "./LinkDialog";
import { historySender, LinkHistoryList } from "./LinkHistoryDialog";
import { LinkMarkSign } from "./LinkMark";
import type { LinkGroup, LinkHistoryEntry } from "./links";
import type { SessionView } from "./sessions";

const LOOK = { kind: "letters", text: "C", color: "#d97757" } as const;

function render(element: ReactElement, language: Language = "en"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>{element}</I18nContext.Provider>,
  );
}

function session(id: number, title: string): SessionView {
  return {
    id,
    profileId: "claude",
    title,
    cwd: `/work/${title}`,
    status: "running",
    exitCode: null,
    createdAt: 0,
  };
}

const none = () => Promise.resolve(null);
const copy = () => Promise.resolve();

const entry: LinkHistoryEntry = {
  at: Date.UTC(2026, 10, 20, 9, 30),
  fromName: "app",
  fromRole: "lead",
  toName: "web",
  outcome: "delivered",
  text: "First line\nsecond line",
};

describe("the lead", () => {
  it("carries a mark of its own and says so", () => {
    const html = render(
      <LinkMarkSign mark={{ group: 1, color: 2, name: "app", braked: false, lead: true }} />,
    );
    expect(html).toContain('class="session-link lead"');
    expect(html).toContain("Linked as “app”, lead");
    const german = render(
      <LinkMarkSign mark={{ group: 1, color: 2, name: "app", braked: false, lead: true }} />,
      "de",
    );
    expect(german).toContain("Verknüpft als „app“, Leitung");
  });

  it("is offered in the link dialog once two sessions are chosen, none by default", () => {
    const props = {
      candidates: [session(1, "App"), session(2, "Web")],
      badgeFor: () => LOOK,
      programOf: () => "Claude Code",
      unattended: new Set<number>(),
      selectKey: "Ctrl",
      onLink: none,
      onClose: () => undefined,
    };
    const one = render(<LinkDialog {...props} initial={[1]} />);
    expect(one).not.toContain("No lead");
    const two = render(<LinkDialog {...props} initial={[1, 2]} />);
    expect(two).toContain("<span>Lead</span>");
    expect(two).toMatch(/<option value="" selected="">No lead<\/option>/);
    expect(two).toContain('<option value="2">Web</option>');
    expect(two).toContain("hangar-send lead");
  });

  it("is preselected when editing a group that has one", () => {
    const group: LinkGroup = {
      id: 1,
      color: 1,
      askFirst: false,
      braked: false,
      members: [
        { session: 1, name: "app", role: "member" },
        { session: 2, name: "web", role: "lead" },
      ],
    };
    const html = render(
      <LinkGroupDialog
        group={group}
        titleOf={(id) => (id === 1 ? "App" : "Web")}
        onSave={none}
        onClose={() => undefined}
      />,
      "de",
    );
    expect(html).toContain('<option value="2" selected="">Web</option>');
    expect(html).toContain("Keine Leitung");
  });
});

describe("LinkHistoryList", () => {
  it("shows each message closed to one line, the whole text inside", () => {
    const html = render(
      <LinkHistoryList
        entries={[entry, { ...entry, fromName: null, fromRole: "member", outcome: "discarded" }]}
        language="en"
        copy={copy}
      />,
    );
    expect(html.match(/<details>/g)?.length).toBe(2);
    expect(html).toContain("app (Lead) to web");
    expect(html).toContain("Hangar to web");
    expect(html).toContain("delivered");
    expect(html).toContain("discarded");
    expect(html).toContain('<span class="link-history-preview">First line</span>');
    expect(html).toContain("First line\nsecond line</pre>");
    expect(html).toContain(">Copy</button>");
  });

  it("says when nothing was delivered or discarded yet", () => {
    const html = render(<LinkHistoryList entries={[]} language="de" copy={copy} />, "de");
    expect(html).toContain("Noch keine Nachricht zugestellt oder verworfen.");
  });

  it("names the sender", () => {
    const t = createTranslator("de");
    expect(historySender(entry, t)).toBe("app (Leitung)");
    expect(historySender({ ...entry, fromRole: "member" }, t)).toBe("app");
    expect(historySender({ ...entry, fromName: null }, t)).toBe("Hangar");
  });
});
