import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import { LinkDialog, LinkGroupDialog } from "./LinkDialog";
import type { BracketPiece } from "./linkBracket";
import type { PendingDelivery } from "./linkCards";
import {
  LinkBracketHead,
  LinkBracketLine,
  LinkRailEdge,
  linkMarkText,
  pendingLines,
} from "./LinkMark";
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

const piece: BracketPiece = {
  group: 4,
  color: 3,
  braked: false,
  first: true,
  last: false,
  label: true,
  members: 3,
};

function head(
  shown: BracketPiece = piece,
  pending: readonly PendingDelivery[] = [],
  language: Language = "en",
  menuOpen = false,
): string {
  return render(
    <LinkBracketHead
      piece={shown}
      pending={pending}
      menuOpen={menuOpen}
      onMenu={() => undefined}
    />,
    language,
  );
}

const onTheWay: PendingDelivery[] = [
  { id: 7, fromName: "app", toName: "web", text: "Please run the tests\nand report" },
  { id: 8, fromName: null, toName: "docs", text: "You are linked with other agents" },
];

describe("the link bracket", () => {
  it("labels the group with its size, a button that opens the group's menu", () => {
    const html = head();
    expect(html).toContain('class="link-bracket-head"');
    expect(html).toContain("--link-color:var(--link-3)");
    // The name starts with the visible label (WCAG 2.5.3).
    expect(html).toContain('>Group · 3<span class="sr-only">: Group with 3 sessions</span>');
    expect(html).toContain('aria-haspopup="menu"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("aria-label=");
    expect(head(piece, [], "en", true)).toContain('aria-expanded="true"');
    expect(head(piece, [], "de")).toContain(
      '>Gruppe · 3<span class="sr-only">: Gruppe mit 3 Sessions</span>',
    );
  });

  it("dashes its line and says why while the brake holds", () => {
    const braked = { ...piece, braked: true };
    const html = head(braked);
    expect(html).toContain('class="link-bracket head braked"');
    expect(html).toContain("Group with 3 sessions, messages wait for your click");
    expect(render(<LinkBracketLine piece={braked} />)).toContain('class="link-bracket braked"');
  });

  it("counts the messages on their way, quietly, and lists them in the tooltip", () => {
    const html = head(piece, onTheWay);
    expect(html).toContain('<span class="link-bracket-pending">2 on the way</span>');
    expect(html).toContain("app to web: Please run the tests");
    expect(html).toContain("Hangar to docs: You are linked with other agents");
    // Screen readers get the list as the button's description.
    expect(html).toMatch(/aria-describedby="[^"]+"/);
    expect(head(piece, onTheWay, "de")).toContain("2 in Zustellung");
    // Nothing on the way: no count, no description.
    expect(head()).not.toContain("link-bracket-pending");
    expect(head()).not.toContain("aria-describedby");
  });

  it("cuts a long first line in the tooltip", () => {
    const long = [{ id: 1, fromName: "app", toName: "web", text: "x".repeat(200) }];
    const [line] = pendingLines(long, createTranslator("en"));
    expect(line).toBe(`app to web: ${"x".repeat(59)}…`);
  });

  it("closes a run with ticks only where no label stands above it", () => {
    expect(render(<LinkBracketLine piece={piece} />)).toContain('class="link-bracket"');
    const later = { ...piece, label: false, last: true };
    expect(render(<LinkBracketLine piece={later} />)).toContain('class="link-bracket start end"');
    expect(render(<LinkBracketLine piece={later} between />)).toContain(
      'class="link-bracket between"',
    );
  });

  it("is decoration beside a row and in the rail, whose buttons name the link", () => {
    expect(render(<LinkBracketLine piece={piece} />)).toContain('aria-hidden="true"');
    const rail = render(<LinkRailEdge piece={{ ...piece, last: true }} />);
    expect(rail).toContain('class="rail-link-edge start end"');
    expect(rail).toContain('aria-hidden="true"');
  });

  it("names the member for screen readers, and the brake", () => {
    const mark = { group: 1, color: 0, name: "web", braked: false, lead: false };
    expect(linkMarkText(mark, createTranslator("en"))).toBe("Linked as “web”");
    expect(linkMarkText(mark, createTranslator("de"))).toBe("Verknüpft als „web“");
    expect(linkMarkText({ ...mark, braked: true }, createTranslator("en"))).toContain(
      "messages wait for your click",
    );
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
    // Nothing to select: no tip about selecting.
    expect(html).not.toContain("+Klick");
  });

  it("says what a lead does only once one is picked", () => {
    const leadHelp = "The lead hands out tasks";
    const html = render(
      <LinkGroupDialog
        group={{
          id: 1,
          color: 0,
          askFirst: false,
          braked: false,
          members: [
            { session: 1, name: "app", role: "member" },
            { session: 2, name: "web", role: "member" },
          ],
        }}
        titleOf={(id) => `s${String(id)}`}
        onSave={none}
        onClose={() => undefined}
      />,
    );
    expect(html).not.toContain(leadHelp);
    const led = render(
      <LinkGroupDialog
        group={{
          id: 1,
          color: 0,
          askFirst: false,
          braked: false,
          members: [
            { session: 1, name: "app", role: "lead" },
            { session: 2, name: "web", role: "member" },
          ],
        }}
        titleOf={(id) => `s${String(id)}`}
        onSave={none}
        onClose={() => undefined}
      />,
    );
    expect(led).toContain(leadHelp);
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

  it("grows a member's group: groups to merge with, sessions to add, no lead choice", () => {
    const other: LinkGroup = {
      ...group,
      id: 2,
      members: [
        { session: 5, name: "docs", role: "member" },
        { session: 6, name: "shop", role: "lead" },
      ],
    };
    const html = render(
      <LinkDialog
        candidates={[session(3, "relay")]}
        groups={[other]}
        group={group}
        onJoin={none}
        initial={[]}
        badgeFor={() => LOOK}
        programOf={() => "Claude Code"}
        unattended={new Set()}
        selectKey="Ctrl"
        onLink={none}
        onClose={() => undefined}
      />,
    );
    expect(html).toContain("Add sessions to this group, or link it with another group into one.");
    expect(html).toContain("Link with group: docs, shop");
    expect(html).toContain("Add session");
    // One session to add is enough; nothing is checked at the start.
    expect(html.match(/type="checkbox"/g)?.length).toBe(1);
    expect(html).not.toContain('checked=""');
    expect(html).not.toContain("No lead");
    expect(html).toContain("Ctrl+click");
  });

  it("says when a growing group would hold too many", () => {
    const seven: LinkGroup = {
      ...group,
      members: Array.from({ length: 7 }, (_, index) => ({
        session: 10 + index,
        name: `m${String(index)}`,
        role: "member" as const,
      })),
    };
    const html = render(
      <LinkDialog
        candidates={[session(3, "relay"), session(4, "web")]}
        group={seven}
        initial={[3, 4]}
        badgeFor={() => LOOK}
        programOf={() => "Claude Code"}
        unattended={new Set()}
        selectKey="Ctrl"
        onLink={none}
        onClose={() => undefined}
      />,
    );
    expect(html).toContain("A group takes at most 8 sessions.");
    expect(html).toMatch(/class="primary-button" disabled=""/);
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
