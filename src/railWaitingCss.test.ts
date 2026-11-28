/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// The collapsed sidebar uses the class `rail-waiting` twice (Sidebar.tsx): once for the number of
// waiting sessions at the top of the rail, once for the messages waiting on one session, a disc at
// the bottom left of its button. The disc's absolute position must stay inside the session button;
// as a bare `.rail-waiting` rule it also moved the top count out of the rail's flow, down to the
// window's bottom left corner over the Settings button.

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

describe("the waiting counts in the rail", () => {
  it("keeps the count of waiting sessions in the rail's flow", () => {
    expect(declarations(".rail-waiting").has("position")).toBe(false);
  });

  it("pins a session's waiting messages to its own button", () => {
    expect(declarations(".rail-session .rail-waiting").get("position")).toBe("absolute");
    expect(declarations(".rail-session").get("position")).toBe("relative");
  });
});
