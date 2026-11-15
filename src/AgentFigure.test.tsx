/// <reference types="node" />
import { readFileSync } from "node:fs";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AgentFigure, BUNDLED_FIGURES, builtinFigureUrl } from "./AgentFigure";
import { BUILTIN_FIGURES } from "./subAgentLibrary";

describe("AgentFigure", () => {
  it("bundles exactly the built-in figures", () => {
    expect(BUNDLED_FIGURES).toEqual([...BUILTIN_FIGURES]);
    for (const id of BUILTIN_FIGURES) expect(builtinFigureUrl(id)).toBeTruthy();
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
