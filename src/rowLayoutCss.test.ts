/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { LOOK_STATES } from "./stateLook";

// The row layout's forms and row-wide looks (rowLayout.ts) live in styles.css, "Row layout".
// These tests keep the rules that matter: no new motion, the parts that stand beside the first
// two lines, the state colors, the forms that must fit the narrowest sidebar, and that a layout
// never touches what every row shares.

const RAW = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, "");

/** The row layout's own section of styles.css, comments removed. */
const SECTION = (() => {
  const start = RAW.indexOf("/* --- Row layout");
  const end = RAW.indexOf("/* --- End of row layout --- */");
  if (start < 0 || end < start) throw new Error("row layout section missing");
  return RAW.slice(start, end).replace(/\/\*[\s\S]*?\*\//g, "");
})();

/** Declarations of every rule whose selector list contains `selector`. */
function declarations(selector: string, css = CSS): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim().replace(/\s+/g, " "));
    if (!selectors.includes(selector)) continue;
    for (const part of (match[2] ?? "").split(";")) {
      const [name, ...rest] = part.split(":");
      if (name !== undefined && rest.length > 0) found.set(name.trim(), rest.join(":").trim());
    }
  }
  return found;
}

function rootToken(name: string): string {
  const value = declarations(":root").get(name);
  if (value === undefined) throw new Error(`${name} missing in :root`);
  return value;
}

function px(value: string): number {
  const match = /^(\d+(?:\.\d+)?)px$/.exec(value);
  if (match?.[1] === undefined) throw new Error(`not in px: ${value}`);
  return Number(match[1]);
}

function em(value: string): number {
  const match = /^(\d+(?:\.\d+)?)em$/.exec(value);
  if (match?.[1] === undefined) throw new Error(`not in em: ${value}`);
  return Number(match[1]);
}

/** The narrowest sidebar's text width with the scroll bar (sessionFactsCss.test.ts). */
const NARROWEST_TEXT_PX = 210;

describe("motion", () => {
  it("adds no animation or transition; the stripes stay the only motion, and only while working", () => {
    expect(SECTION).not.toMatch(/animation\s*:/);
    expect(SECTION).not.toMatch(/@keyframes/);
    expect(SECTION).not.toMatch(/transition\s*:/);
  });

  it("keeps the segments still", () => {
    expect(declarations(".session-context-segments > span").get("animation")).toBeUndefined();
    expect(SECTION).not.toMatch(/session-context-segments[^{]*::after/);
  });
});

describe("parts beside the first two lines", () => {
  it("lets the lines below an avatar or ring take the row's whole width", () => {
    expect(declarations(".session-text.has-lead").get("display")).toBe("grid");
    expect(declarations(".session-text > .row-line").get("grid-column")).toBe("1 / -1");
    expect(declarations(".session-text.has-lead > .row-line.beside").get("grid-column")).toBe("2");
    expect(declarations(".row-span").get("grid-row")).toBe("1 / span 2");
    expect(declarations(".row-trail").get("grid-column")).toBe("-2 / -1");
    expect(declarations(".session-text.has-lead.has-trail").get("grid-template-columns")).toBe(
      "auto minmax(0, 1fr) auto",
    );
  });

  it("sizes avatar and ring from tokens", () => {
    for (const name of [
      "--row-avatar",
      "--row-avatar-card",
      "--context-ring",
      "--context-ring-width",
    ]) {
      expect(px(rootToken(name))).toBeGreaterThan(0);
    }
  });
});

describe("state colors", () => {
  it("takes the solid pill's colors from every state look's readable chip pair", () => {
    for (const state of LOOK_STATES) {
      const rule = declarations(`.session.edge-${state}`);
      expect(rule.get("--state-solid")).toBe(`var(--look-${state}-chip-bg)`);
      expect(rule.get("--on-state-solid")).toBe(`var(--look-${state}-chip-fg)`);
    }
  });

  it("keeps the full text color in the tinted pill, also on the active row", () => {
    expect(declarations(".session .session-state.state-pill").get("color")).toBe("var(--text)");
  });

  it("keeps the hover shade under the band", () => {
    expect(declarations('[data-row-state="band"] .session:hover').get("box-shadow")).toContain(
      "var(--row-hover-shade)",
    );
  });
});

describe("fitting the narrowest sidebar", () => {
  it("fits the load's outlined pills", () => {
    const font = px(declarations(".session-load.pills").get("font-size") ?? "");
    const gap = px(declarations(".session-load.pills").get("column-gap") ?? "");
    const pill = declarations(".session-load.pills .session-resource");
    const inner = px(pill.get("gap") ?? "");
    const padding = 2 * px((pill.get("padding") ?? "").split(" ")[1] ?? "");
    const icon = 11;
    const percent = em(rootToken("--fact-percent")) * font;
    const memory = em(rootToken("--fact-memory")) * font;
    const dot = 0.4 * font;
    const cpu = padding + icon + inner + percent;
    const ram = padding + icon + inner + memory;
    const gpu = padding + icon + inner + percent + inner + dot + inner + memory - 4;
    expect(cpu + ram + gpu + 2 * gap).toBeLessThanOrEqual(NARROWEST_TEXT_PX);
  });
});

describe("compact density", () => {
  it("keeps the title line where it is, so the tree's elbow still meets it", () => {
    const main = declarations('[data-row-density="compact"] .session-main');
    expect(main.get("padding-top")).toBeUndefined();
    expect(main.get("padding")).toBeUndefined();
    expect(
      declarations('[data-row-density="compact"] .row-line:not(.session-title-row)').get(
        "line-height",
      ),
    ).toBeDefined();
  });
});

describe("what stays the same in every layout", () => {
  it("never touches the row's frame, margins, tree guides or hover actions' place", () => {
    expect(SECTION).not.toMatch(/tree-guide|agent-rows|subagent|--agent-elbow-y|--tree-/);
    expect(SECTION).not.toMatch(/margin-bottom|border-left|--row-edge:/);
    expect(SECTION).not.toMatch(/\.session-actions\s*\{/);
  });
});

describe("the builder", () => {
  it("lets a chip be dragged with a pen or touch too, and marks the drop place by shape", () => {
    expect(declarations(".row-chip-name").get("touch-action")).toBe("none");
    expect(declarations(".row-builder-slot.drop").get("border-style")).toBe("solid");
    expect(declarations(".row-builder-marker").get("width")).toBe("2px");
  });

  it("frames the chosen chip more firmly, not by color alone", () => {
    expect(declarations(".row-chip.selected").get("box-shadow")).toContain("inset 0 0 0 1px");
  });
});
