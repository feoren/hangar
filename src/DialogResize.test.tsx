// @vitest-environment happy-dom

// Component test for the resizable dialog: `useDialogSize` + `DialogResizeHandles` in a small
// harness that stands in for the settings dialog. The pure rules live in dialogSize.test.ts; this
// file covers the wiring: pointer drag, keyboard, reset, unmount mid-drag, clamping, storage.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DialogResizeHandles, useDialogSize, type DialogSizeBounds } from "./DialogResize";

const KEY = "test.dialogSize";
const BOUNDS: DialogSizeBounds = {
  storageKey: KEY,
  standard: { width: 800, height: 600 },
  min: { width: 400, height: 300 },
};

function Harness() {
  const state = useDialogSize(BOUNDS);
  return (
    <div className="dialog" data-testid="dialog" style={state.style}>
      <DialogResizeHandles state={state} />
    </div>
  );
}

let host: HTMLElement;
let root: Root | null;
let frames: Map<number, FrameRequestCallback>;
let nextFrame: number;
let cancelled: number[];

function setViewport(width: number, height: number) {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: height, configurable: true });
}

function mount() {
  root = createRoot(host);
  act(() => {
    root?.render(<Harness />);
  });
}

function unmount() {
  act(() => {
    root?.unmount();
  });
  root = null;
}

function dialog(): HTMLElement {
  const element = host.querySelector<HTMLElement>(".dialog");
  if (!element) throw new Error("dialog is not rendered");
  return element;
}

function handle(edge: string): HTMLElement {
  const element = host.querySelector<HTMLElement>(`.dialog-resize.${edge}`);
  if (!element) throw new Error(`handle ${edge} is not rendered`);
  return element;
}

function pointer(target: HTMLElement, type: string, clientX: number, clientY = 0) {
  act(() => {
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
        clientX,
        clientY,
      }),
    );
  });
}

function press(target: HTMLElement, key: string) {
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
}

/** Runs every queued animation frame, as the browser would before the next paint. */
function flushFrames() {
  const pending = [...frames.values()];
  frames.clear();
  for (const callback of pending) callback(0);
}

beforeEach(() => {
  Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  setViewport(1200, 900);
  host = document.createElement("div");
  document.body.appendChild(host);
  frames = new Map();
  nextFrame = 1;
  cancelled = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const id = nextFrame++;
    frames.set(id, callback);
    return id;
  });
  // Cancelling is only recorded: a frame that was already queued still gets run by the tests, so
  // they prove the component itself ignores a late frame.
  vi.stubGlobal("cancelAnimationFrame", (id: number) => {
    cancelled.push(id);
  });
  // Pointer capture is not implemented by the DOM stand-in; the component only calls it.
  Element.prototype.setPointerCapture = () => {};
  Element.prototype.releasePointerCapture = () => {};
  root = null;
});

afterEach(() => {
  if (root) unmount();
  host.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("DialogResizeHandles", () => {
  it("opens at the standard size without a stored one", () => {
    mount();
    expect(dialog().style.width).toBe("800px");
    expect(dialog().style.height).toBe("600px");
    expect(host.querySelectorAll(".dialog-resize")).toHaveLength(8);
  });

  it("grows symmetrically when the east edge is dragged and stores only on pointerup", () => {
    mount();
    const east = handle("e");
    pointer(east, "pointerdown", 100);
    pointer(east, "pointermove", 110);
    flushFrames();

    // 10 px of pointer travel is 20 px of width: the dialog grows to both sides.
    expect(dialog().style.width).toBe("820px");
    expect(dialog().style.height).toBe("600px");
    expect(localStorage.getItem(KEY)).toBeNull();

    pointer(east, "pointerup", 110);
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual({ width: 820, height: 600 });
    expect(dialog().style.width).toBe("820px");
  });

  it("updates at most once per frame however many moves arrive", () => {
    mount();
    const east = handle("e");
    pointer(east, "pointerdown", 100);
    pointer(east, "pointermove", 105);
    pointer(east, "pointermove", 110);
    pointer(east, "pointermove", 120);
    expect(frames.size).toBe(1);
    flushFrames();
    expect(dialog().style.width).toBe("840px");
  });

  it("stores nothing for a press without any movement", () => {
    mount();
    const east = handle("e");
    pointer(east, "pointerdown", 100);
    pointer(east, "pointerup", 100);
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("ignores a press with another button than the primary one", () => {
    mount();
    const east = handle("e");
    act(() => {
      east.dispatchEvent(
        new PointerEvent("pointerdown", {
          bubbles: true,
          cancelable: true,
          button: 2,
          clientX: 100,
        }),
      );
    });
    pointer(east, "pointermove", 150);
    flushFrames();
    pointer(east, "pointerup", 150);
    expect(dialog().style.width).toBe("800px");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("stops at the minimum and at the window limit while dragging", () => {
    mount();
    const east = handle("e");
    pointer(east, "pointerdown", 100);
    pointer(east, "pointermove", -1000);
    flushFrames();
    expect(dialog().style.width).toBe("400px");
    pointer(east, "pointermove", 5000);
    flushFrames();
    // 1200 px window less 16 px margin on each side.
    expect(dialog().style.width).toBe("1168px");
    pointer(east, "pointerup", 5000);
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual({ width: 1168, height: 600 });
  });

  it("grows the width by 16 px for ArrowRight on the focusable se handle and updates the ARIA values", () => {
    mount();
    const corner = handle("se");
    expect(corner.getAttribute("tabindex")).toBe("0");
    expect(handle("e").hasAttribute("tabindex")).toBe(false);
    expect(corner.getAttribute("aria-valuenow")).toBe("800");

    press(corner, "ArrowRight");

    expect(dialog().style.width).toBe("816px");
    expect(corner.getAttribute("aria-valuenow")).toBe("816");
    expect(corner.getAttribute("aria-valuetext")).toBe("816 × 600 px");
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual({ width: 816, height: 600 });
  });

  it("ignores keys other than the arrows", () => {
    mount();
    press(handle("se"), "a");
    expect(dialog().style.width).toBe("800px");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("restores the standard size and forgets the stored one on a double click", () => {
    localStorage.setItem(KEY, JSON.stringify({ width: 900, height: 700 }));
    mount();
    expect(dialog().style.width).toBe("900px");

    act(() => {
      handle("w").dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true }));
    });

    expect(dialog().style.width).toBe("800px");
    expect(dialog().style.height).toBe("600px");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("writes no frame after the dialog was closed in the middle of a drag", () => {
    mount();
    const east = handle("e");
    const element = dialog();
    pointer(east, "pointerdown", 100);
    pointer(east, "pointermove", 110);
    expect(frames.size).toBe(1);
    const [queued] = [...frames.keys()];

    unmount();
    expect(cancelled).toContain(queued);

    // Even a frame that the browser had already scheduled must not touch the detached dialog.
    expect(() => {
      flushFrames();
    }).not.toThrow();
    expect(element.style.width).toBe("800px");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("clamps a stored size larger than the window and gives it back when the window grows", () => {
    localStorage.setItem(KEY, JSON.stringify({ width: 2000, height: 1500 }));
    mount();
    expect(dialog().style.width).toBe("1168px");
    expect(dialog().style.height).toBe("868px");

    setViewport(2400, 1800);
    act(() => {
      window.dispatchEvent(new Event("resize"));
    });

    expect(dialog().style.width).toBe("2000px");
    expect(dialog().style.height).toBe("1500px");
    // The stored preference itself was never overwritten by the clamped value.
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual({ width: 2000, height: 1500 });
  });

  it("falls back to the standard size for a malformed stored value", () => {
    localStorage.setItem(KEY, "{not json");
    mount();
    expect(dialog().style.width).toBe("800px");
  });

  it("keeps working when localStorage throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage denied");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("storage denied");
    });

    mount();
    expect(dialog().style.width).toBe("800px");

    const east = handle("e");
    pointer(east, "pointerdown", 100);
    pointer(east, "pointermove", 110);
    flushFrames();
    pointer(east, "pointerup", 110);
    expect(dialog().style.width).toBe("820px");

    press(handle("se"), "ArrowRight");
    expect(dialog().style.width).toBe("836px");

    act(() => {
      handle("n").dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true }));
    });
    expect(dialog().style.width).toBe("800px");
  });
});
