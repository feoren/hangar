// @vitest-environment happy-dom

// Sessions joining an existing link group (ADR 0014): from "Link with …" with one click per
// group, and from "Edit group", where a joining session shows with its default name, which can be
// changed before Save.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { LinkDialog, LinkGroupDialog, type AddedMember } from "./LinkDialog";
import { MAX_LINK_MEMBERS, type LinkGroup } from "./links";
import type { SessionView } from "./sessions";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const LOOK = { kind: "letters", text: "C", color: "#d97757" } as const;

let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
});

function show(element: React.ReactElement) {
  act(() => {
    root.render(
      <I18nContext.Provider value={createTranslator("en")}>{element}</I18nContext.Provider>,
    );
  });
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
  id: 7,
  color: 2,
  askFirst: false,
  braked: false,
  brake: "none",
  budgetLeft: 20,
  budget: 20,
  pendingIntroductions: [],
  members: [
    { session: 1, name: "shop", role: "lead" },
    { session: 2, name: "cart", role: "member" },
  ],
};

function buttons(): HTMLButtonElement[] {
  return [...host.querySelectorAll("button")];
}

function button(text: string): HTMLButtonElement {
  const found = buttons().find((entry) => entry.textContent === text);
  if (found === undefined) throw new Error(`no button "${text}"`);
  return found;
}

function type(input: HTMLInputElement, value: string) {
  act(() => {
    // React tracks the value it set; the prototype's setter makes the change visible to it.
    Reflect.set(HTMLInputElement.prototype, "value", value, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** The button that adds session `title` in "Edit group". */
function adder(title: string): HTMLButtonElement {
  const found = [...host.querySelectorAll<HTMLButtonElement>(".link-add-session")].find(
    (entry) => entry.textContent === title,
  );
  if (found === undefined) throw new Error(`no button to add "${title}"`);
  return found;
}

function add(title: string) {
  act(() => {
    adder(title).click();
  });
}

describe("Link with …", () => {
  it("offers each group with room as one click, besides a new group", async () => {
    const onJoin = vi.fn<(group: number) => Promise<string | null>>(() => Promise.resolve(null));
    show(
      <LinkDialog
        candidates={[session(3, "docs"), session(4, "relay")]}
        groups={[group]}
        onJoin={onJoin}
        initial={[3]}
        badgeFor={() => LOOK}
        programOf={() => "Claude Code"}
        unattended={new Set()}
        selectKey="Ctrl"
        onLink={() => Promise.resolve(null)}
        onClose={() => undefined}
      />,
    );
    expect(host.textContent).toContain("New group");
    expect(host.querySelectorAll('input[type="checkbox"]').length).toBe(2);
    await act(async () => {
      button("Add to group: shop, cart").click();
      await Promise.resolve();
    });
    expect(onJoin).toHaveBeenCalledWith(7);
  });

  it("offers only the groups when no other session could start a new one", () => {
    show(
      <LinkDialog
        candidates={[session(3, "docs")]}
        groups={[group]}
        onJoin={() => Promise.resolve(null)}
        initial={[3]}
        badgeFor={() => LOOK}
        programOf={() => "Claude Code"}
        unattended={new Set()}
        selectKey="Ctrl"
        onLink={() => Promise.resolve(null)}
        onClose={() => undefined}
      />,
    );
    expect(host.textContent).toContain("Add to group: shop, cart");
    expect(host.textContent).not.toContain("New group");
    expect(host.textContent).not.toContain("No other session can be linked");
    expect(host.querySelector('input[type="checkbox"]')).toBeNull();
    // No "Link" button that could never be pressed.
    expect(buttons().some((entry) => entry.textContent === "Link")).toBe(false);
  });
});

type SaveFn = (
  names: { session: number; name: string }[],
  askFirst: boolean,
  lead: number | null,
  added: AddedMember[],
) => Promise<string | null>;

describe("Edit group: adding sessions", () => {
  function edit(onSave: SaveFn, target: LinkGroup = group) {
    show(
      <LinkGroupDialog
        group={target}
        titleOf={(id) => ({ 1: "Shop", 2: "Cart", 3: "Docs", 4: "Relay" })[id] ?? "?"}
        candidates={[session(3, "Docs"), session(4, "Relay")]}
        // The window derives it from folder and program (linkAliases.ts); here the title.
        defaultName={(id) => (id === 3 ? "docs" : "relay")}
        onSave={onSave}
        onClose={() => undefined}
      />,
    );
  }

  it("adds a session only on a click, never on focus or arrow keys", () => {
    edit(() => Promise.resolve(null));
    expect(host.textContent).toContain("Add session");
    const docs = adder("Docs");
    // A native button: Enter and Space press it like a click.
    expect(docs.type).toBe("button");
    act(() => {
      docs.focus();
      for (const key of ["ArrowDown", "ArrowUp", "Tab"]) {
        docs.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      }
    });
    expect(host.querySelector('input[aria-label="Name of Docs"]')).toBeNull();
    add("Docs");
    expect(host.querySelector('input[aria-label="Name of Docs"]')).not.toBeNull();
    // Once added, it is no longer offered.
    expect(() => adder("Docs")).toThrow();
    expect(adder("Relay").textContent).toBe("Relay");
  });

  it("shows a joining session with its default name, editable, and saves it", async () => {
    const onSave = vi.fn<SaveFn>(() => Promise.resolve(null));
    edit(onSave);
    add("Docs");
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Name of Docs"]');
    if (input === null) throw new Error("no name field for the newcomer");
    expect(input.value).toBe("docs");
    expect(host.textContent).toContain("new");
    // The newcomer can lead from the start.
    const lead = [...host.querySelectorAll("select")].find((entry) =>
      entry.textContent.includes("No lead"),
    );
    expect(lead?.textContent).toContain("Docs");
    type(input, "writer");
    await act(async () => {
      button("Save").click();
      await Promise.resolve();
    });
    expect(onSave).toHaveBeenCalledWith(
      [
        { session: 1, name: "shop" },
        { session: 2, name: "cart" },
      ],
      false,
      1,
      [{ session: 3, name: "writer" }],
    );
  });

  it("checks a newcomer's name with the members' and can take it out again", () => {
    edit(() => Promise.resolve(null));
    add("Docs");
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Name of Docs"]');
    if (input === null) throw new Error("no name field for the newcomer");
    type(input, "cart");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "Two members cannot share a name.",
    );
    expect(button("Save").disabled).toBe(true);
    const out = buttons().find((entry) => entry.getAttribute("aria-label") === "Don’t add Docs");
    if (out === undefined) throw new Error("no way to take the newcomer out");
    act(() => {
      out.click();
    });
    expect(host.querySelector('input[aria-label="Name of Docs"]')).toBeNull();
    expect(button("Save").disabled).toBe(false);
  });

  it("offers no session to add once the group is full", () => {
    const members = Array.from({ length: MAX_LINK_MEMBERS }, (_, index) => ({
      session: 10 + index,
      name: `m${String(index)}`,
      role: "member" as const,
    }));
    edit(() => Promise.resolve(null), { ...group, members });
    expect(host.textContent).not.toContain("Add session");
  });
});
