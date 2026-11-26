/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// "Edit link group" lists the members' names and, below them, each session that joins with its name
// and a button that takes it out again (LinkDialog.tsx). The name fields form one column: a
// joining row must not give up width to its button, or its field starts left of the others.

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

describe("a joining session's row in Edit link group", () => {
  it("keeps the full row width for its name, like a member's row", () => {
    // A flex row would share the width with the button and shift the two grid columns.
    expect(declarations(".link-joining").get("display")).toBeUndefined();
    expect(declarations(".link-joining").get("position")).toBe("relative");
  });

  it("places the button inside the end of the name field, which leaves room for it", () => {
    const button = declarations(".link-unadd");
    expect(button.get("position")).toBe("absolute");
    expect(button.get("inset-inline-end")).toBeDefined();
    const room = declarations(".link-joining .text-input").get("padding-inline-end");
    // The button is 26 px wide (.icon-button) and sits 2 px from the field's end.
    expect(room).toBe("30px");
  });
});
