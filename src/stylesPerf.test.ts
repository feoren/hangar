/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

/** Declarations of every top level rule whose selector list is exactly `selector`. */
function declarations(selector: string): string[] {
  const found: string[] = [];
  const rule = /([^{}]+)\{([^{}]*)\}/g;
  for (const match of CSS.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(rule)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim().replace(/\s+/g, " "));
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found;
}

function hasDeclaration(selector: string, property: string, value: RegExp): boolean {
  return declarations(selector).some((body) =>
    body
      .split(";")
      .map((part) => part.trim())
      .some((part) => {
        const [name, ...rest] = part.split(":");
        return name?.trim() === property && value.test(rest.join(":").trim());
      }),
  );
}

describe("hidden terminal panes", () => {
  // xterm 6 pauses its renderer only through an IntersectionObserver on its screen, which looks at
  // geometry, not at visibility: an invisible pane in the viewport still draws every output frame.
  it("moves hidden panes out of the viewport and keeps them invisible", () => {
    expect(hasDeclaration(".pane", "visibility", /^hidden$/)).toBe(true);
    expect(hasDeclaration(".pane", "translate", /^-\d+vw 0$/)).toBe(true);
  });

  it("brings visible panes back without a transform", () => {
    expect(hasDeclaration(".pane.visible", "visibility", /^visible$/)).toBe(true);
    expect(hasDeclaration(".pane.visible", "translate", /^none$/)).toBe(true);
  });

  it("moves the panes out of the viewport while the home view shows", () => {
    expect(hasDeclaration(".panes.put-aside > :not(.empty)", "visibility", /^hidden$/)).toBe(true);
    expect(hasDeclaration(".panes.put-aside > .pane", "translate", /^-\d+vw 0$/)).toBe(true);
  });

  it("never resizes or removes a hidden pane, so fit and the scrollback stay intact", () => {
    for (const selector of [
      ".pane",
      ".panes.put-aside > .pane",
      ".panes.put-aside > :not(.empty)",
    ]) {
      for (const body of declarations(selector)) {
        expect(body).not.toMatch(/display\s*:\s*none/);
        expect(body).not.toMatch(/(^|;)\s*(width|height)\s*:\s*0/);
      }
    }
  });
});
