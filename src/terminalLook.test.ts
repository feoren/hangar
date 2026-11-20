/// <reference types="node" />
import { readFileSync } from "node:fs";

import { afterEach, describe, expect, it } from "vitest";

import {
  BUNDLED_FONTS,
  SYSTEM_FONTS,
  createFontLoader,
  defaultUsesBundledFont,
  fontChoices,
  fontLoadQueries,
  fontStack,
  installedFonts,
  isCursorStyle,
  isFontInstalled,
  isBundledFont,
  isTerminalFont,
  setBundledMonoFont,
  type MeasureText,
  waitingStack,
} from "./terminalLook";

/** Paths only: the lazy glob never loads the files. */
const FONT_FILES = import.meta.glob("./assets/fonts/*/*");

/** Fake canvas: every installed font has its own width, everything else falls back. */
function fakeMeasure(installed: readonly string[]): MeasureText {
  return (cssFont) => {
    const match = /"([^"]+)"/.exec(cssFont);
    const fallback = cssFont.endsWith("serif") && !cssFont.endsWith("sans-serif") ? 500 : 400;
    if (match?.[1] !== undefined && installed.includes(match[1])) {
      return 300 + match[1].length;
    }
    return fallback;
  };
}

describe("font stack", () => {
  afterEach(() => {
    setBundledMonoFont(false);
  });

  it("keeps the previous stack byte for byte in Chromium (Windows)", () => {
    expect(fontStack("default")).toBe(
      '"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
    );
    expect(fontStack("Fira Code")).toBe(
      '"Fira Code", "Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
    );
    expect(fontStack("Cascadia Code")).toBe(
      '"Cascadia Code", "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace',
    );
  });

  it("puts the bundled Cascadia after the Windows fonts in WebKit", () => {
    setBundledMonoFont(true);
    expect(fontStack("default")).toBe(
      '"Cascadia Mono", "Cascadia Code", Consolas, "Hangar Cascadia Mono", "DejaVu Sans Mono", monospace',
    );
    expect(fontStack("Fira Code")).toBe(
      '"Fira Code", "Cascadia Mono", "Cascadia Code", Consolas, "Hangar Cascadia Mono", "DejaVu Sans Mono", monospace',
    );
    expect(fontStack("Cascadia Code")).toBe(
      '"Cascadia Code", "Cascadia Mono", Consolas, "Hangar Cascadia Mono", "DejaVu Sans Mono", monospace',
    );
  });

  it("waits in the previous stack, which differs from every stack that has to load", () => {
    for (const bundled of [false, true]) {
      setBundledMonoFont(bundled);
      expect(waitingStack()).toBe(
        '"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
      );
      for (const font of BUNDLED_FONTS) {
        expect(fontStack(font)).not.toBe(waitingStack());
      }
    }
    setBundledMonoFont(true);
    expect(fontStack("default")).not.toBe(waitingStack());
  });

  it("matches --mono in styles.css and in webkit.css", () => {
    const styles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    const webkit = readFileSync(new URL("./webkit.css", import.meta.url), "utf8");
    const mono = (css: string): string =>
      (/--mono:\s*([^;]+);/.exec(css)?.[1] ?? "").replace(/\s+/g, " ").trim();
    expect(mono(styles)).toBe(fontStack("default"));
    setBundledMonoFont(true);
    expect(mono(webkit)).toBe(fontStack("default"));
  });
});

describe("installed fonts", () => {
  it("detects a font by a width different from the fallback", () => {
    const measure = fakeMeasure(["JetBrains Mono"]);
    expect(isFontInstalled("JetBrains Mono", measure)).toBe(true);
    expect(isFontInstalled("Fira Code", measure)).toBe(false);
  });

  it("detects a font that is itself the default monospace font", () => {
    // Consolas measures like monospace, but differs from serif.
    const measure: MeasureText = (cssFont) => {
      if (cssFont.includes('"Consolas"')) return 400;
      return cssFont.endsWith("serif") ? 500 : 400;
    };
    expect(isFontInstalled("Consolas", measure)).toBe(true);
  });

  it("lists only installed fonts in the offered order", () => {
    expect(installedFonts(fakeMeasure(["Consolas", "Cascadia Code"]))).toEqual([
      "Cascadia Code",
      "Consolas",
    ]);
    expect(installedFonts(fakeMeasure([]))).toEqual([]);
  });
});

describe("font choices", () => {
  it("offers the default, every bundled font and the installed system fonts", () => {
    expect(fontChoices(["Consolas"], "default")).toEqual([
      "default",
      "JetBrains Mono",
      "Fira Code",
      "Geist Mono",
      "Hack",
      "Source Code Pro",
      "IBM Plex Mono",
      "Victor Mono",
      "Consolas",
    ]);
  });

  it("keeps a stored font listed that is not installed here", () => {
    expect(fontChoices([], "Cascadia Code").at(-1)).toBe("Cascadia Code");
    expect(
      fontChoices(["Cascadia Code"], "Cascadia Code").filter((f) => f === "Cascadia Code"),
    ).toHaveLength(1);
  });

  it("never checks bundled fonts for installation", () => {
    const asked: string[] = [];
    installedFonts((cssFont) => {
      asked.push(cssFont);
      return 400;
    });
    expect(asked.some((cssFont) => BUNDLED_FONTS.some((font) => cssFont.includes(font)))).toBe(
      false,
    );
  });

  it("keeps saved fonts from earlier versions valid", () => {
    for (const font of [
      "Cascadia Mono",
      "Cascadia Code",
      "JetBrains Mono",
      "Fira Code",
      "Consolas",
    ]) {
      expect(isTerminalFont(font), font).toBe(true);
    }
  });
});

describe("font loading", () => {
  it("loads regular and bold of bundled fonts only", () => {
    expect(fontLoadQueries("Hack")).toEqual(['16px "Hack"', 'bold 16px "Hack"']);
    expect(fontLoadQueries("Consolas")).toEqual([]);
    expect(fontLoadQueries("default")).toEqual([]);
    expect(isBundledFont("Geist Mono")).toBe(true);
    expect(isBundledFont("Cascadia Mono")).toBe(false);
  });

  it("is ready at once for fonts that need no loading", () => {
    const loader = createFontLoader(() => Promise.reject(new Error("not expected")));
    expect(loader.isReady("default")).toBe(true);
    expect(loader.isReady("Consolas")).toBe(true);
  });

  it("becomes ready after loading and loads each font once", async () => {
    const queries: string[] = [];
    const loader = createFontLoader((query) => {
      queries.push(query);
      return Promise.resolve([]);
    });
    expect(loader.isReady("JetBrains Mono")).toBe(false);
    await Promise.all([loader.load("JetBrains Mono"), loader.load("JetBrains Mono")]);
    expect(loader.isReady("JetBrains Mono")).toBe(true);
    await loader.load("JetBrains Mono");
    expect(queries).toEqual(['16px "JetBrains Mono"', 'bold 16px "JetBrains Mono"']);
  });

  it("needs the bundled Cascadia for the default only in WebKit without the Windows fonts", () => {
    expect(defaultUsesBundledFont(fakeMeasure(["DejaVu Sans Mono"]))).toBe(false);
    setBundledMonoFont(true);
    expect(defaultUsesBundledFont(fakeMeasure(["Cascadia Mono", "Consolas"]))).toBe(false);
    expect(defaultUsesBundledFont(fakeMeasure(["Consolas"]))).toBe(false);
    expect(defaultUsesBundledFont(fakeMeasure(["DejaVu Sans Mono", "Menlo"]))).toBe(true);
    expect(defaultUsesBundledFont(null)).toBe(false);
    setBundledMonoFont(false);
    expect(fontLoadQueries("default", true)).toEqual([
      '16px "Hangar Cascadia Mono"',
      'bold 16px "Hangar Cascadia Mono"',
    ]);
  });

  it("keeps the default ready at once where a Windows font is installed", () => {
    let asked = 0;
    const loader = createFontLoader(
      () => Promise.reject(new Error("not expected")),
      () => {
        asked += 1;
        return false;
      },
    );
    expect(loader.isReady("default")).toBe(true);
    expect(loader.isReady("default")).toBe(true);
    expect(asked).toBe(1);
  });

  it("waits for the bundled Cascadia before the default is ready on macOS and Linux", async () => {
    const queries: string[] = [];
    const loader = createFontLoader(
      (query) => {
        queries.push(query);
        return Promise.resolve([]);
      },
      () => true,
    );
    expect(loader.isReady("default")).toBe(false);
    await loader.load("default");
    expect(loader.isReady("default")).toBe(true);
    expect(queries).toEqual(['16px "Hangar Cascadia Mono"', 'bold 16px "Hangar Cascadia Mono"']);
  });

  it("counts a failed font as ready, so terminals fall back instead of waiting", async () => {
    const loader = createFontLoader(() => Promise.reject(new Error("missing")));
    await expect(loader.load("Fira Code")).resolves.toBeUndefined();
    expect(loader.isReady("Fira Code")).toBe(true);
  });
});

describe("validation", () => {
  it("accepts only known values", () => {
    expect(isCursorStyle("bar")).toBe(true);
    expect(isCursorStyle("beam")).toBe(false);
    expect(isTerminalFont("default")).toBe(true);
    expect(isTerminalFont("Fira Code")).toBe(true);
    expect(isTerminalFont("Comic Sans MS")).toBe(false);
    expect(isTerminalFont(3)).toBe(false);
  });
});

describe("font lists", () => {
  it("names every font once, bundled and system fonts apart", () => {
    const all: string[] = [...BUNDLED_FONTS, ...SYSTEM_FONTS];
    expect(new Set(all).size).toBe(all.length);
  });

  it("offers the common system fonts of every platform when installed", () => {
    const installed = ["Ubuntu Mono", "Menlo", "SF Mono", "DejaVu Sans Mono", "Lucida Console"];
    expect(installedFonts(fakeMeasure(installed))).toEqual([
      "Lucida Console",
      "SF Mono",
      "Menlo",
      "DejaVu Sans Mono",
      "Ubuntu Mono",
    ]);
  });

  it("loads the new bundled fonts like the others", () => {
    for (const font of ["Source Code Pro", "IBM Plex Mono", "Victor Mono"] as const) {
      expect(isBundledFont(font), font).toBe(true);
      expect(fontLoadQueries(font)).toEqual([`16px "${font}"`, `bold 16px "${font}"`]);
    }
  });

  it("ships regular, bold, a license and an @font-face rule per weight for every bundled font", () => {
    const css = readFileSync(new URL("./fonts.css", import.meta.url), "utf8");
    const files = Object.keys(FONT_FILES);
    for (const font of BUNDLED_FONTS) {
      const slug = font.toLowerCase().replaceAll(" ", "-");
      const folder = `./assets/fonts/${slug}/`;
      const inFolder = files.filter((file) => file.startsWith(folder));
      expect(inFolder, font).toContain(`${folder}${slug}-regular.woff2`);
      expect(inFolder, font).toContain(`${folder}${slug}-bold.woff2`);
      expect(
        inFolder.some((file) => /(OFL\.txt|LICENSE\.md)$/.test(file)),
        `${font} license`,
      ).toBe(true);
      for (const weight of ["400", "700"]) {
        const rule = new RegExp(`font-family: "${font}";\\s*font-weight: ${weight};`);
        expect(css, `${font} ${weight}`).toMatch(rule);
      }
    }
  });
});
