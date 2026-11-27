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
    act(() => {
      root.render(
        <I18nContext.Provider value={t}>
          <ProgramList>
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
    return { onStart, onMenuClose };
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
    expect(claude?.title).toBe("Start a new session with Claude Code\nclaude --verbose");
    expect(claude?.textContent).toContain("2.1.0");
    act(() => {
      claude?.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
  });

  it("has no pill, plus or arrow, and says when a row has a menu", () => {
    render();
    expect(host.querySelector("svg")).toBeNull();
    expect(host.textContent).not.toContain("›");
    expect(host.textContent).not.toContain("Model");
    expect(host.querySelectorAll("button")).toHaveLength(3);
    expect(row("Claude Code")?.getAttribute("aria-haspopup")).toBe("menu");
    expect(row("Claude Code")?.getAttribute("aria-expanded")).toBe("false");
    expect(row("PowerShell")?.hasAttribute("aria-haspopup")).toBe(false);
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

  it("starts with the default from the row while its flyout is open", () => {
    const { onStart } = render();
    const claude = hoverOpen("Claude Code");
    act(() => {
      claude.click();
    });
    expect(onStart).toHaveBeenCalledExactlyOnceWith("claude");
    expect(flyout()).toBeNull();
  });
});
