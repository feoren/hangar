/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// The sub-agent count (SubAgentCount.tsx, styles.css "Sub-agent count") must never take width
// from a session row: its appearance would shift the row's right parts against the next rows and
// switch the title line's container queries. These tests keep every dot outside the flow.

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** Declarations of every top level rule whose selector list contains `selector`. */
function declarations(selector: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of CSS.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim().replace(/\s+/g, " "));
    if (!selectors.includes(selector)) continue;
    for (const part of (match[2] ?? "").split(";")) {
      const [name, ...rest] = part.split(":");
      if (name !== undefined && rest.length > 0) found.set(name.trim(), rest.join(":").trim());
    }
  }
  return found;
}

describe("the sub-agent count", () => {
  it("lies outside the flow in every place", () => {
    expect(declarations(".subagent-count").get("position")).toBe("absolute");
    for (const place of [".badge-count", ".row-corner", ".rail-subagents"]) {
      const rules = declarations(place);
      // No place puts it back into the flow or gives it a margin.
      expect(rules.get("position") ?? "absolute").toBe("absolute");
      expect(rules.has("margin")).toBe(false);
      expect(rules.has("margin-left")).toBe(false);
      expect(rules.has("top") && rules.has("right")).toBe(true);
    }
  });

  it("is anchored to the badge, the row or the rail button", () => {
    expect(declarations(".badge-anchor").get("position")).toBe("relative");
    expect(declarations(".session").get("position")).toBe("relative");
    expect(declarations(".rail-session").get("position")).toBe("relative");
  });

  it("shows once per row and stays below the hover actions", () => {
    expect(declarations(".session:has(.badge-count) > .row-corner").get("display")).toBe("none");
    const dot = Number(declarations(".subagent-count").get("z-index"));
    const actions = Number(declarations(".session-actions").get("z-index"));
    expect(dot).toBeLessThan(actions);
  });

  it("does not animate", () => {
    const rules = declarations(".subagent-count");
    expect(rules.has("animation")).toBe(false);
    expect(rules.has("transition")).toBe(false);
  });
});
