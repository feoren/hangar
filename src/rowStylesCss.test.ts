/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { LOOK_STATES } from "./stateLook";

// The row styles (rowStyle.ts) are mostly CSS on the standard row's markup (styles.css, "Row
// styles"). These tests keep the rules that matter: only compositor motion besides the
// accordion's fold, the fold honoring reduced motion, the lines that must fit the narrowest
// sidebar, and the parts that stand beside the first two lines.

const RAW = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, "");

/** The row styles' own section of styles.css, comments removed. */
const SECTION = (() => {
  const start = RAW.indexOf("/* --- Row styles");
  const end = RAW.indexOf("/* --- End of row styles --- */");
  if (start < 0 || end < start) throw new Error("row styles section missing");
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

const REDUCED = [...CSS.matchAll(/@media \(prefers-reduced-motion: reduce\) \{([\s\S]*?)\n\}/g)]
  .map((match) => match[1] ?? "")
  .join("\n");

/** The narrowest sidebar's text width with the scroll bar (sessionFactsCss.test.ts). */
const NARROWEST_TEXT_PX = 210;

describe("motion", () => {
  it("adds no endless animation; the stripes stay the only one, and only while working", () => {
    expect(SECTION).not.toMatch(/animation\s*:/);
    expect(SECTION).not.toMatch(/@keyframes/);
  });

  it("moves only transform and opacity, except the accordion's fold", () => {
    for (const match of SECTION.matchAll(/transition\s*:([^;]*);/g)) {
      const properties = (match[1] ?? "")
        .split(",")
        .map((part) => part.trim().split(/\s+/)[0] ?? "");
      for (const property of properties) {
        expect(["transform", "opacity", "grid-template-rows", "visibility", "none"]).toContain(
          property,
        );
      }
    }
  });

  it("folds the accordion's lines from 0fr to 1fr, hidden while folded", () => {
    expect(declarations(".session-more").get("grid-template-rows")).toBe("0fr");
    // Outside the reduced motion block, which turns it off.
    expect(SECTION).toMatch(/\n\.session-more \{[^}]*transition: grid-template-rows /);
    expect(declarations(".session.expanded .session-more").get("grid-template-rows")).toBe("1fr");
    expect(declarations(".session-more-inner").get("visibility")).toBe("hidden");
    expect(declarations(".session-more-inner").get("overflow")).toBe("hidden");
    expect(declarations(".session.expanded .session-more-inner").get("visibility")).toBe("visible");
  });

  it("folds at once with reduced motion", () => {
    for (const selector of [".session-more", ".session-more-inner", ".session-expander svg"]) {
      expect(declarations(selector, REDUCED).get("transition")).toBe("none");
    }
  });

  it("keeps the segments still", () => {
    expect(declarations(".session-context-segments > span").get("animation")).toBeUndefined();
    expect(SECTION).not.toMatch(/session-context-segments[^{]*::after/);
  });
});

describe("the accordion's chevron", () => {
  it("lies over room the main button leaves for it and shows keyboard focus", () => {
    expect(declarations(".session-expander").get("position")).toBe("absolute");
    expect(
      px(declarations('[data-rows="accordion"] .session-main').get("padding-left") ?? ""),
    ).toBeGreaterThanOrEqual(
      px(declarations(".session-expander").get("left") ?? "") +
        px(declarations(".session-expander").get("width") ?? ""),
    );
    expect(declarations(".session-expander:focus-visible").get("outline")).toContain("solid");
    expect(declarations(".session.expanded .session-expander svg").get("transform")).toBe(
      "rotate(90deg)",
    );
  });
});

describe("parts beside the first two lines", () => {
  it("lets the load line take the row's whole width below an avatar or ring", () => {
    for (const style of ["avatar", "ring", "card"]) {
      expect(declarations(`[data-rows="${style}"] .session-text`).get("display")).toBe("grid");
      expect(declarations(`[data-rows="${style}"] .session-detail`).get("grid-column")).toBe(
        "1 / -1",
      );
      expect(declarations(`[data-rows="${style}"] .session-title-row`).get("grid-column")).toBe(
        "2",
      );
    }
    expect(declarations(".session-avatar").get("grid-row")).toBe("1 / span 2");
    expect(declarations(".session-ring").get("grid-row")).toBe("1 / span 2");
    expect(declarations('[data-rows="card"] .session-ring').get("grid-column")).toBe("3");
  });

  it("moves the run time to a line of its own rather than cutting a value", () => {
    for (const style of ["ring", "card"]) {
      expect(declarations(`[data-rows="${style}"] .session-detail`).get("height")).toBe("auto");
    }
    expect(declarations(".session-detail").get("flex-wrap")).toBe("wrap");
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
      const rule = declarations(`[data-rows="band"] .edge-${state}`);
      expect(rule.get("--state-solid")).toBe(`var(--look-${state}-chip-bg)`);
      expect(rule.get("--on-state-solid")).toBe(`var(--look-${state}-chip-fg)`);
    }
  });

  it("keeps the full text color in the tinted pill, also on the active row", () => {
    expect(declarations('[data-rows="pill"] .session .session-state').get("color")).toBe(
      "var(--text)",
    );
  });

  it("keeps the hover shade under the band", () => {
    expect(declarations('[data-rows="band"] .session:hover').get("box-shadow")).toContain(
      "var(--row-hover-shade)",
    );
  });
});

describe("fitting the narrowest sidebar", () => {
  it("fits the tags style's outlined load pills", () => {
    const font = px(declarations('[data-rows="tags"] .session-detail').get("font-size") ?? "");
    const gap = px(declarations('[data-rows="tags"] .session-detail').get("column-gap") ?? "");
    const pill = declarations('[data-rows="tags"] .session-resource');
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

  it("fits the columns style's title line with the title at its minimum", () => {
    const columns =
      px(rootToken("--column-percent")) +
      px(rootToken("--column-context")) +
      px(rootToken("--column-runtime")) +
      2 * px(declarations(".session-columns").get("gap") ?? "");
    // The widest shape is the working chip's three dots, measured 27 px in Edge.
    const shape = 30 + 5;
    const title = em(declarations('[data-rows="columns"] .session').get("--title-min") ?? "") * 13;
    expect(declarations(".session-columns-head").get("gap")).toBe(
      declarations(".session-columns").get("gap"),
    );
    expect(shape + title + 5 + columns).toBeLessThanOrEqual(NARROWEST_TEXT_PX - 1);
    // "100 %" at 11 px is 29.8 px; the run time cell as wide as the fourth line's.
    expect(px(rootToken("--column-percent"))).toBeGreaterThanOrEqual(29.8);
    expect(px(rootToken("--column-runtime"))).toBeGreaterThanOrEqual(
      em(rootToken("--fact-runtime")) * 11,
    );
  });

  it("names the columns once, aligned with the rows' cells", () => {
    expect(declarations(".session-column-cpu").get("width")).toBe("var(--column-percent)");
    expect(declarations(".session-column-runtime").get("width")).toBe("var(--column-runtime)");
    expect(declarations('[data-rows="columns"] .session-list').get("scrollbar-gutter")).toBe(
      "stable",
    );
    expect(declarations(".session-columns-head").get("scrollbar-gutter")).toBe("stable");
  });
});

describe("what stays the same in every style", () => {
  it("never touches the row's frame, margins, tree guides or hover actions' place", () => {
    expect(SECTION).not.toMatch(/tree-guide|agent-rows|subagent/);
    expect(SECTION).not.toMatch(/margin-bottom|border-left|--row-edge:/);
    expect(SECTION).not.toMatch(/\.session-actions\s*\{/);
  });
});
