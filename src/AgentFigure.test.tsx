/// <reference types="node" />
import { readFileSync } from "node:fs";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { AgentFigure, useCrewFigure } from "./AgentFigure";
import { ANIMATED_FIGURES, BUNDLED_FIGURES, builtinFigureUrl } from "./figureFiles";
import {
  BUILTIN_FIGURES,
  defaultFigure,
  figureTop,
  type Figure,
  type FigureView,
} from "./subAgentLibrary";

const store = vi.hoisted(() => {
  const state: { read: readonly string[] | null } = { read: null };
  return state;
});

vi.mock("./subAgentLibraryStore", async () => {
  const { EMPTY_LIBRARY: empty } = await import("./subAgentLibrary");
  const figures: ReadonlyMap<string, FigureView> = new Map([
    ["hangar-tester", { kind: "builtin", id: "security-02" }],
  ]);
  return {
    useSubAgentLibrary: () => ({ library: empty, figures, error: null, read: store.read }),
  };
});

describe("useCrewFigure", () => {
  function crewFigure(name: string, cwd: string): Figure | null {
    let seen: Figure | null = null;
    function Probe() {
      seen = useCrewFigure(name, cwd);
      return null;
    }
    renderToStaticMarkup(<Probe />);
    return seen;
  }

  it("keeps the chosen figure off for an unknown name in a folder the listing does not cover", () => {
    store.read = null;
    expect(crewFigure("hangar-tester", "C:\\src\\hangar")).toEqual(
      defaultFigure("hangar-tester", null),
    );
    store.read = ["C:\\src\\other"];
    expect(crewFigure("hangar-tester", "C:\\src\\hangar")).toEqual(
      defaultFigure("hangar-tester", null),
    );
    // Covered, the name's chosen figure counts as before.
    store.read = ["C:\\src\\hangar"];
    expect(crewFigure("hangar-tester", "C:\\src\\hangar")).toEqual({
      kind: "builtin",
      id: "security-02",
    });
  });
});

describe("AgentFigure", () => {
  it("bundles exactly the built-in figures", () => {
    expect(BUNDLED_FIGURES).toEqual([...BUILTIN_FIGURES]);
    for (const id of BUILTIN_FIGURES) expect(builtinFigureUrl(id)).toBeTruthy();
  });

  it("bundles idle loops only under the names of built-in figures", () => {
    for (const id of ANIMATED_FIGURES) expect(BUILTIN_FIGURES).toContain(id);
  });

  it("lifts a wide figure's face into the round box, only there", () => {
    const head = (id: string) =>
      renderToStaticMarkup(<AgentFigure figure={{ kind: "builtin", id }} size={26} crop="head" />);
    expect(head("sysadmin-01")).toContain("--figure-lift:65");
    expect(head("tester-01")).not.toContain("--figure-lift");
    const full = renderToStaticMarkup(
      <AgentFigure figure={{ kind: "builtin", id: "sysadmin-01" }} size={48} />,
    );
    expect(full).not.toContain("--figure-lift");
    expect(figureTop("comfyui-01")).toBe(56);
    expect(figureTop("backend-01")).toBe(2);
    expect(figureTop("no-such-figure")).toBe(2);
  });

  it("draws a bundled figure, an own image or an initial", () => {
    const builtin = renderToStaticMarkup(
      <AgentFigure figure={{ kind: "builtin", id: "tester-01" }} size={28} crop="head" />,
    );
    expect(builtin).toContain('class="agent-figure head"');
    expect(builtin).toContain('aria-hidden="true"');
    expect(builtin).toContain("<img");

    const image = renderToStaticMarkup(
      <AgentFigure
        figure={{ kind: "image", url: "data:image/png;base64,AA" }}
        size={56}
        label="backend"
      />,
    );
    expect(image).toContain('src="data:image/png;base64,AA"');
    expect(image).toContain('aria-label="backend"');

    // The face shows the initial on its color; the whole figure is the drawn visitor in that
    // color with the initial on its badge, no capsule.
    const face = renderToStaticMarkup(
      <AgentFigure
        figure={{ kind: "initial", letter: "W", color: "#8b5cc8" }}
        size={20}
        crop="head"
      />,
    );
    expect(face).toContain(">W<");
    expect(face).toContain("background:#8b5cc8");

    const whole = renderToStaticMarkup(
      <AgentFigure figure={{ kind: "initial", letter: "W", color: "#8b5cc8" }} size={60} />,
    );
    expect(whole).toContain("agent-figure full guest");
    expect(whole).toContain("<svg");
    expect(whole).toContain(">W</text>");
    expect(whole).toContain("fill:#8b5cc8");
    expect(whole).not.toContain("background:#8b5cc8");
  });

  // The idle loop of a bundled figure is its own animated WebP (figureMotion.ts), not CSS; no
  // CSS moves a figure.
  it("stands still: no figure moves, the crew's stage only changes color", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
      /\/\*[\s\S]*?\*\//g,
      "",
    );
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => ({
      selector: (match[1] ?? "").trim(),
      body: match[2] ?? "",
    }));
    expect(css).not.toMatch(/@keyframes figure-/);
    const moves = /animation|transition|transform|(^|[;\s])(translate|rotate|scale)\s*:/;
    // No figure moves anywhere, not even on hover: no animation, transition or transform.
    for (const rule of rules.filter((r) => r.selector.includes(".agent-figure"))) {
      expect(rule.body, rule.selector).not.toMatch(moves);
    }
    // In the crew only the bubble's tail is turned (a static shape); nothing else is transformed,
    // so neither a figure nor its stage or card can be moved from an ancestor.
    const transformed = rules
      .filter((r) => r.selector.includes(".crew-"))
      .filter((r) => /transform\s*:|(^|[;\s])(translate|rotate|scale)\s*:/.test(r.body))
      .map((r) => r.selector);
    expect(transformed).toEqual([".crew-bubble::after"]);
    const tail = rules.find((r) => r.selector === ".crew-bubble::after")?.body ?? "";
    expect(tail).toMatch(/transform:\s*rotate\(45deg\);/);
    expect(tail).not.toMatch(/animation|transition:[^;]*transform/);
    // The crew's only loops: typing dots and the waiting glow, plus a new text rising in.
    const crewAnimations = rules
      .filter((r) => r.selector.includes(".crew-"))
      .flatMap((r) => [...r.body.matchAll(/animation:\s*([\w-]+)/g)].map((m) => m[1]));
    expect(new Set(crewAnimations)).toEqual(
      new Set(["crew-say-in", "dots-fade", "crew-wait-glow", "none"]),
    );
  });
});
