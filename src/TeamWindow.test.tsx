// @vitest-environment happy-dom

// The Team window resizes like the Settings dialog: grips on its edges, the size from the
// shared hook and remembered under its own key. The account and team stores are stubbed with an
// empty state (no account view yet), which renders the window frame without a body.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./useAccount", () => ({
  loadProjects: vi.fn(),
  useAccount: () => ({
    projects: null,
    projectsError: null,
    loadingProjects: false,
    view: null,
    unavailable: false,
  }),
}));
vi.mock("./useTeam", () => ({
  useTeam: () => ({ activities: [], pending: [], statuses: new Map() }),
}));

import { TeamWindow } from "./TeamWindow";

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  Object.defineProperty(window, "innerWidth", { value: 1400, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: 900, configurable: true });
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  host.remove();
});

function mount() {
  root = createRoot(host);
  act(() => {
    root?.render(<TeamWindow sessions={[]} onClose={() => undefined} />);
  });
  const panel = host.querySelector<HTMLElement>(".team-window");
  if (panel === null) throw new Error("no team window");
  return panel;
}

describe("TeamWindow", () => {
  it("has resize grips and opens at the standard size", () => {
    const panel = mount();
    expect(panel.classList.contains("resizable")).toBe(true);
    expect(panel.querySelectorAll(".dialog-resize").length).toBeGreaterThan(0);
    expect(panel.style.width).toBe("936px");
    expect(panel.style.height).toBe("672px");
  });

  it("opens at the size the user chose last time", () => {
    localStorage.setItem("hangar.teamWindowSize", JSON.stringify({ width: 700, height: 500 }));
    const panel = mount();
    expect(panel.style.width).toBe("700px");
    expect(panel.style.height).toBe("500px");
  });
});
