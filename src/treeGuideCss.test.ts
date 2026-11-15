/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// The sidebar's tree guides (agent groups, sub-agent tree): dots in whole device pixels, so they
// look the same at every display scale. Where the WebView has anchor positioning, each tree's
// vertical line is a single element ending at its last row's elbow, so its dots never meet a
// second pattern; the per-row pieces are the fallback (styles.css).

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

interface Rule {
  selectors: string[];
  body: Map<string, string>;
}

/** The @supports blocks with `condition`, and the CSS outside of every @supports block. */
function splitSupports(condition: string): { inside: string; outside: string } {
  let inside = "";
  let outside = "";
  let rest = CSS;
  for (;;) {
    const start = rest.indexOf("@supports");
    if (start < 0) break;
    const open = rest.indexOf("{", start);
    let depth = 1;
    let end = open + 1;
    while (depth > 0 && end < rest.length) {
      if (rest[end] === "{") depth += 1;
      if (rest[end] === "}") depth -= 1;
      end += 1;
    }
    outside += rest.slice(0, start);
    if (rest.slice(start, open).includes(condition)) inside += rest.slice(open + 1, end - 1);
    rest = rest.slice(end);
  }
  return { inside, outside: outside + rest };
}

function rules(css: string): Rule[] {
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => {
    const body = new Map<string, string>();
    for (const part of (match[2] ?? "").split(";")) {
      const [name, ...rest] = part.split(":");
      if (name !== undefined && rest.length > 0) {
        body.set(name.trim(), rest.join(":").trim().replace(/\s+/g, " "));
      }
    }
    return {
      selectors: (match[1] ?? "").split(",").map((part) => part.trim().replace(/\s+/g, " ")),
      body,
    };
  });
}

const RULES = rules(CSS);
const ANCHORED = splitSupports("anchor-name");
/** Rules outside any @supports block: the fallback drawing. */
const BASE = rules(ANCHORED.outside);
/** Rules that apply with anchor positioning. */
const SINGLE = rules(ANCHORED.inside);

function declaration(selector: string, property: string, from = BASE): string | undefined {
  let found: string | undefined;
  for (const rule of from) {
    if (rule.selectors.includes(selector)) found = rule.body.get(property) ?? found;
  }
  return found;
}

const GUIDE = /tree-guide|subagent-tree|agent-rows/;

describe("tree guides", () => {
  it("draws no part with a dotted border any more", () => {
    for (const rule of RULES) {
      if (!rule.selectors.some((selector) => GUIDE.test(selector))) continue;
      for (const [property, value] of rule.body) {
        expect(`${property}: ${value}`).not.toMatch(/dotted|^border-(style|width|left)\b/);
      }
    }
  });

  it("snaps the dot to whole device pixels and repeats it with a steady period", () => {
    expect(declaration(":root", "--device-px")).toBe("1px");
    for (const [token, direction] of [
      ["--tree-dots-y", "to bottom"],
      ["--tree-dots-x", "to right"],
    ] as const) {
      expect(declaration(":root", token)).toBe(
        `repeating-linear-gradient( ${direction}, var(--tree-guide) 0 var(--tree-dot), ` +
          "transparent var(--tree-dot) calc(2 * var(--tree-dot)) )",
      );
    }
  });

  it("gives every dotted part the dot pattern and the dot's thickness", () => {
    for (const selector of [
      ".agent-rows::before",
      ".tree-guide::before",
      ".subagent-tree-item::before",
      ".agent-rows > .subagent-tree-row.tree-continues::before",
    ]) {
      expect(declaration(selector, "background")).toBe("var(--tree-dots-y)");
      expect(declaration(selector, "width")).toBe("var(--tree-dot)");
    }
    for (const selector of [".tree-guide::after", ".subagent-tree-item::after"]) {
      expect(declaration(selector, "background")).toBe("var(--tree-dots-x)");
      expect(declaration(selector, "height")).toBe("var(--tree-dot)");
      // Beside the vertical line, so no dot is drawn twice.
      expect(declaration(selector, "left")).toBe("var(--tree-dot)");
    }
  });

  it("keeps a plain line where the WebView lacks CSS round()", () => {
    // The first :root rule holds values without round(); only an @supports guard snaps them, so
    // an older WebView draws a one-pixel line instead of none.
    const root = RULES.find((rule) => rule.selectors.includes(":root"));
    expect(root?.body.get("--tree-dot")).toBe("1px");
    expect(root?.body.get("--row-edge-drawn")).toBe("var(--row-edge)");
    expect(root?.body.get("--row-border-drawn")).toBe("var(--row-border)");
    const guarded =
      /@supports \(width: round\(down, 1px, 1px\)\) \{\s*:root \{([^{}]*)\}\s*\}/.exec(CSS);
    expect(guarded?.[1]).toContain("--tree-dot: max(var(--device-px), round(down, 1px,");
    expect(guarded?.[1]).toContain("--row-edge-drawn:");
    expect(guarded?.[1]).toContain("--row-border-drawn:");
    expect(CSS.match(/round\(/g)?.length).toBe(4);
  });

  it("places a row's guide with the edge width the browser draws", () => {
    expect(declaration(".tree-guide", "left")).toBe(
      "calc(-1 * (var(--tree-step) + var(--row-edge-drawn)))",
    );
  });

  it("runs a session row's elbow up to the row's frame", () => {
    expect(declaration(".agent-rows .tree-guide::after", "width")).toBe(
      "calc(var(--tree-step) - var(--tree-dot))",
    );
  });

  it("keeps the way to the row on screen solid in the accent without anchors", () => {
    for (const selector of [
      ".agent-rows:has(> .session.active)::before",
      ".agent-rows > .session:has(~ .session.active) > .tree-guide::before",
      ".session.active > .tree-guide::after",
    ]) {
      expect(declaration(selector, "background")).toBe("var(--accent)");
    }
    // Down to the elbow, corner included; the dots below start right after it.
    expect(declaration(".session.active > .tree-guide::before", "top")).toBe(
      "calc(var(--agent-elbow-y) + var(--tree-dot))",
    );
  });

  it("draws one vertical line per tree where anchors work", () => {
    expect(declaration(".agent-rows > .session.tree-last", "anchor-name", SINGLE)).toBe(
      "--tree-end",
    );
    expect(declaration(".agent-rows > .session.active", "anchor-name", SINGLE)).toBe("--tree-lit");
    expect(declaration(".agent-rows > .session.active.tree-last", "anchor-name", SINGLE)).toBe(
      "--tree-end, --tree-lit",
    );
    // Dotted from the header to the last row's elbow, corner included.
    for (const selector of [".agent-rows::before", ".agent-rows:has(> .session.active)::before"]) {
      expect(declaration(selector, "background", SINGLE)).toBe("var(--tree-dots-y)");
      expect(declaration(selector, "bottom", SINGLE)).toBe(
        "calc(anchor(--tree-end top) - var(--agent-elbow-y) - var(--tree-dot))",
      );
    }
    // The accent over it, down to the active row's elbow.
    const lit = ".agent-rows:has(> .session.active)::after";
    expect(declaration(lit, "background", SINGLE)).toBe("var(--accent)");
    expect(declaration(lit, "bottom", SINGLE)).toBe(
      "calc(anchor(--tree-lit top) - var(--agent-elbow-y) - var(--tree-dot))",
    );
    expect(declaration(lit, "left", SINGLE)).toBe(declaration(".agent-rows::before", "left"));
    // The rows keep only their elbows.
    for (const selector of [
      ".agent-rows .tree-guide::before",
      ".agent-rows > .subagent-tree-row.tree-continues::before",
    ]) {
      expect(declaration(selector, "display", SINGLE)).toBe("none");
    }
    expect(declaration(".agent-rows .session.active > .tree-guide", "background", SINGLE)).toBe(
      "none",
    );
  });

  it("draws one line for the sub-agent tree where anchors work", () => {
    expect(
      declaration(".subagent-tree > .subagent-tree-item:last-child", "anchor-name", SINGLE),
    ).toBe("--subagent-end");
    expect(declaration(".subagent-tree::before", "background", SINGLE)).toBe("var(--tree-dots-y)");
    expect(declaration(".subagent-tree::before", "left", SINGLE)).toBe("var(--subagent-indent)");
    expect(declaration(".subagent-tree::before", "bottom", SINGLE)).toBe(
      "calc(anchor(--subagent-end top) - 12px - var(--tree-dot))",
    );
    expect(declaration(".subagent-tree-item::before", "display", SINGLE)).toBe("none");
    expect(declaration(".subagent-tree", "padding", BASE)).toBe("0 0 0 var(--subagent-indent)");
  });
});
