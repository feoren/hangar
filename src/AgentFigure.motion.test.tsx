// @vitest-environment happy-dom

// When a bundled figure plays its idle loop: the setting, reduced motion, power saving, a hidden
// window and whether the figure is on screen decide between the animated and the still file.
// The observer, the media query, power saving and the page's visibility are stubbed.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AgentFigure } from "./AgentFigure";
import type { Figure } from "./subAgentLibrary";
import { setFigureMotion } from "./useFigureMotion";

const power = vi.hoisted(() => ({ saving: false, listeners: new Set<() => void>() }));

// Fixed files, so the test holds whichever loops are bundled: "backend-03" has one, "tester-01"
// has none.
const STILL = (id: string) => `/still/${id}.webp`;
const LOOP = "/animated/backend-03.webp";

vi.mock("./figureFiles", () => ({
  builtinFigureUrl: (id: string) => `/still/${id}.webp`,
  animatedFigureUrl: (id: string) =>
    id === "backend-03" ? "/animated/backend-03.webp" : undefined,
}));

vi.mock("./usePower", () => ({
  isPowerSaving: () => power.saving,
  onPowerSavingChange: (listener: () => void) => {
    power.listeners.add(listener);
    return () => {
      power.listeners.delete(listener);
    };
  },
}));

/** Stand-in for IntersectionObserver: the test says which element is on screen. */
class FakeObserver {
  static all = new Set<FakeObserver>();
  readonly targets = new Set<Element>();
  // The figure reads only these two fields of an entry.
  constructor(
    private readonly callback: (entries: { target: Element; isIntersecting: boolean }[]) => void,
  ) {
    FakeObserver.all.add(this);
  }
  observe(target: Element) {
    this.targets.add(target);
  }
  unobserve(target: Element) {
    this.targets.delete(target);
  }
  disconnect() {
    this.targets.clear();
  }
  static watching(): number {
    return [...FakeObserver.all].reduce((sum, observer) => sum + observer.targets.size, 0);
  }
  static show(target: Element, isIntersecting: boolean) {
    for (const observer of FakeObserver.all) {
      if (!observer.targets.has(target)) continue;
      observer.callback([{ target, isIntersecting }]);
    }
  }
}
vi.stubGlobal("IntersectionObserver", FakeObserver);

const reduced = { matches: false, listeners: new Set<() => void>() };
vi.stubGlobal("matchMedia", (query: string) => ({
  get matches() {
    return query.includes("reduced-motion") && reduced.matches;
  },
  addEventListener: (_: string, listener: () => void) => reduced.listeners.add(listener),
  removeEventListener: (_: string, listener: () => void) => reduced.listeners.delete(listener),
}));

let visibility: DocumentVisibilityState = "visible";
Object.defineProperty(document, "visibilityState", {
  configurable: true,
  get: () => visibility,
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

let host: HTMLElement;
let root: Root;

const MOVING: Figure = { kind: "builtin", id: "backend-03" };

function mount(figure: Figure, crop: "full" | "head" = "full") {
  act(() => {
    root.render(<AgentFigure figure={figure} size={88} crop={crop} />);
  });
}

function img(): HTMLImageElement {
  const found = host.querySelector("img");
  if (found === null) throw new Error("no image");
  return found;
}

/** Runs `apply`, then lets the decode before a swap (`useDecodedSource`) finish. */
async function settle(apply: () => void) {
  await act(async () => {
    apply();
    await Promise.resolve();
  });
}

async function onScreen(value: boolean) {
  await settle(() => {
    FakeObserver.show(img(), value);
  });
}

async function change(apply: () => void, listeners: Set<() => void>) {
  await settle(() => {
    apply();
    for (const listener of listeners) listener();
  });
}

beforeEach(() => {
  power.saving = false;
  reduced.matches = false;
  visibility = "visible";
  setFigureMotion("animated");
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

describe("AgentFigure motion", () => {
  it("plays the loop only while the figure is on screen", async () => {
    mount(MOVING);
    // Before the observer answers, the still file shows.
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
    await onScreen(true);
    expect(img().getAttribute("src")).toBe(LOOP);
    await onScreen(false);
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
  });

  it("stands still while the window is hidden and moves again when it shows", async () => {
    mount(MOVING);
    await onScreen(true);
    await settle(() => {
      visibility = "hidden";
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
    // Nothing is watched while figures may not move.
    expect(FakeObserver.watching()).toBe(0);
    await settle(() => {
      visibility = "visible";
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await onScreen(true);
    expect(img().getAttribute("src")).toBe(LOOP);
  });

  it("stands still when the user chose still", async () => {
    mount(MOVING);
    await onScreen(true);
    await settle(() => {
      setFigureMotion("still");
    });
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
  });

  it("stands still under reduced motion, even with animated chosen", async () => {
    mount(MOVING);
    await onScreen(true);
    await change(() => {
      reduced.matches = true;
    }, reduced.listeners);
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
  });

  it("stands still while saving power, even with animated chosen", async () => {
    mount(MOVING);
    await onScreen(true);
    await change(() => {
      power.saving = true;
    }, power.listeners);
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
  });

  it("swaps the file only once the next one is decoded, and also when decoding fails", async () => {
    const pending: { src: string; resolve: () => void; reject: () => void }[] = [];
    const decode = vi.spyOn(HTMLImageElement.prototype, "decode").mockImplementation(function (
      this: HTMLImageElement,
    ) {
      return new Promise<void>((resolve, reject) => {
        pending.push({
          src: this.getAttribute("src") ?? "",
          resolve,
          reject: () => {
            reject(new Error("bad"));
          },
        });
      });
    });
    try {
      mount(MOVING);
      await onScreen(true);
      // The loop is being decoded: the still file stays until it is ready.
      expect(pending.map((entry) => entry.src)).toEqual([LOOP]);
      expect(img().getAttribute("src")).toBe(STILL("backend-03"));
      await settle(() => {
        pending[0]?.resolve();
      });
      expect(img().getAttribute("src")).toBe(LOOP);
      await onScreen(false);
      expect(img().getAttribute("src")).toBe(LOOP);
      await settle(() => {
        pending[1]?.reject();
      });
      expect(img().getAttribute("src")).toBe(STILL("backend-03"));
    } finally {
      decode.mockRestore();
    }
  });

  it("forgets that a figure was on screen while figures stood still", async () => {
    mount(MOVING);
    await onScreen(true);
    await change(() => {
      power.saving = true;
    }, power.listeners);
    // Power saving ends; the observer has not answered for the figure again yet.
    await change(() => {
      power.saving = false;
    }, power.listeners);
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
    await onScreen(true);
    expect(img().getAttribute("src")).toBe(LOOP);
  });

  it("keeps the face crop still", () => {
    mount(MOVING, "head");
    expect(FakeObserver.watching()).toBe(0);
    expect(img().getAttribute("src")).toBe(STILL("backend-03"));
  });

  it("keeps a figure without an animated file still", () => {
    mount({ kind: "builtin", id: "tester-01" });
    expect(FakeObserver.watching()).toBe(0);
    expect(img().getAttribute("src")).toBe(STILL("tester-01"));
  });

  it("shows an own image as it is, never swapped", () => {
    mount({ kind: "image", url: "data:image/webp;base64,AA" });
    expect(FakeObserver.watching()).toBe(0);
    expect(img().getAttribute("src")).toBe("data:image/webp;base64,AA");
  });
});
