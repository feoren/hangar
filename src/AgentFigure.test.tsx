/// <reference types="node" />
import { readFileSync } from "node:fs";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AgentFigure, BUNDLED_FIGURES, builtinFigureUrl } from "./AgentFigure";
import { SAVING_CLASS } from "./power";
import { BUILTIN_FIGURES } from "./subAgentLibrary";

describe("AgentFigure", () => {
  it("bundles exactly the built-in figures", () => {
    expect(BUNDLED_FIGURES).toEqual([...BUILTIN_FIGURES]);
    for (const id of BUILTIN_FIGURES) expect(builtinFigureUrl(id)).toBeTruthy();
  });

  it("draws a bundled figure, an own image or an initial", () => {
    const builtin = renderToStaticMarkup(
      <AgentFigure figure={{ kind: "builtin", id: "tester-01" }} size={28} crop="head" busy />,
    );
    expect(builtin).toContain('class="agent-figure head busy"');
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

  it("stills its motion for reduced motion and in the power saving mode", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
      /\/\*[\s\S]*?\*\//g,
      "",
    );
    expect(css).toMatch(
      /prefers-reduced-motion: reduce\)\s*\{\s*\.agent-figure img,\s*\.agent-figure\.busy img\s*\{\s*animation: none;/,
    );
    // The busy rule is more specific than `.agent-figure img`: it must be stilled by name.
    expect(css).toMatch(
      new RegExp(
        `:root\\.${SAVING_CLASS} \\.agent-figure img,\\s*:root\\.${SAVING_CLASS} \\.agent-figure\\.busy img\\s*\\{\\s*animation: none;`,
      ),
    );
    // Only transform moves: the compositor does it without layout or paint.
    const frames = [...css.matchAll(/@keyframes figure-[a-z]+\s*\{([\s\S]*?\}\s*)\}/g)];
    expect(frames.length).toBe(2);
    for (const frame of frames) {
      const properties = [...(frame[1] ?? "").matchAll(/([a-z-]+)\s*:/g)].map((m) => m[1]);
      expect(properties.every((name) => name === "transform")).toBe(true);
    }
  });
});
