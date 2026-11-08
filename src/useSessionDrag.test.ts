// The click guard of a session drag: after a cancelled drag, the release and its click must not
// activate the row, but a keyboard click (detail 0) always passes. Runs in Node with small DOM
// stand-ins built on the platform's EventTarget, so no DOM library is needed.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SessionDrag, type DragHandle, type DragPress } from "./useSessionDrag";

class PointerLikeEvent extends Event {
  readonly pointerId: number;
  readonly clientX: number;
  readonly clientY: number;
  constructor(type: string, init: { pointerId?: number; clientX?: number; clientY?: number }) {
    super(type, { cancelable: true });
    this.pointerId = init.pointerId ?? 1;
    this.clientX = init.clientX ?? 0;
    this.clientY = init.clientY ?? 0;
  }
}

class ClickEvent extends Event {
  readonly detail: number;
  constructor(detail: number) {
    super("click", { cancelable: true });
    this.detail = detail;
  }
}

class KeyEvent extends Event {
  readonly key: string;
  constructor(key: string) {
    super("keydown", { cancelable: true });
    this.key = key;
  }
}

class Row extends EventTarget implements DragHandle {
  private captured = new Set<number>();
  setPointerCapture(pointerId: number) {
    this.captured.add(pointerId);
  }
  hasPointerCapture(pointerId: number) {
    return this.captured.has(pointerId);
  }
  releasePointerCapture(pointerId: number) {
    this.captured.delete(pointerId);
  }
}

let win: EventTarget;

/** A click as the browser fires it at the window first (capture): `false` if it was swallowed. */
function click(detail: number): boolean {
  return win.dispatchEvent(new ClickEvent(detail));
}

function press(row: Row): DragPress {
  return { button: 0, isPrimary: true, pointerId: 1, clientX: 10, clientY: 10, currentTarget: row };
}

beforeEach(() => {
  vi.useFakeTimers();
  win = new EventTarget();
  vi.stubGlobal(
    "window",
    Object.assign(win, {
      setTimeout: (handler: () => void, ms?: number) => setTimeout(handler, ms),
    }),
  );
  vi.stubGlobal("document", {
    body: { classList: { add: vi.fn(), remove: vi.fn() } },
    querySelector: () => null,
    elementFromPoint: () => null,
  });
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function startDrag() {
  const row = new Row();
  const views: unknown[] = [];
  const drag = new SessionDrag(
    { layout: "single", order: [7], onMove: vi.fn(), onPlace: vi.fn() },
    (view) => views.push(view),
  );
  drag.begin(7, press(row));
  // Past the 5 px threshold: now it is a drag.
  row.dispatchEvent(new PointerLikeEvent("pointermove", { clientX: 40, clientY: 40 }));
  return { row, views };
}

describe("SessionDrag click guard", () => {
  it("swallows the click of the release that follows an Escape cancel", () => {
    const { row, views } = startDrag();
    expect(views.at(-1)).toMatchObject({ id: 7 });
    win.dispatchEvent(new KeyEvent("Escape"));
    expect(views.at(-1)).toBeNull();
    expect(row.hasPointerCapture(1)).toBe(false);

    // The button is released afterwards; its click must not activate the row.
    win.dispatchEvent(new PointerLikeEvent("pointerup", {}));
    expect(click(1)).toBe(false);
    // The guard ends right after that release.
    vi.runAllTimers();
    expect(click(1)).toBe(true);
  });

  it("lets a keyboard click (detail 0) through while the guard is up", () => {
    startDrag();
    win.dispatchEvent(new KeyEvent("Escape"));
    expect(click(0)).toBe(true);
    expect(click(1)).toBe(false);
  });

  it("ends the guard at the next press when the release happened outside the window", () => {
    startDrag();
    win.dispatchEvent(new KeyEvent("Escape"));
    win.dispatchEvent(new PointerLikeEvent("pointerdown", {}));
    expect(click(1)).toBe(true);
  });

  it("swallows only the drop's own click after a committed drag", () => {
    const { row } = startDrag();
    row.dispatchEvent(new PointerLikeEvent("pointerup", { clientX: 40, clientY: 40 }));
    expect(click(1)).toBe(false);
    vi.runAllTimers();
    expect(click(1)).toBe(true);
  });

  it("does not guard a plain click without a drag", () => {
    const row = new Row();
    const drag = new SessionDrag(
      { layout: "single", order: [7], onMove: vi.fn(), onPlace: vi.fn() },
      vi.fn(),
    );
    drag.begin(7, press(row));
    row.dispatchEvent(new PointerLikeEvent("pointerup", {}));
    expect(click(1)).toBe(true);
  });
});
