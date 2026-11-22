// @vitest-environment happy-dom

import { act, createRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AgentStartMenu, agentStartItems, offersAgentStart, originText } from "./agentStartMenu";
import type { Profile } from "./api";
import { createTranslator, I18nContext } from "./i18n";
import type { PopupItem } from "./PopupMenu";
import {
  BUILTIN_FIGURES,
  EMPTY_LIBRARY,
  figureName,
  NEUTRAL_FIGURE_COLOR,
  parseFigures,
  type AgentDef,
  type Library,
} from "./subAgentLibrary";

const t = createTranslator("en");
const PROJECT = "C:\\src\\hangar";

function def(name: string, patch: Partial<AgentDef> = {}): AgentDef {
  return {
    name,
    description: `${name} work.\nSecond line.`,
    model: null,
    color: null,
    tools: [],
    scope: "global",
    folder: null,
    file: `${name}.md`,
    ...patch,
  };
}

const claude: Profile = {
  id: "claude",
  name: "Claude Code",
  command: "claude",
  args: [],
  cwd: null,
  icon: null,
  source: "catalog",
  installed: true,
  homepage: null,
  installHint: null,
  takesAgent: true,
};

function actions(items: PopupItem[]) {
  return items.flatMap((item) => (item.type === "action" ? [item] : []));
}

describe("the start as sub-agent menu", () => {
  const global = def("hangar-backend");
  const project = def("hangar-backend", { scope: "project", folder: PROJECT });
  const own = def("reviewer", { scope: "project", folder: PROJECT });
  const library: Library = { ...EMPTY_LIBRARY, defs: [global, project, own] };
  const figures = parseFigures({ "hangar-backend": { kind: "builtin", id: "security-01" } });

  it("lists face, name, origin and the first line of the description", () => {
    const start = vi.fn();
    const items = agentStartItems(claude, [project, own], library, figures, t, start);
    expect(items[0]).toEqual({
      type: "heading",
      key: "heading",
      label: "Start Claude Code as sub-agent",
    });
    const entries = actions(items);
    expect(entries.map((item) => item.label)).toEqual(["hangar-backend", "reviewer"]);
    expect(entries[0]?.detail).toBe(
      "from project hangar, replaces the global one · hangar-backend work.",
    );
    expect(entries[1]?.detail).toBe("from project hangar · reviewer work.");
    expect(entries[1]?.title).toContain("Start Claude Code as reviewer");
    entries[1]?.run();
    expect(start).toHaveBeenCalledWith(claude, "reviewer");
  });

  it("never gives a project definition the global figure of its name", () => {
    const overriding = actions(agentStartItems(claude, [project], library, figures, t, vi.fn()))[0];
    const globalOnly = actions(agentStartItems(claude, [global], library, figures, t, vi.fn()))[0];
    const neutral = renderToStaticMarkup(<>{overriding?.icon}</>);
    expect(neutral).toContain(`background:${NEUTRAL_FIGURE_COLOR}`);
    const trusted = renderToStaticMarkup(<>{globalOnly?.icon}</>);
    expect(trusted).toContain("<img");
    expect(originText(global, library, t)).toBe("Global");
    // The user's own agents wear no figure, only their initial.
    const mine = def("writer");
    const plain = actions(
      agentStartItems(claude, [mine], { ...EMPTY_LIBRARY, defs: [mine] }, figures, t, vi.fn()),
    )[0];
    expect(renderToStaticMarkup(<>{plain?.icon}</>)).not.toContain("<img");
  });

  it("names what a project definition brings beyond its prompt", () => {
    const powered = def("ops", {
      scope: "project",
      folder: PROJECT,
      extras: ["hooks", "mcpServers"],
    });
    const entry = actions(
      agentStartItems(
        claude,
        [powered],
        { ...EMPTY_LIBRARY, defs: [powered] },
        figures,
        t,
        vi.fn(),
      ),
    )[0];
    expect(entry?.detail).toBe("from project hangar · brings hooks, mcpServers · ops work.");
  });

  it("names the built-in figures in words", () => {
    expect(figureName("developer-02", t)).toBe("Developer 2");
    expect(figureName("security-01", createTranslator("de"))).toBe("Sicherheit 1");
    expect(figureName("unknown-01", t)).toBe("unknown-01");
    expect(figureName("social-media-02", t)).toBe("Social media 2");
    expect(figureName("sales-01", createTranslator("de"))).toBe("Vertrieb 1");
    expect(figureName("n8n-03", t)).toBe("n8n 3");
    expect(figureName("contracts-02", createTranslator("de"))).toBe("Verträge 2");
    // Every bundled figure has a name in words.
    for (const id of BUILTIN_FIGURES) expect(figureName(id, t)).not.toBe(id);
  });
});

describe("the sub-agent chevron beside a program", () => {
  const starts = [def("hangar-backend")];

  it("shows only for a program that takes an agent, with a way to start and definitions", () => {
    expect(offersAgentStart(claude, true, starts)).toBe(true);
    expect(offersAgentStart({ ...claude, takesAgent: false }, true, starts)).toBe(false);
    expect(offersAgentStart({}, true, starts)).toBe(false);
    expect(offersAgentStart(claude, false, starts)).toBe(false);
    expect(offersAgentStart(claude, true, [])).toBe(false);
    expect(offersAgentStart(claude, true, undefined)).toBe(false);
  });
});

describe("closing the start as sub-agent menu", () => {
  Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
  let root: Root | null = null;
  let host: HTMLElement | null = null;

  afterEach(() => {
    act(() => {
      root?.unmount();
    });
    root = null;
    host?.remove();
    host = null;
  });

  function open(handlers: { onStart: () => void; onClosed: () => void; onMenuClose: () => void }) {
    const anchor = createRef<HTMLButtonElement>();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    act(() => {
      root?.render(
        <I18nContext.Provider value={t}>
          <button type="button" ref={anchor}>
            chevron
          </button>
          <AgentStartMenu
            profile={claude}
            starts={[def("hangar-backend")]}
            library={EMPTY_LIBRARY}
            figures={new Map()}
            position={{ x: 0, y: 0 }}
            anchor={anchor}
            {...handlers}
          />
        </I18nContext.Provider>,
      );
    });
    return anchor;
  }

  function press(key: string) {
    const focused = document.activeElement;
    act(() => {
      focused?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    });
  }

  it("returns the focus to the chevron after Escape", () => {
    const handlers = { onStart: vi.fn(), onClosed: vi.fn(), onMenuClose: vi.fn() };
    const anchor = open(handlers);
    // The menu takes the focus on its first entry.
    expect(document.activeElement?.textContent).toContain("hangar-backend");
    press("Escape");
    expect(handlers.onClosed).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(anchor.current);
    expect(handlers.onMenuClose).not.toHaveBeenCalled();
    expect(handlers.onStart).not.toHaveBeenCalled();
  });

  it("leaves the focus to the new session after a start", () => {
    const handlers = { onStart: vi.fn(), onClosed: vi.fn(), onMenuClose: vi.fn() };
    const anchor = open(handlers);
    press("Enter");
    expect(handlers.onStart).toHaveBeenCalledWith(claude, "hangar-backend");
    expect(handlers.onClosed).toHaveBeenCalledTimes(1);
    expect(handlers.onMenuClose).toHaveBeenCalledTimes(1);
    expect(document.activeElement).not.toBe(anchor.current);
  });
});
