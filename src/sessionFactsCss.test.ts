/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// The third line of a session row lines its values up in columns of fixed width and moves the
// session group onto a fourth line when both groups do not fit side by side (styles.css, "Third
// line"). These tests keep the cells, the wrap and the giving-way steps consistent.

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** Font size of the line; the --fact-* tokens are in em of it. */
const LINE_PX = 11;
/** Widest names measured with the UI font (em): "CPU", "RAM", "GPU". */
const NAME_EM = { cpu: 1.87, memory: 2.14, gpu: 1.93 };
const ICON_PX = 10;
const NAME_GAP_PX = 4;
const CELL_GAP_PX = 8;

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

/** A --fact-* token in em, from the given selector (`:root` or a language override). */
function token(name: string, selector = ":root"): number {
  const value = declarations(selector).get(name);
  const em = /^(\d+(?:\.\d+)?)em$/.exec(value ?? "");
  if (em?.[1] === undefined)
    throw new Error(`${name} on ${selector} is not in em: ${String(value)}`);
  return Number(em[1]);
}

/** The max-width of the session-text container query that holds a rule for `selector`. */
function stepFor(selector: string): number {
  for (const match of CSS.matchAll(
    /@container session-text \(max-width: (\d+)px\) \{([\s\S]*?)\n\}/g,
  )) {
    if (declarations(selector, match[2] ?? "").size > 0) return Number(match[1]);
  }
  throw new Error(`no giving-way step for ${selector}`);
}

describe("cells of fixed width", () => {
  it("gives every value its cell from a token in :root", () => {
    expect(declarations(".session-value-percent").get("min-width")).toBe("var(--fact-percent)");
    expect(declarations(".session-context-share").get("min-width")).toBe("var(--fact-percent)");
    expect(declarations(".session-value-memory").get("min-width")).toBe("var(--fact-memory)");
    expect(declarations(".session-runtime").get("min-width")).toBe("var(--fact-runtime)");
    expect(declarations(".session-context-tokens").get("min-width")).toBe("var(--fact-tokens)");
    expect(declarations(".session-context-bar").get("width")).toBe("var(--fact-bar)");
  });

  it("sizes the cells for the widest measured value with some room", () => {
    // Measured in Edge at 11 px with tabular digits: "100 %", "12,3 GB", "199.9k / 200k", "99 days".
    expect(token("--fact-percent")).toBeGreaterThanOrEqual(2.71);
    expect(token("--fact-memory")).toBeGreaterThanOrEqual(3.37);
    expect(token("--fact-tokens")).toBeGreaterThanOrEqual(5.92);
    expect(token("--fact-runtime")).toBeGreaterThanOrEqual(3.36);
    // "99 giorni", "23 godz.", "99 ημέρες".
    expect(token("--fact-runtime", ":root:lang(it)")).toBeGreaterThanOrEqual(3.93);
    expect(token("--fact-runtime", ":root:lang(pl)")).toBeGreaterThanOrEqual(3.79);
    expect(token("--fact-runtime", ":root:lang(el)")).toBeGreaterThanOrEqual(4.43);
  });

  it("right-aligns the numbers in steady digits", () => {
    for (const cell of [".session-value", ".session-runtime", ".session-context-tokens"]) {
      expect(declarations(cell).get("text-align")).toBe("end");
    }
    expect(declarations(".session-detail").get("font-variant-numeric")).toBe("tabular-nums");
  });

  it("keeps the place of a column without a value", () => {
    expect(declarations(".session-resource.vacant").get("visibility")).toBe("hidden");
  });
});

describe("two groups, side by side or on two lines", () => {
  it("lets the session group wrap onto a fourth line", () => {
    const line = declarations(".session-detail");
    expect(line.get("display")).toBe("flex");
    expect(line.get("flex-wrap")).toBe("wrap");
    expect(declarations(".session-detail:empty").get("display")).toBe("none");
  });

  it("never cuts a group mid-text: cells that do not fit go whole, out of sight", () => {
    for (const group of [".session-load", ".session-run"]) {
      const rule = declarations(group);
      expect(rule.get("flex-wrap")).toBe("wrap");
      expect(rule.get("height")).toBe("1lh");
      expect(rule.get("overflow")).toBe("hidden");
      expect(rule.get("min-width")).toBe("0");
    }
    // Every row decides alike whether the session group moves: it claims its full width.
    expect(declarations(".session-run").get("flex-basis")?.replace(/\s+/g, " ")).toBe(
      "calc( var(--fact-runtime) + var(--fact-bar) + var(--fact-percent) + var(--fact-tokens) + 20px )",
    );
    // The context meter's parts are cells of the session group itself.
    expect(declarations(".session-context").get("display")).toBe("contents");
  });

  it("gives way only once a group alone no longer fits", () => {
    const percent = token("--fact-percent");
    const memory = token("--fact-memory");
    const loadWithGpu =
      (NAME_EM.cpu + percent + NAME_EM.memory + memory + NAME_EM.gpu + percent + memory) * LINE_PX +
      4 * NAME_GAP_PX +
      2 * CELL_GAP_PX;
    expect(stepFor(".session-load:has(.session-resource-gpu) .session-resource-name")).toBe(
      Math.ceil(loadWithGpu),
    );
    const load = (NAME_EM.cpu + percent + NAME_EM.memory + memory) * LINE_PX + 2 * NAME_GAP_PX;
    expect(stepFor(".session-load .session-resource-name")).toBe(Math.ceil(load + CELL_GAP_PX));
    // With icons instead of names the load group still fits the narrowest sidebar's rows.
    const iconsWithGpu =
      loadWithGpu - (NAME_EM.cpu + NAME_EM.memory + NAME_EM.gpu) * LINE_PX + 3 * ICON_PX;
    expect(iconsWithGpu).toBeLessThan(220);

    const runtime = token("--fact-runtime", ":root:lang(el)");
    const session =
      (runtime + token("--fact-bar") + percent + token("--fact-tokens")) * LINE_PX +
      3 * CELL_GAP_PX -
      NAME_GAP_PX;
    expect(stepFor(".session-run .session-context-tokens")).toBe(Math.ceil(session));
  });
});
