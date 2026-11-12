import { describe, expect, it } from "vitest";

import catalogText from "../src-tauri/src/agent_catalog.toml?raw";
import { AGENT_ICON_IDS, agentIcon } from "./agentIcons";
import noticeText from "./assets/agent-icons/NOTICE.md?raw";

const SOURCES = import.meta.glob<string>("./assets/agent-icons/*.svg", {
  eager: true,
  query: "?raw",
  import: "default",
});

/** Vite inlines assets below 4 KB as data URLs, so no icon costs a request of its own. */
const MAX_BYTES = 4096;
const OPENING = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">';
const ELEMENTS = new Set(["svg", "rect", "path", "text"]);
const ATTRIBUTES = new Set([
  "xmlns",
  "viewBox",
  "width",
  "height",
  "x",
  "y",
  "dy",
  "rx",
  "d",
  "fill",
  "stroke",
  "stroke-opacity",
  "transform",
  "text-anchor",
  "font-family",
  "font-size",
  "font-weight",
]);

function catalogIds(toml: string): string[] {
  return [...toml.matchAll(/^\[\[agent\]\]\s*\nid = "([^"]+)"/gm)].map((match) => match[1] ?? "");
}

describe("agent icons", () => {
  const ids = catalogIds(catalogText);

  it("reads the catalog", () => {
    expect(ids.length).toBeGreaterThan(20);
    expect(ids).toContain("claude");
  });

  it("has an icon for every catalog agent and none for anything else", () => {
    expect([...AGENT_ICON_IDS].sort()).toEqual([...ids].sort());
    for (const id of ids) expect(agentIcon(id), id).toBeTruthy();
    expect(agentIcon("pwsh")).toBeUndefined();
    expect(agentIcon("constructor")).toBeUndefined();
  });

  it("names every icon's source and license in the notice", () => {
    for (const id of ids) expect(noticeText, id).toContain(`\`${id}.svg\``);
  });

  it("ships only small, static SVG without scripts or outside references", () => {
    const files = Object.entries(SOURCES);
    expect(files).toHaveLength(ids.length);
    for (const [path, svg] of files) {
      expect(new TextEncoder().encode(svg).length, path).toBeLessThanOrEqual(MAX_BYTES);
      expect(svg.startsWith(OPENING), path).toBe(true);
      expect(svg.trimEnd().endsWith("</svg>"), path).toBe(true);
      // No comments, processing instructions, CDATA, entities or style sheets.
      expect(svg, path).not.toMatch(/<[!?]|&|url\(|javascript:|style/i);
      const elements = [...svg.matchAll(/<([A-Za-z][\w:-]*)/g)].map((match) => match[1] ?? "");
      for (const element of elements)
        expect(ELEMENTS.has(element), `${path}: ${element}`).toBe(true);
      const attributes = [...svg.matchAll(/\s([A-Za-z][\w:-]*)="[^"]*"/g)].map(
        (match) => match[1] ?? "",
      );
      for (const attribute of attributes)
        expect(ATTRIBUTES.has(attribute), `${path}: ${attribute}`).toBe(true);
      // The namespace is the only URL.
      expect(svg.match(/https?:/g), path).toHaveLength(1);
    }
  });
});
