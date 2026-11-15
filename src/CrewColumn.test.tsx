// @vitest-environment happy-dom

// Component test for the crew beside a terminal. Which sub-agents stand in it and their texts are
// covered by crew.test.ts and crewText.test.ts; this file covers the wiring: clicks, the "more"
// button, the accessible name of a figure, the two forms and the empty crew.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CrewColumn } from "./CrewColumn";
import { createTranslator, I18nContext } from "./i18n";
import { MAX_CREW, type CrewForm } from "./crew";
import { EMPTY_VIEW, type SubAgent, type SubAgentView } from "./subAgents";

const view = vi.hoisted((): { current: SubAgentView } => ({
  current: { located: null, agents: [] },
}));

vi.mock("./useSubAgents", () => ({
  useSubAgentView: () => ({ view: view.current, error: null }),
}));

vi.mock("./AgentFigure", async (importOriginal) => {
  const original = await importOriginal<typeof import("./AgentFigure")>();
  return {
    ...original,
    useCrewFigure: () => ({ kind: "initial", letter: "E", color: "#336699" }),
  };
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

function subAgent(id: string, patch: Partial<SubAgent> = {}): SubAgent {
  return {
    id,
    description: `Task ${id}`,
    agentType: "Explore",
    spawnDepth: 1,
    status: "running",
    startedAt: Date.now() - 5000,
    lastActivity: Date.now(),
    tool: "Grep",
    target: null,
    note: null,
    tokens: null,
    excerpt: null,
    ...patch,
  };
}

type ShowFn = (sessionId: number, agentId: string | null, byKeyboard: boolean) => void;
type HideFn = () => void;

let host: HTMLElement;
let root: Root | null;

function mount(
  form: CrewForm,
  handlers: { onShow?: ShowFn; onHide?: HideFn } = {},
  picked: string | null = null,
) {
  const onShow = handlers.onShow ?? vi.fn<ShowFn>();
  const onHide = handlers.onHide ?? vi.fn<HideFn>();
  root = createRoot(host);
  act(() => {
    root?.render(
      <I18nContext.Provider value={createTranslator("de")}>
        <CrewColumn
          form={form}
          sessionId={7}
          sessionTitle="Main session"
          cwd={null}
          watchKey="7:cli"
          hidden={new Set()}
          sessionWaits={false}
          picked={picked}
          onShow={onShow}
          onHide={onHide}
        />
      </I18nContext.Provider>,
    );
  });
  return { onShow, onHide };
}

/** A real mouse click has detail 1; a programmatic click() has 0, which counts as the keyboard. */
function mouseClick(element: HTMLElement | null | undefined) {
  element?.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
}

function buttons(): HTMLButtonElement[] {
  return [...host.querySelectorAll<HTMLButtonElement>("button.crew-button")];
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  view.current = { located: null, agents: [subAgent("a1")] };
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  view.current = EMPTY_VIEW;
});

describe("CrewColumn", () => {
  it("renders nothing without entries", () => {
    view.current = EMPTY_VIEW;
    mount("column");
    expect(host.innerHTML).toBe("");
  });

  it("renders nothing when every sub-agent is history", () => {
    view.current = {
      located: null,
      agents: [subAgent("old", { status: "finished", lastActivity: Date.now() - 60_000 })],
    };
    mount("strip");
    expect(host.innerHTML).toBe("");
  });

  it("shows a click on a figure as onShow(session, agent, false)", () => {
    const { onShow } = mount("column");
    act(() => {
      mouseClick(buttons()[0]);
    });
    expect(onShow).toHaveBeenCalledTimes(1);
    expect(onShow).toHaveBeenCalledWith(7, "a1", false);
  });

  it("reports a keyboard activation (detail 0) as byKeyboard", () => {
    const { onShow } = mount("column");
    act(() => {
      buttons()[0]?.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 0 }));
    });
    expect(onShow).toHaveBeenCalledWith(7, "a1", true);
  });

  it("hides the crew with the hide button", () => {
    const { onHide, onShow } = mount("column");
    const hide = host.querySelector<HTMLButtonElement>("button.crew-hide");
    expect(hide?.textContent).toBe("Crew ausblenden");
    act(() => {
      hide?.click();
    });
    expect(onHide).toHaveBeenCalledTimes(1);
    expect(onShow).not.toHaveBeenCalled();
  });

  it("offers no 'more' button up to the limit", () => {
    view.current = {
      located: null,
      agents: Array.from({ length: MAX_CREW }, (_, i) => subAgent(`a${String(i)}`)),
    };
    mount("column");
    expect(buttons()).toHaveLength(MAX_CREW);
    expect(host.querySelector(".crew-more")).toBeNull();
  });

  it("offers the 'more' button from seven sub-agents on and opens the whole list", () => {
    view.current = {
      located: null,
      agents: Array.from({ length: MAX_CREW + 2 }, (_, i) => subAgent(`a${String(i)}`)),
    };
    const { onShow } = mount("column");
    expect(buttons()).toHaveLength(MAX_CREW);
    const more = host.querySelector<HTMLButtonElement>("button.crew-more");
    expect(more?.textContent).toBe("+2 weitere");
    act(() => {
      mouseClick(more);
    });
    expect(onShow).toHaveBeenCalledWith(7, null, false);
  });

  it("names a figure by name, state word and bubble text", () => {
    mount("column");
    const label = buttons()[0]?.getAttribute("aria-label") ?? "";
    const text = host.querySelector(".crew-bubble-text")?.textContent ?? "";
    expect(text).not.toBe("");
    expect(label).toBe(`Explore, Arbeitet, ${text}`);
  });

  it("uses the same accessible name in the strip", () => {
    mount("strip");
    const text = host.querySelector(".crew-chip-say")?.textContent ?? "";
    expect(text).not.toBe("");
    expect(buttons()[0]?.getAttribute("aria-label")).toBe(`Explore, Arbeitet, ${text}`);
  });

  it("labels the region with the session title", () => {
    mount("column");
    expect(host.querySelector("aside.crew")?.getAttribute("aria-label")).toContain("Main session");
  });

  it("draws the whole figure with a plinth in the column form", () => {
    mount("column");
    expect(host.querySelector("aside")?.className).toBe("crew crew-column");
    expect(host.querySelector(".agent-figure.full")).not.toBeNull();
    expect(host.querySelector(".agent-figure.head")).toBeNull();
    expect(host.querySelector(".crew-item")?.classList.contains("state-working")).toBe(true);
    expect(host.querySelector(".crew-plinth-label")?.textContent).toBe("Arbeitet");
    expect(host.querySelector(".crew-chip-text")).toBeNull();
  });

  it("shows which sub-agent it is under the plinth", () => {
    mount("column");
    expect(host.querySelector(".crew-name")?.textContent).toBe("Explore");
  });

  it("puts the sub-agent's name before its text in the strip", () => {
    mount("strip");
    expect(host.querySelector(".crew-chip-name")?.textContent).toBe("Explore");
    expect(host.querySelector(".crew-name")).toBeNull();
  });

  it("shows the plinth word of a finished sub-agent", () => {
    view.current = {
      located: null,
      agents: [subAgent("a1", { status: "finished", lastActivity: Date.now() })],
    };
    mount("column");
    expect(host.querySelector(".crew-item")?.classList.contains("state-done")).toBe(true);
    expect(host.querySelector(".crew-plinth-label")?.textContent).toBe("Fertig");
  });

  it("draws only the head and one line of text in the strip form", () => {
    mount("strip");
    expect(host.querySelector("aside")?.className).toBe("crew crew-strip");
    expect(host.querySelector(".agent-figure.head")).not.toBeNull();
    expect(host.querySelector(".agent-figure.full")).toBeNull();
    expect(host.querySelector(".crew-plinth")).toBeNull();
    expect(host.querySelector(".crew-bubble")).toBeNull();
    expect(host.querySelector(".crew-chip-text")).not.toBeNull();
  });

  it("marks the picked sub-agent", () => {
    mount("column", {}, "a1");
    expect(host.querySelector(".crew-item")?.classList.contains("picked")).toBe(true);
    expect(buttons()[0]?.getAttribute("aria-current")).toBe("true");
  });
});
