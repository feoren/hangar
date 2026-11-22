/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const CSS = readFileSync(new URL("./styles.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The bodies of the keyframes whose name starts with `prefix`, by name. */
function keyframes(prefix: string): Map<string, string> {
  const found = new Map<string, string>();
  const rule = /@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*[^{}]*)\}/g;
  for (const match of CSS.matchAll(rule)) {
    const name = match[1] ?? "";
    if (name.startsWith(prefix)) found.set(name, match[2] ?? "");
  }
  return found;
}

describe("tree flow styles", () => {
  it("move nothing but background positions, so no frame needs layout", () => {
    const frames = keyframes("tree-flow-");
    expect([...frames.keys()].sort()).toEqual([
      "tree-flow-dots-x",
      "tree-flow-dots-y",
      "tree-flow-pulse-x",
      "tree-flow-pulse-y",
      "tree-flow-stream-x",
      "tree-flow-stream-y",
    ]);
    for (const [name, body] of frames) {
      const properties = [...body.matchAll(/([\w-]+)\s*:/g)].map((match) => match[1]);
      expect(new Set(properties), name).toEqual(new Set(["background-position"]));
    }
  });

  it("stand still with reduced motion and while saving energy", () => {
    expect(CSS).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{\s*\.tree-flow-line,\s*\.tree-flow-elbow \{\s*animation: none;/,
    );
    expect(CSS).toMatch(
      /:root\.power-saving \.tree-flow-line,\s*:root\.power-saving \.tree-flow-elbow \{\s*animation: none;/,
    );
  });

  it("quiet the accent way to the active row while a flow runs over it", () => {
    expect(CSS).toMatch(
      /--tree-lit-quiet: color-mix\(in srgb, var\(--accent\) \d+%, transparent\);/,
    );
    expect(CSS).toMatch(
      /\.agent-rows:has\(> \.session\.active\):has\(> \.tree-flow-line\.live\)::after \{\s*background: var\(--tree-lit-quiet\);/,
    );
    expect(CSS).toMatch(
      /\.session\.active > \.tree-guide:has\(> \.tree-flow-elbow\.live\)::after \{\s*background: var\(--tree-lit-quiet\);/,
    );
  });

  it("keep a part that is not live faded out and still", () => {
    expect(CSS).toMatch(
      /\.tree-flow-line,\s*\.tree-flow-elbow \{[^}]*opacity: 0;[^}]*animation-play-state: paused;/,
    );
    expect(CSS).toMatch(
      /\.tree-flow-line\.live,\s*\.tree-flow-elbow\.live \{[^}]*opacity: 1;[^}]*animation-play-state: running;/,
    );
  });
});
