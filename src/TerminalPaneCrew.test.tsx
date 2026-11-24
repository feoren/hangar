// @vitest-environment happy-dom

// The crew's form follows the pane's width: below 560 px the strip above the terminal, from 560 px
// on the column beside it. The pane is rendered with a fake terminal and a stand-in crew; the
// ResizeObserver is replaced by one the test drives.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CREW_COLUMN_MIN_PANE } from "./crew";
import { SessionTerminal } from "./terminal";
import { TerminalPane } from "./TerminalPane";

vi.mock("./CrewColumn", () => ({
  CrewColumn: ({ form }: { form: string }) => <aside data-testid="crew" data-form={form} />,
}));

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

class FakeObserver {
  static all: FakeObserver[] = [];
  disconnected = false;
  constructor(private readonly callback: (entries: { contentRect: { width: number } }[]) => void) {
    FakeObserver.all.push(this);
  }
  observe() {}
  unobserve() {}
  disconnect() {
    this.disconnected = true;
  }
  fire(width: number) {
    this.callback([{ contentRect: { width } }]);
  }
}

/** Only what the pane calls; the real class needs xterm and the backend. */
function fakeTerminal(): SessionTerminal {
  const blank: unknown = Object.create(SessionTerminal.prototype);
  if (!(blank instanceof SessionTerminal)) throw new Error("prototype is not a terminal");
  return Object.assign(blank, {
    mount: vi.fn(),
    fit: vi.fn(),
    focus: vi.fn(),
    subscribeLastPrompt: () => () => undefined,
    lastPrompt: () => null,
    revealLastPrompt: vi.fn(),
  });
}

const terminal = fakeTerminal();

const crew = {
  sessionId: 1,
  sessionTitle: "Main",
  cwd: null,
  watchKey: null,
  hidden: new Set<string>(),
  sessionWaits: false,
  picked: null,
  onShow: () => undefined,
  onHide: () => undefined,
};

let host: HTMLElement;
let root: Root | null;
const original: unknown = Reflect.get(globalThis, "ResizeObserver");

function mount(withCrew: boolean) {
  root = createRoot(host);
  act(() => {
    root?.render(
      <TerminalPane
        terminal={terminal}
        sessionId={1}
        active
        visible
        split={null}
        dropping={false}
        promptBar
        onFocusPane={() => undefined}
        onMaximize={() => undefined}
        onHeaderMenu={() => undefined}
        crew={withCrew ? crew : null}
      />,
    );
  });
}

/** Reports `width` to every observer of the pane (the fit observer ignores the entry). */
function resize(width: number) {
  act(() => {
    for (const observer of FakeObserver.all) observer.fire(width);
  });
}

function form(): string | null {
  return host.querySelector("[data-testid=crew]")?.getAttribute("data-form") ?? null;
}

beforeEach(() => {
  FakeObserver.all = [];
  Reflect.set(globalThis, "ResizeObserver", FakeObserver);
  host = document.createElement("div");
  document.body.appendChild(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  Reflect.set(globalThis, "ResizeObserver", original);
});

describe("TerminalPane crew form", () => {
  it("has the documented threshold", () => {
    expect(CREW_COLUMN_MIN_PANE).toBe(560);
  });

  it("shows the column before the pane is measured", () => {
    mount(true);
    expect(form()).toBe("column");
    expect(host.querySelector(".pane-row [data-testid=crew]")).not.toBeNull();
  });

  it("switches to the strip one pixel below the threshold", () => {
    mount(true);
    resize(559);
    expect(form()).toBe("strip");
    // The strip stands above the terminal row, not inside it.
    expect(host.querySelector(".pane-row [data-testid=crew]")).toBeNull();
    expect(host.querySelectorAll("[data-testid=crew]")).toHaveLength(1);
  });

  it("takes the column at exactly 560 and above", () => {
    mount(true);
    resize(559);
    expect(form()).toBe("strip");
    resize(560);
    expect(form()).toBe("column");
    resize(1200);
    expect(form()).toBe("column");
  });

  it("ignores a zero width from a hidden pane", () => {
    mount(true);
    resize(300);
    expect(form()).toBe("strip");
    resize(0);
    expect(form()).toBe("strip");
  });

  it("renders no crew without one", () => {
    mount(false);
    resize(300);
    expect(form()).toBeNull();
  });

  it("disconnects its observers on unmount", () => {
    mount(true);
    act(() => {
      root?.unmount();
    });
    root = null;
    expect(FakeObserver.all.every((observer) => observer.disconnected)).toBe(true);
  });
});
