// @vitest-environment happy-dom

// The right panel's slide: the phases, the panel staying mounted until its slide ends, the
// terminals' gate held meanwhile, and no slide with reduced motion or while saving power.

import { act, type RefObject } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SAVING_CLASS } from "./power";
import { PANEL_MOTION_LIMIT_MS, sidePanelMotion } from "./sidebarMotion";
import {
  nextPanelPhase,
  panelMoving,
  useSidePanelMotion,
  type PanelPhase,
} from "./sidePanelMotion";

describe("nextPanelPhase", () => {
  it("slides in and out when motion is allowed", () => {
    expect(nextPanelPhase("closed", "show", true)).toBe("opening");
    expect(nextPanelPhase("opening", "settled", true)).toBe("open");
    expect(nextPanelPhase("open", "hide", true)).toBe("closing");
    expect(nextPanelPhase("closing", "settled", true)).toBe("closed");
  });

  it("comes and goes at once without motion", () => {
    expect(nextPanelPhase("closed", "show", false)).toBe("open");
    expect(nextPanelPhase("open", "hide", false)).toBe("closed");
  });

  it("turns around mid slide", () => {
    expect(nextPanelPhase("opening", "hide", true)).toBe("closing");
    expect(nextPanelPhase("closing", "show", true)).toBe("opening");
  });

  it("ignores repeats and a settle while resting", () => {
    expect(nextPanelPhase("open", "show", true)).toBe("open");
    expect(nextPanelPhase("opening", "show", true)).toBe("opening");
    expect(nextPanelPhase("closing", "hide", true)).toBe("closing");
    expect(nextPanelPhase("closed", "hide", true)).toBe("closed");
    expect(nextPanelPhase("open", "settled", true)).toBe("open");
    expect(nextPanelPhase("closed", "settled", true)).toBe("closed");
  });

  it("counts only the slides as moving", () => {
    const moving = (["closed", "opening", "open", "closing"] as const).filter(panelMoving);
    expect(moving).toEqual(["opening", "closing"]);
  });
});

describe("useSidePanelMotion", () => {
  let host: HTMLDivElement;
  let root: Root;
  let reduced = false;
  let seen: PanelPhase[] = [];
  let frame: RefObject<HTMLElement | null> | null = null;

  function Probe({ shown, animate }: { shown: boolean; animate: boolean }) {
    const { phase, panel } = useSidePanelMotion(shown, animate);
    seen.push(phase);
    frame = panel;
    return phase === "closed" ? null : <aside ref={panel} data-phase={phase} />;
  }

  const render = (shown: boolean, animate = true) => {
    act(() => {
      root.render(<Probe shown={shown} animate={animate} />);
    });
  };
  const aside = () => host.querySelector("aside");
  const end = (propertyName: string) => {
    const element = frame?.current;
    if (!element) throw new Error("no panel frame");
    const event = new Event("transitionend");
    Object.defineProperty(event, "propertyName", { value: propertyName });
    act(() => {
      element.dispatchEvent(event);
    });
  };

  beforeEach(() => {
    Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
    reduced = false;
    seen = [];
    vi.stubGlobal("matchMedia", (query: string): Pick<MediaQueryList, "matches" | "media"> => ({
      matches: reduced && query.includes("reduce"),
      media: query,
    }));
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    document.documentElement.classList.remove(SAVING_CLASS);
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("starts at rest without a slide", () => {
    render(true);
    expect(aside()?.dataset.phase).toBe("open");
    expect(sidePanelMotion.isMoving).toBe(false);
  });

  it("mounts sliding in, holds the fits and lets them run once at the end", () => {
    render(false);
    expect(aside()).toBeNull();
    render(true);
    expect(aside()?.dataset.phase).toBe("opening");
    expect(sidePanelMotion.isMoving).toBe(true);
    const fit = vi.fn();
    sidePanelMotion.whenSettled(fit);
    sidePanelMotion.whenSettled(fit);
    end("opacity");
    expect(fit).not.toHaveBeenCalled();
    end("width");
    expect(aside()?.dataset.phase).toBe("open");
    expect(sidePanelMotion.isMoving).toBe(false);
    expect(fit).toHaveBeenCalledTimes(1);
  });

  it("stays mounted while sliding out and unmounts when the slide ends", () => {
    render(true);
    render(false);
    expect(aside()?.dataset.phase).toBe("closing");
    expect(sidePanelMotion.isMoving).toBe(true);
    end("transform");
    expect(aside()).toBeNull();
    expect(sidePanelMotion.isMoving).toBe(false);
  });

  it("settles anyway when no transition ends", () => {
    vi.useFakeTimers();
    render(true);
    render(false);
    act(() => {
      vi.advanceTimersByTime(PANEL_MOTION_LIMIT_MS);
    });
    expect(aside()).toBeNull();
    expect(sidePanelMotion.isMoving).toBe(false);
  });

  it("comes and goes at once with reduced motion", () => {
    reduced = true;
    render(false);
    render(true);
    expect(aside()?.dataset.phase).toBe("open");
    render(false);
    expect(aside()).toBeNull();
    expect(seen).not.toContain("opening");
    expect(seen).not.toContain("closing");
  });

  it("comes and goes at once while saving power", () => {
    document.documentElement.classList.add(SAVING_CLASS);
    render(true);
    render(false);
    expect(aside()).toBeNull();
    expect(sidePanelMotion.isMoving).toBe(false);
  });

  it("leaves at once when told not to animate (detached)", () => {
    render(true);
    render(false, false);
    expect(aside()).toBeNull();
    expect(sidePanelMotion.isMoving).toBe(false);
  });
});
