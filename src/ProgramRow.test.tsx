// @vitest-environment happy-dom

// The program rows in "New session": a row is one start button with no pill, plus or arrow. A row
// with models opens them in a flyout beside it once the pointer rests on it, keeps it open on the
// way into the flyout, closes it when the pointer leaves both, hands over to the next row at once
// and opens with Right arrow from the keyboard.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SessionModel } from "./api";
import { createTranslator, I18nContext } from "./i18n";
import { modelSections, type ModelSection } from "./localLlm";
import { SUBMENU_GRACE_MS, SUBMENU_OPEN_DELAY_MS } from "./PopupMenu";
import { FLYOUT_GRACE_MS, FLYOUT_OPEN_DELAY_MS, ProgramList, ProgramRow } from "./ProgramRow";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

const sectionsFor = (name: string, id: string, label: string) =>
  modelSections({ name, models: [{ id, label }], localSources: [] }, null);

const CLAUDE = sectionsFor("Claude Code", "opus", "Opus 5.5");
const CODEX = sectionsFor("Codex", "gpt-5", "GPT-5");

interface RowSpec {
  id: string;
  name: string;
  models: readonly ModelSection[];
}

const ROWS: readonly RowSpec[] = [
  { id: "claude", name: "Claude Code", models: CLAUDE },
  { id: "codex", name: "Codex", models: CODEX },
  { id: "shell", name: "PowerShell", models: [] },
];

describe("ProgramRow", () => {
  let root: Root;
  let host: HTMLDivElement;
  /** Outside the list, where the pointer goes when it leaves. */
  let outside: HTMLDivElement;

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    host = document.createElement("div");
    outside = document.createElement("div");
    document.body.append(host, outside);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    vi.restoreAllMocks();
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  function render(rows: readonly RowSpec[] = ROWS) {
    const onStart = vi.fn<(id: string, model?: SessionModel) => void>();
    const onMenuClose = vi.fn();
    const onMove = vi.fn<(id: string, toIndex: number, shown: readonly string[]) => void>();
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <ProgramList onMove={onMove}>
            {rows.map((row) => (
              <ProgramRow
                key={row.id}
                id={row.id}
                name={row.name}
                commandLine={`${row.id} --verbose`}
                badge={{ kind: "letters", text: "CC", color: "#5aa9ff" }}
                version="2.1.0"
                updateAvailable={false}
                models={row.models}
                onStart={(model) => {
                  if (model === undefined) onStart(row.id);
                  else onStart(row.id, model);
                }}
                onMenuClose={onMenuClose}
              />
            ))}
          </ProgramList>
        </I18nContext.Provider>,
      );
    });
    return { onStart, onMenuClose, onMove };
  }

  /** Rows 30 px high, one below the other, as the sidebar lays them out. */
  function layOut() {
    host.querySelectorAll<HTMLLIElement>("li[data-program]").forEach((li, index) => {
      li.getBoundingClientRect = () => new DOMRect(0, index * 30, 200, 30);
    });
  }

  function press(on: Element | null, y: number, init: PointerEventInit = {}) {
    act(() => {
      on?.dispatchEvent(
        new PointerEvent("pointerdown", {
          bubbles: true,
          button: 0,
          pointerId: 1,
          pointerType: "mouse",
          clientY: y,
          ...init,
        }),
      );
    });
  }

  function moveTo(y: number) {
    act(() => {
      document.dispatchEvent(
        new PointerEvent("pointermove", { bubbles: true, pointerId: 1, clientY: y }),
      );
    });
  }

  function release(y: number) {
    act(() => {
      document.dispatchEvent(
        new PointerEvent("pointerup", { bubbles: true, pointerId: 1, clientY: y }),
      );
    });
  }

  const row = (name: string) =>
    host.querySelector<HTMLButtonElement>(
      `button.profile[aria-label="Start a new session with ${name}"]`,
    );
  const flyout = () => document.querySelector<HTMLElement>(".model-menu");
  const flyoutName = () => flyout()?.getAttribute("aria-label") ?? null;

  /** The pointer moves from `from` (null: from outside) onto `to`, at `point`. */
  function pointer(to: Element, from: Element | null, point = { x: 20, y: 20 }) {
    const init = { bubbles: true, pointerType: "mouse", clientX: point.x, clientY: point.y };
    act(() => {
      if (from !== null)
        from.dispatchEvent(new PointerEvent("pointerout", { ...init, relatedTarget: to }));
      to.dispatchEvent(new PointerEvent("pointerover", { ...init, relatedTarget: from }));
      to.dispatchEvent(new PointerEvent("pointermove", init));
    });
  }

  function wait(ms: number) {
    act(() => {
      vi.advanceTimersByTime(ms);
    });
  }

  function hoverOpen(name: string) {
    const button = row(name);
    if (button === null) throw new Error(`no row ${name}`);
    pointer(button, null);
    wait(FLYOUT_OPEN_DELAY_MS);
    return button;
  }

  it("starts with the default from the whole row, named for what it does", () => {
    const { onStart } = render();
    const claude = row("Claude Code");
    expect(claude?.title).toBe(
      "Start a new session with Claude Code\nclaude --verbose\nDrag to change the order (Alt+↑/↓)",
    );
    expect(claude?.textContent).toContain("2.1.0");
    act(() => {
      claude?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
  });

  it("has no pill or plus, and marks a row that has a menu with a small arrow", () => {
    render();
    expect(host.querySelectorAll("svg")).toHaveLength(
      host.querySelectorAll(".profile-more").length,
    );
    expect(host.textContent).not.toContain("›");
    expect(host.textContent).not.toContain("Model");
    expect(host.querySelectorAll("button")).toHaveLength(3);
    expect(row("Claude Code")?.getAttribute("aria-haspopup")).toBe("menu");
    expect(row("Claude Code")?.getAttribute("aria-expanded")).toBe("false");
    expect(row("PowerShell")?.hasAttribute("aria-haspopup")).toBe(false);
    // Only a row with models carries the arrow.
    expect(row("Claude Code")?.querySelector(".profile-more")).not.toBeNull();
    expect(row("PowerShell")?.querySelector(".profile-more")).toBeNull();
    expect(row("PowerShell")?.hasAttribute("aria-expanded")).toBe(false);
  });

  it("opens the flyout once the pointer rests on the row, without taking the focus", () => {
    render();
    const claude = row("Claude Code");
    if (claude === null) throw new Error("no row");
    pointer(claude, null);
    wait(FLYOUT_OPEN_DELAY_MS - 1);
    expect(flyout()).toBeNull();
    wait(1);
    expect(flyoutName()).toBe("Start Claude Code with a model");
    expect(claude.getAttribute("aria-expanded")).toBe("true");
    expect(flyout()?.contains(document.activeElement)).toBe(false);
  });

  it("does not open when the pointer only passes over the row", () => {
    render();
    const claude = row("Claude Code");
    if (claude === null) throw new Error("no row");
    pointer(claude, null);
    wait(FLYOUT_OPEN_DELAY_MS / 2);
    pointer(outside, claude);
    wait(FLYOUT_OPEN_DELAY_MS * 4);
    expect(flyout()).toBeNull();
  });

  it("stays open on the way into the flyout and closes once the pointer leaves both", () => {
    render();
    const claude = hoverOpen("Claude Code");
    // Across the gap between the row and the flyout.
    pointer(outside, claude);
    wait(FLYOUT_GRACE_MS - 50);
    const item = flyout()?.querySelector('[role="menuitem"]');
    if (!item) throw new Error("no model");
    pointer(item, outside);
    wait(FLYOUT_GRACE_MS * 2);
    expect(flyout()).not.toBeNull();
    pointer(outside, item);
    wait(FLYOUT_GRACE_MS - 1);
    expect(flyout()).not.toBeNull();
    wait(1);
    expect(flyout()).toBeNull();
  });

  it("hands over to the next row at once, and closes over a row without models", () => {
    render();
    const claude = hoverOpen("Claude Code");
    const codex = row("Codex");
    const shell = row("PowerShell");
    if (codex === null || shell === null) throw new Error("no row");
    pointer(codex, claude, { x: 20, y: 50 });
    expect(flyoutName()).toBe("Start Codex with a model");
    expect(document.querySelectorAll(".model-menu")).toHaveLength(1);
    pointer(shell, codex, { x: 20, y: 80 });
    expect(flyout()).toBeNull();
    wait(FLYOUT_GRACE_MS * 2);
    expect(flyout()).toBeNull();
  });

  it("lets the pointer cross the next row on its way into the open flyout", () => {
    render();
    const claude = hoverOpen("Claude Code");
    const codex = row("Codex");
    if (codex === null) throw new Error("no row");
    // Only the flyout has a place on screen here; happy-dom lays nothing out.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      return this.classList.contains("model-menu")
        ? DOMRect.fromRect({ x: 300, y: 0, width: 200, height: 400 })
        : new DOMRect();
    });
    pointer(claude, claude, { x: 100, y: 20 });
    // Down and to the right, towards the flyout, over the Codex row.
    pointer(codex, claude, { x: 140, y: 50 });
    expect(flyoutName()).toBe("Start Claude Code with a model");
    // The pointer stays on Codex: it takes over after the grace period.
    wait(FLYOUT_GRACE_MS);
    expect(flyoutName()).toBe("Start Codex with a model");
  });

  it("never opens a flyout for a row without models", () => {
    render();
    hoverOpen("PowerShell");
    wait(FLYOUT_GRACE_MS * 2);
    expect(flyout()).toBeNull();
    const shell = row("PowerShell");
    shell?.focus();
    act(() => {
      shell?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    expect(flyout()).toBeNull();
  });

  it("ignores touch: a tap starts, nothing opens", () => {
    const { onStart } = render();
    const claude = row("Claude Code");
    act(() => {
      claude?.dispatchEvent(
        new PointerEvent("pointermove", { bubbles: true, pointerType: "touch", clientX: 5 }),
      );
    });
    wait(FLYOUT_OPEN_DELAY_MS * 2);
    expect(flyout()).toBeNull();
    act(() => {
      claude?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
  });

  it("opens with Right arrow on the first model and goes back on Left arrow and Escape", () => {
    render();
    const claude = row("Claude Code");
    claude?.focus();
    const key = (target: Element | null, name: string) => {
      act(() => {
        target?.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
      });
    };
    key(claude, "ArrowRight");
    expect(flyout()).not.toBeNull();
    expect(document.activeElement?.textContent).toContain("Opus 5.5");
    key(document.activeElement, "ArrowLeft");
    expect(flyout()).toBeNull();
    expect(document.activeElement).toBe(claude);
    key(claude, "ArrowRight");
    key(document.activeElement, "Escape");
    expect(flyout()).toBeNull();
    expect(document.activeElement).toBe(claude);
  });

  it("gives the focus back to the terminal, not the row, after a flyout the pointer opened", () => {
    render();
    const terminal = document.createElement("textarea");
    outside.append(terminal);
    terminal.focus();
    hoverOpen("Claude Code");
    const item = flyout()?.querySelector('[role="menuitem"]');
    if (!item) throw new Error("no model");
    pointer(item, null);
    expect(flyout()?.contains(document.activeElement)).toBe(true);
    act(() => {
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(flyout()).toBeNull();
    expect(document.activeElement).toBe(terminal);
  });

  it("closes when the rows scroll under it, but not when something else scrolls", () => {
    render();
    const terminal = document.createElement("div");
    outside.append(terminal);
    hoverOpen("Claude Code");
    act(() => {
      terminal.dispatchEvent(new Event("scroll"));
      flyout()?.dispatchEvent(new Event("scroll"));
    });
    expect(flyout()).not.toBeNull();
    act(() => {
      host.dispatchEvent(new Event("scroll"));
    });
    expect(flyout()).toBeNull();
  });

  it("starts with the chosen model", () => {
    const { onStart, onMenuClose } = render();
    hoverOpen("Claude Code");
    const opus = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((item) =>
      item.textContent.includes("Opus 5.5"),
    );
    act(() => {
      opus?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude", { source: "provider", id: "opus" });
    expect(onMenuClose).toHaveBeenCalledOnce();
    expect(flyout()).toBeNull();
  });

  it("moves a row dragged with the mouse, and its release starts nothing", () => {
    const { onStart, onMove } = render();
    layOut();
    const shell = row("PowerShell");
    press(shell, 75);
    moveTo(40);
    // Dragged above Codex's middle: the row follows, Codex steps aside.
    expect(host.querySelector("li.dragging")?.getAttribute("data-program")).toBe("shell");
    expect(host.querySelector<HTMLElement>('li[data-program="codex"]')?.style.transform).toBe(
      "translateY(30px)",
    );
    release(40);
    act(() => {
      shell?.click();
    });
    expect(onMove).toHaveBeenCalledExactlyOnceWith("shell", 1, ["claude", "codex", "shell"]);
    expect(onStart).not.toHaveBeenCalled();
    expect(host.querySelector("li.dragging")).toBeNull();
    expect(document.querySelector('[role="status"]')?.textContent).toContain("position 2 of 3");
    // The next click starts again.
    act(() => {
      shell?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("shell");
  });

  it("treats a press without movement as a click, and Escape ends a drag unsaved", () => {
    const { onStart, onMove } = render();
    layOut();
    const claude = row("Claude Code");
    press(claude, 15);
    moveTo(17);
    release(17);
    act(() => {
      claude?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
    press(claude, 15);
    moveTo(80);
    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    release(80);
    expect(onMove).not.toHaveBeenCalled();
    expect(host.querySelector("li.dragging")).toBeNull();
  });

  it("does not drag with touch or a right press", () => {
    const { onMove } = render();
    layOut();
    press(row("Codex"), 45, { pointerType: "touch" });
    moveTo(5);
    release(5);
    press(row("Codex"), 45, { button: 2 });
    moveTo(5);
    release(5);
    expect(onMove).not.toHaveBeenCalled();
  });

  it("moves a row one place with Alt+Up and Alt+Down", () => {
    const { onMove } = render();
    const codex = row("Codex");
    const key = (name: string, altKey = true) => {
      act(() => {
        codex?.dispatchEvent(new KeyboardEvent("keydown", { key: name, altKey, bubbles: true }));
      });
    };
    key("ArrowUp");
    key("ArrowDown");
    key("ArrowDown", false);
    expect(onMove.mock.calls).toEqual([
      ["codex", 0, ["claude", "codex", "shell"]],
      ["codex", 2, ["claude", "codex", "shell"]],
    ]);
    expect(codex?.title).toContain("Drag to change the order");
  });

  it("starts with the default from the row while its flyout is open", () => {
    const { onStart } = render();
    const claude = hoverOpen("Claude Code");
    act(() => {
      claude.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
    expect(flyout()).toBeNull();
  });

  describe("reasoning efforts", () => {
    const EFFORT_ROWS: readonly RowSpec[] = [
      {
        id: "codex",
        name: "Codex",
        models: modelSections(
          {
            name: "Codex",
            models: [
              {
                id: "gpt-sol",
                label: "GPT Sol",
                efforts: ["low", "high", "ultra"],
                defaultEffort: "low",
              },
              { id: "gpt-plain", label: "GPT Plain" },
            ],
            localSources: [],
          },
          null,
        ),
      },
    ];
    const efforts = () => document.querySelector<HTMLElement>(".popup-submenu");
    const model = (label: string) => {
      const item = [...(flyout()?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])].find(
        (entry) => entry.textContent.includes(label) && efforts()?.contains(entry) !== true,
      );
      if (item === undefined) throw new Error(`no model ${label}`);
      return item;
    };
    const key = (name: string) => {
      act(() => {
        document.activeElement?.dispatchEvent(
          new KeyboardEvent("keydown", { key: name, bubbles: true }),
        );
      });
    };

    it("marks a model with efforts and opens them once the pointer rests on it", () => {
      render(EFFORT_ROWS);
      hoverOpen("Codex");
      const sol = model("GPT Sol");
      expect(sol.getAttribute("aria-haspopup")).toBe("menu");
      expect(sol.querySelector(".menu-hint")?.textContent).toBe("›");
      expect(model("GPT Plain").getAttribute("aria-haspopup")).toBeNull();
      pointer(sol, null);
      wait(SUBMENU_OPEN_DELAY_MS - 1);
      expect(efforts()).toBeNull();
      wait(1);
      expect(efforts()?.getAttribute("aria-label")).toBe("Reasoning effort for GPT Sol");
      expect(sol.getAttribute("aria-expanded")).toBe("true");
      // Only the levels the model takes, in its order; its own default says so quietly.
      expect(
        [...(efforts()?.querySelectorAll('[role="menuitem"]') ?? [])].map(
          (entry) => entry.textContent,
        ),
      ).toEqual(["Low · Default", "High", "Ultra"]);
      // Another model without efforts: the submenu goes after the grace period.
      pointer(model("GPT Plain"), sol);
      wait(SUBMENU_GRACE_MS);
      expect(efforts()).toBeNull();
    });

    it("keeps the submenu while the pointer crosses a neighbor on its way into it", () => {
      render(EFFORT_ROWS);
      hoverOpen("Codex");
      const sol = model("GPT Sol");
      pointer(sol, null);
      wait(SUBMENU_OPEN_DELAY_MS);
      pointer(model("GPT Plain"), sol);
      wait(SUBMENU_GRACE_MS - 50);
      const high = efforts()?.querySelectorAll<HTMLElement>('[role="menuitem"]')[1];
      if (high === undefined) throw new Error("no level");
      pointer(high, null);
      wait(SUBMENU_GRACE_MS * 2);
      expect(efforts()).not.toBeNull();
      expect(document.activeElement).toBe(high);
    });

    it("starts with the picked effort, and the model itself with the program's own", () => {
      const { onStart, onMenuClose } = render(EFFORT_ROWS);
      hoverOpen("Codex");
      const sol = model("GPT Sol");
      pointer(sol, null);
      wait(SUBMENU_OPEN_DELAY_MS);
      const high = efforts()?.querySelectorAll<HTMLElement>('[role="menuitem"]')[1];
      expect(high?.title).toBe("Start Codex with GPT Sol, reasoning effort: High");
      act(() => {
        high?.click();
      });
      expect(onStart).toHaveBeenCalledExactlyOnceWith("codex", {
        source: "provider",
        id: "gpt-sol",
        effort: "high",
      });
      expect(onMenuClose).toHaveBeenCalledOnce();
      expect(flyout()).toBeNull();

      onStart.mockClear();
      hoverOpen("Codex");
      act(() => {
        model("GPT Sol").click();
      });
      expect(onStart).toHaveBeenCalledExactlyOnceWith("codex", {
        source: "provider",
        id: "gpt-sol",
      });
    });

    it("opens with Right arrow, goes back with Left arrow and Escape, starts with Enter", () => {
      const { onStart } = render(EFFORT_ROWS);
      row("Codex")?.focus();
      key("ArrowRight");
      expect(document.activeElement?.textContent).toContain("GPT Sol");
      key("ArrowRight");
      expect(efforts()).not.toBeNull();
      expect(document.activeElement?.textContent).toBe("Low · Default");
      key("ArrowDown");
      expect(document.activeElement?.textContent).toBe("High");
      key("ArrowLeft");
      expect(efforts()).toBeNull();
      expect(flyout()).not.toBeNull();
      expect(document.activeElement?.textContent).toContain("GPT Sol");
      key("ArrowRight");
      key("Escape");
      expect(efforts()).toBeNull();
      expect(document.activeElement?.textContent).toContain("GPT Sol");
      // Down to a model without efforts: Right arrow opens nothing there.
      key("ArrowDown");
      key("ArrowRight");
      expect(efforts()).toBeNull();
      key("ArrowUp");
      key("ArrowRight");
      key("ArrowDown");
      key("ArrowDown");
      key("Enter");
      expect(onStart).toHaveBeenCalledExactlyOnceWith("codex", {
        source: "provider",
        id: "gpt-sol",
        effort: "ultra",
      });
      expect(flyout()).toBeNull();
    });
  });
});
