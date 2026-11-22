/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { SAVING_CLASS } from "./power";

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** Declarations of every rule whose selector list contains `selector`. */
function declarations(selector: string): string[] {
  const found: string[] = [];
  for (const match of CSS.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim().replace(/\s+/g, " "));
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found;
}

const ROOT = `:root.${SAVING_CLASS}`;
/** Leaves the attention settings previews out (styles.css). */
const PREVIEWS = ":not(.look-card *)";

describe("power saving styles", () => {
  it("stills every looping row and chip animation", () => {
    for (const target of [
      `.look .state-dot${PREVIEWS}`,
      `.status-chip-dots span${PREVIEWS}`,
      ".subagent-dot.running",
      ".rail-session.attention .badge::after",
    ]) {
      expect(declarations(`${ROOT} ${target}`).join(";"), target).toMatch(/animation:\s*none/);
    }
    // A row breathes through its own layer, which goes away with the still tint left.
    expect(declarations(`${ROOT} .look${PREVIEWS} .row-pulse`).join(";")).toMatch(
      /display:\s*none/,
    );
  });

  it("keeps the attention settings previews moving", () => {
    expect(declarations(`${ROOT} .look .row-pulse`)).toEqual([]);
    for (const target of [".look .state-dot", ".status-chip-dots span"]) {
      expect(declarations(`${ROOT} ${target}`), target).toEqual([]);
      expect(declarations(`${ROOT} ${target}${PREVIEWS}`).join(";"), target).toMatch(
        /animation:\s*none/,
      );
    }
    expect(declarations(`${ROOT} .session.attention${PREVIEWS}`).join(";")).toMatch(
      /--edge-ring:\s*75%/,
    );
    // The terminal settings' cursor preview is no row; it keeps blinking as chosen.
    expect(CSS.includes(`${ROOT} .terminal-preview-cursor`)).toBe(false);
  });

  it("swaps the sparklines for still bars", () => {
    expect(declarations(`${ROOT} .gauge-spark`).join(";")).toMatch(/display:\s*none/);
    expect(declarations(`${ROOT} .gauge-bar`).join(";")).toMatch(/display:\s*block/);
  });

  it("does not add a second pause for hidden terminal panes", () => {
    const selectors = [...CSS.matchAll(/([^{}]+)\{/g)].map((match) => match[1] ?? "");
    const paused = selectors.filter((list) => list.includes(ROOT) && /\.pane(?![\w-])/.test(list));
    expect(paused).toEqual([]);
  });
});
