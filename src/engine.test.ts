/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { applyDocumentEngine, engineFromUserAgent } from "./engine";

const WEBVIEW2 =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0";
const WKWEBVIEW =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const WEBKITGTK =
  "Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const HEADLESS_CHROME =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/142.0.0.0 Safari/537.36";

describe("engineFromUserAgent", () => {
  it("tells WebView2 and other Chromium browsers apart from WebKit", () => {
    expect(engineFromUserAgent(WEBVIEW2)).toBe("chromium");
    expect(engineFromUserAgent(HEADLESS_CHROME)).toBe("chromium");
  });

  it("recognizes WKWebView on macOS and WebKitGTK on Linux", () => {
    expect(engineFromUserAgent(WKWEBVIEW)).toBe("webkit");
    expect(engineFromUserAgent(WEBKITGTK)).toBe("webkit");
  });

  it("keeps the native controls for anything else", () => {
    expect(engineFromUserAgent("")).toBe("chromium");
    expect(engineFromUserAgent("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101")).toBe(
      "chromium",
    );
  });
});

describe("applyDocumentEngine", () => {
  it("sets data-engine on the root element", () => {
    const root: Pick<HTMLElement, "dataset"> = { dataset: {} };
    applyDocumentEngine(root, WEBKITGTK);
    expect(root.dataset.engine).toBe("webkit");
    applyDocumentEngine(root, WEBVIEW2);
    expect(root.dataset.engine).toBe("chromium");
  });
});

/** Splits a selector list at its top-level commas, not at those inside `:where(...)`. */
function selectorList(selector: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const char of selector) {
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (char === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
    } else {
      current += char;
    }
  }
  parts.push(current.trim());
  return parts;
}

/** The selector of every rule, also of rules inside an at-rule. */
function selectors(css: string): string[] {
  return css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("}")
    .map((block) => block.split("{").at(-2)?.trim() ?? "")
    .filter((selector) => selector !== "" && !selector.startsWith("@"));
}

/** Classes of the rules in styles.css that ask for a thin scrollbar. */
function thinScrollbarClasses(css: string): string[] {
  const rules = css.replace(/\/\*[\s\S]*?\*\//g, "").split("}");
  return rules
    .filter((rule) => /scrollbar-width:\s*thin/.test(rule))
    .flatMap((rule) => selectorList(rule.split("{")[0]?.trim() ?? ""))
    .sort();
}

describe("webkit.css", () => {
  const css = readFileSync(new URL("./webkit.css", import.meta.url), "utf8");
  const styles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

  it("only styles the WebKit engine, so Chromium keeps its native controls", () => {
    const all = selectors(css);
    expect(all.length).toBeGreaterThan(0);
    for (const selector of all) {
      for (const part of selectorList(selector)) {
        expect(part, part).toMatch(/^:where\(\s*\[data-engine="webkit"\]/);
        // Every alternative inside the :where() is gated too.
        const inner = /^:where\(([\s\S]*)\)/.exec(part)?.[1] ?? "";
        for (const alternative of selectorList(inner)) {
          expect(alternative, part).toMatch(/^\[data-engine="webkit"\]/);
        }
      }
    }
  });

  it("gives every box with a thin scrollbar in styles.css the thin WebKit scrollbar", () => {
    const thin = thinScrollbarClasses(styles);
    expect(thin.length).toBeGreaterThan(0);
    const block = /:where\(([^)]*)\)::-webkit-scrollbar \{\s*width: 10px;/.exec(css)?.[1] ?? "";
    const gated = selectorList(block)
      .map((part) => part.replace('[data-engine="webkit"]', "").trim())
      .sort();
    expect(gated).toEqual(thin);
  });
});
