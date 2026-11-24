/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// A session row has up to four lines: title and state; folder and branch; the load; run time and
// context (styles.css, "First line" to "Fourth line"). Values sit in columns of fixed width
// that line up from row to row. These tests keep the cells, the lines and the stripes consistent.

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** Font size of the load line; the --fact-* tokens are in em of it. */
const LINE_PX = 11;
/** The icons for CPU, RAM and GPU (SessionLoad.tsx). */
const ICON_PX = 11;
/** After an icon and around the GPU's dot. */
const NEAR_PX = 4;
const CELL_GAP_PX = 6;
/** The narrowest sidebar's text width (305 px sidebar) with the list's scroll bar, measured in
 * Edge at 125 %. */
const NARROWEST_TEXT_PX = 210;

/** Declarations of every top level rule whose selector list contains `selector`. */
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

/** A --fact-* token in em from :root. */
function token(name: string): number {
  const value = declarations(":root").get(name);
  const em = /^(\d+(?:\.\d+)?)em$/.exec(value ?? "");
  if (em?.[1] === undefined) throw new Error(`${name} is not in em: ${String(value)}`);
  return Number(em[1]);
}

describe("cells of fixed width", () => {
  it("gives every value its cell from a token in :root", () => {
    expect(declarations(".session-value-percent").get("min-width")).toBe("var(--fact-percent)");
    expect(declarations(".session-context-share").get("min-width")).toBe("var(--fact-percent)");
    expect(declarations(".session-value-memory").get("min-width")).toBe("var(--fact-memory)");
    expect(declarations(".session-runtime").get("min-width")).toBe("var(--fact-runtime)");
    expect(declarations(".session-context-tokens").get("min-width")).toBe("var(--fact-tokens)");
  });

  it("sizes the cells for the widest measured value with some room", () => {
    // Measured in Edge at 11 px with tabular digits: "100 %", "12,3 GB", "199.9k / 200k", and the
    // run time up to nine days in every language ("9 ημέρες").
    expect(token("--fact-percent")).toBeGreaterThanOrEqual(2.71);
    expect(token("--fact-memory")).toBeGreaterThanOrEqual(3.37);
    expect(token("--fact-tokens")).toBeGreaterThanOrEqual(5.92);
    expect(token("--fact-runtime")).toBeGreaterThanOrEqual(3.89);
  });

  it("right-aligns the numbers in steady digits", () => {
    for (const cell of [".session-value", ".session-runtime", ".session-context-tokens"]) {
      expect(declarations(cell).get("text-align")).toBe("end");
    }
    expect(declarations(".row-fact").get("font-variant-numeric")).toBe("tabular-nums");
  });

  it("keeps the place of a column without a value", () => {
    expect(declarations(".session-resource.vacant").get("visibility")).toBe("hidden");
  });

  it("lets the context bar take the free width, down to its minimum", () => {
    const bar = declarations(".session-context-bar");
    expect(bar.get("flex")).toBe("1 1 var(--fact-bar-min)");
    expect(bar.get("min-width")).toBe("var(--fact-bar-min)");
    expect(declarations(":root").get("--fact-bar-min")).toBe("24px");
  });
});

describe("the third and fourth line", () => {
  it("keeps the load to one line and never cuts a cell mid-text", () => {
    const rule = declarations(".session-load");
    expect(rule.get("display")).toBe("flex");
    // Cells that do not fit (below the narrowest sidebar) wrap whole out of sight.
    expect(rule.get("flex-wrap")).toBe("wrap");
    expect(rule.get("height")).toBe("1lh");
    expect(rule.get("overflow")).toBe("hidden");
    // A context with a bar takes the line's free width; the share alone keeps its cell.
    expect(declarations(".session-context.grows").get("flex")).toBe("1 1 auto");
    expect(declarations(".session-context").get("flex")).toBe("none");
  });

  it("drops nothing any more: no giving-way steps", () => {
    expect(CSS).not.toMatch(/@container session-text/);
  });

  it("fits the whole load with units into the narrowest sidebar", () => {
    const percent = token("--fact-percent") * LINE_PX;
    const memory = token("--fact-memory") * LINE_PX;
    const dot = 0.4 * LINE_PX;
    const cpu = ICON_PX + NEAR_PX + percent;
    const ram = ICON_PX + NEAR_PX + memory;
    const gpu = ICON_PX + NEAR_PX + percent + NEAR_PX + dot + NEAR_PX + memory - 4;
    expect(cpu + ram + gpu + 2 * CELL_GAP_PX).toBeLessThanOrEqual(NARROWEST_TEXT_PX);
    expect(declarations(".session-value-sep").get("min-width")).toBe("0.4em");
    expect(declarations(".session-value-sep").get("margin")).toBe("0 -2px");
  });

  it("gives the context bar the free width, share and tokens their cells", () => {
    const bar = declarations(".session-context-bar");
    expect(bar.get("flex")).toBe("1 1 var(--fact-bar-min)");
    expect(bar.get("height")).toBe("6px");
    const runtime = token("--fact-runtime") * LINE_PX;
    const share = token("--fact-percent") * LINE_PX;
    const tokens = token("--fact-tokens") * LINE_PX;
    // Run time, bar at its minimum, share close to the bar, tokens.
    const line = runtime + CELL_GAP_PX + 24 + NEAR_PX + share + CELL_GAP_PX + tokens;
    expect(line).toBeLessThanOrEqual(NARROWEST_TEXT_PX);
  });
});

describe("the striped context bar", () => {
  it("draws the stripes on a layer one period wider than the fill, clipped by it", () => {
    const fill = declarations(".session-context-fill");
    expect(fill.get("overflow")).toBe("hidden");
    expect(fill.get("--stripe-period")).toBe("11.3137px");
    // 4 px stripe and 4 px gap across 45° stripes: 8 px times the square root of two.
    expect(8 * Math.SQRT2).toBeCloseTo(11.3137, 3);
    const stripes = declarations(".session-context-fill::after");
    expect(stripes.get("width")).toBe("calc(100% + var(--stripe-period))");
    expect(stripes.get("background")?.replace(/\s+/g, " ")).toContain(
      "repeating-linear-gradient( 45deg",
    );
  });

  it("moves them only while the session works, by transform alone", () => {
    const moving =
      /\.session\.look-working \.session-context-fill::after \{\s*animation: context-stripes [^;]+ infinite;\s*\}/;
    expect(CSS).toMatch(moving);
    const keyframes = /@keyframes context-stripes \{([\s\S]*?)\n\}/.exec(CSS)?.[1] ?? "";
    expect(keyframes).toContain("transform: translateX(11.3137px)");
    expect(keyframes).not.toMatch(/background|width|left/);
    // No other rule animates the stripes.
    expect(CSS.match(/animation: context-stripes/g)).toHaveLength(1);
  });

  it("stops them for reduced motion and while saving power", () => {
    const reduced = [...CSS.matchAll(/@media \(prefers-reduced-motion: reduce\) \{([\s\S]*?)\n\}/g)]
      .map((match) => match[1] ?? "")
      .join("\n");
    expect(
      declarations(".session.look-working .session-context-fill::after", reduced).get("animation"),
    ).toBe("none");
    expect(
      declarations(":root.power-saving .session.look-working .session-context-fill::after").get(
        "animation",
      ),
    ).toBe("none");
  });
});

describe("the first line: signs, title, state", () => {
  it("puts the one-word state in a fixed cell; only the title gives way", () => {
    const state = declarations(".session-state");
    expect(state.get("flex")).toBe("none");
    const status = declarations(".session-status");
    expect(status.get("flex")).toBe("none");
    expect(status.get("min-width")).toBe("var(--state-word)");
    // "Entscheidung" in bold, the widest word in every language: 70.3 px at 11.5 px.
    const em = /^(\d+(?:\.\d+)?)em$/.exec(declarations(":root").get("--state-word") ?? "");
    expect(Number(em?.[1]) * 11.5).toBeGreaterThanOrEqual(70.3);
    const title = declarations(".session-title-row .session-title");
    expect(title.get("flex")).toBe("1 1 0");
    expect(title.get("min-width")).toBe("var(--title-min)");
    expect(declarations(":root").get("--title-min")).toBe("4em");
  });

  it("draws what stands before the title in front of the marker and the title", () => {
    expect(declarations(".session-signs").get("flex")).toBe("none");
    expect(declarations(".row-before-title").get("order")).toBe("-2");
    expect(declarations(".session-title-row > .session-marker").get("order")).toBe("-1");
  });

  it("pushes a line's right part to its edge without room of its own", () => {
    const push = declarations(".row-push");
    expect(push.get("margin-left")).toBe("auto");
    expect(push.get("margin-right")).toBe(`-${declarations(".row-line").get("gap") ?? ""}`);
  });

  it("moves a working row's dot as the dot animation setting chooses, still without motion", () => {
    const moving =
      /\n\.look-working \.state-dot \{[^}]*animation: var\(--dots-name\) var\(--dots-time\) var\(--dots-ease\) infinite both;/;
    expect(CSS).toMatch(moving);
    const reduced = [...CSS.matchAll(/@media \(prefers-reduced-motion: reduce\) \{([\s\S]*?)\n\}/g)]
      .map((match) => match[1] ?? "")
      .join("\n");
    expect(declarations(".look-working .state-dot", reduced).get("animation")).toBe("none");
  });
});

describe("the second line: folder at the start, branch at the right", () => {
  it("keeps folder and branch quieter than the state", () => {
    const where = declarations(".row-where");
    expect(where.get("font-size")).toBe("11.5px");
    expect(where.get("color")).toBe("var(--muted)");
  });

  it("gives way with the folder first, then the branch, each down to a minimum", () => {
    const path = declarations(".session-path");
    const branch = declarations(".session-branch");
    expect(path.get("flex")).toBe("0 1000 auto");
    expect(path.get("min-width")).toBe("34px");
    expect(branch.get("flex")).toBe("0 1 auto");
    expect(branch.get("min-width")).toBe("30px");
    // The folder's icon stays whole while the name gives way.
    expect(declarations(".session-path-icon").get("flex")).toBe("none");
  });
});

describe("the title line", () => {
  // In a narrow row the rate and then the context's bar step back, so the title stays readable
  // and nothing runs past the row's edge.
  it("gives up the rate, then the context's bar, never the title or the state", () => {
    expect(declarations(".session-title-row").get("container")).toBe("title-line / inline-size");
    const steps = [...CSS.matchAll(/@container title-line \(max-width: (\d+)px\) \{([^}]*)\}/g)];
    expect(
      steps.map((step) => [Number(step[1]), (step[2] ?? "").trim().split(/\s*\{/)[0]]),
    ).toEqual([
      [330, ".tree-flow-rate"],
      [230, ".session-context-mini .session-context-bar"],
    ]);
  });
});
