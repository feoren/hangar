import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import { LinkDialog, LinkGroupDialog } from "./LinkDialog";
import { LinkMarkSign } from "./LinkMark";
import type { LinkGroup } from "./links";
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

const group: LinkGroup = {
  id: 1,
  color: 3,
  askFirst: false,
  braked: false,
  members: [
    { session: 1, name: "app", role: "member" },
    { session: 2, name: "web", role: "member" },
  ],
};

const none = () => Promise.resolve(null);

describe("LinkMarkSign", () => {
  it("is a chain in the group's color that names the member", () => {
    const html = render(
      <LinkMarkSign mark={{ group: 1, color: 3, name: "web", braked: false, lead: false }} />,
    );
    expect(html).toContain('class="session-link"');
    expect(html).toContain("color:var(--link-3)");
    expect(html).toContain("Linked as “web”");
    expect(html).toContain("<svg");
    const german = render(
      <LinkMarkSign mark={{ group: 1, color: 3, name: "web", braked: false, lead: false }} />,
      "de",
    );
    expect(german).toContain("Verknüpft als „web“");
  });

  it("shows the brake by shape and says why messages wait", () => {
    const html = render(
      <LinkMarkSign mark={{ group: 1, color: 0, name: "app", braked: true, lead: false }} />,
    );
    expect(html).toContain('class="session-link braked"');
    expect(html).toContain("messages wait for your click");
  });

  it("is only a picture in the rail, whose button names it", () => {
    const html = render(
      <LinkMarkSign mark={{ group: 1, color: 1, name: "app", braked: false, lead: false }} rail />,
    );
    expect(html).toContain('class="rail-link"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).not.toContain("Linked as");
  });
});

describe("LinkDialog", () => {
  it("lists the sessions that can be linked with the chosen one checked", () => {
    const html = render(
      <LinkDialog
        candidates={[session(1, "app"), session(3, "relay")]}
        initial={[1]}
        badgeFor={() => LOOK}
        programOf={() => "Claude Code"}
        unattended={new Set([3])}
        selectKey="Ctrl"
        onLink={none}
        onClose={() => undefined}
      />,
    );
    expect(html).toContain("Link sessions");
    expect(html.match(/type="checkbox"/g)?.length).toBe(2);
    expect(html.match(/checked=""/g)?.length).toBe(1);
    expect(html).toContain("relay");
    expect(html).toContain("Ctrl+click");
    // Each member names its program, its mode without prompts and its folder.
    expect(html).toContain("Claude Code · without prompts · ");
    expect(html).toContain("Claude Code · work/app");
    // The unattended member is not chosen yet: no note.
    expect(html).not.toContain("so “Ask before pasting” is on");
    // One chosen: nothing to link yet.
    expect(html).toMatch(/<button type="button" class="primary-button" disabled="">Link<\/button>/);
  });

  it("says so when no other session can join", () => {
    const html = render(
      <LinkDialog
        candidates={[session(1, "app")]}
        initial={[1]}
        badgeFor={() => LOOK}
        programOf={() => "Codex CLI"}
        unattended={new Set()}
        selectKey="⌘"
        onLink={none}
        onClose={() => undefined}
      />,
      "de",
    );
    expect(html).toContain("Gerade kann keine andere Session verknüpft werden.");
    expect(html).not.toContain('type="checkbox"');
  });
});

describe("LinkGroupDialog", () => {
  it("offers each member's name and the toggle, off by default", () => {
    const html = render(
      <LinkGroupDialog
        group={group}
        titleOf={(id) => (id === 1 ? "App" : "Web")}
        onSave={none}
        onClose={() => undefined}
      />,
    );
    expect(html).toContain('aria-label="Name of App"');
    expect(html).toContain('value="web"');
    expect(html).toContain("Ask before pasting");
    expect(html).not.toMatch(/type="checkbox" checked=""/);
    expect(html).toContain("background:var(--link-3)");
    expect(html).not.toContain('role="alert"');
  });

  it("shows the toggle on for a group that asks first", () => {
    const html = render(
      <LinkGroupDialog
        group={{ ...group, askFirst: true }}
        titleOf={() => "x"}
        onSave={none}
        onClose={() => undefined}
      />,
      "de",
    );
    expect(html).toMatch(/type="checkbox" checked=""/);
    expect(html).toContain("Vor dem Einfügen fragen");
  });
});

describe("LinkDialog with an agent that runs without prompts", () => {
  it("says that the group will ask first", () => {
    const html = render(
      <LinkDialog
        candidates={[session(1, "app"), session(3, "relay")]}
        initial={[1, 3]}
        badgeFor={() => LOOK}
        programOf={() => "Codex CLI"}
        unattended={new Set([3])}
        selectKey="Ctrl"
        onLink={none}
        onClose={() => undefined}
      />,
      "de",
    );
    expect(html).toContain("Deshalb ist „Vor dem Einfügen fragen“ an");
    expect(html).toContain("Codex CLI · ohne Rückfragen · ");
  });
});

describe("LinkGroupDialog help", () => {
  it("says what the names are for", () => {
    const html = render(
      <LinkGroupDialog group={group} titleOf={() => "x"} onSave={none} onClose={() => undefined} />,
    );
    expect(html).toContain("These are the names the agents use to address each other.");
  });
});
