/// <reference types="node" />
import { readFileSync } from "node:fs";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AgentFigure, BUNDLED_FIGURES, builtinFigureUrl } from "./AgentFigure";
import { BUILTIN_FIGURES, figureTop } from "./subAgentLibrary";

describe("AgentFigure", () => {
  it("bundles exactly the built-in figures", () => {
    expect(BUNDLED_FIGURES).toEqual([...BUILTIN_FIGURES]);
    for (const id of BUILTIN_FIGURES) expect(builtinFigureUrl(id)).toBeTruthy();
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

    const initial = renderToStaticMarkup(
      <AgentFigure figure={{ kind: "initial", letter: "W", color: "#8b5cc8" }} size={20} />,
    );
    expect(initial).toContain(">W<");
    expect(initial).toContain("background:#8b5cc8");
  });

  it("stands still: no animation on a figure, the crew's plinth only changes color", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
      /\/\*[\s\S]*?\*\//g,
      "",
    );
    expect(css).not.toMatch(/@keyframes figure-/);
    expect(css).not.toMatch(/^\.agent-figure[^{]*\{[^}]*animation/m);
    expect(css).not.toMatch(/\.crew-[a-z-]+[^{]*\{[^}]*(animation|transform:|translate)/);
  });
});
