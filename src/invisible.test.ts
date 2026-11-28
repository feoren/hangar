import { describe, expect, it } from "vitest";

import { hasInvisible, invisibleSegments, placeholder, revealInvisible } from "./invisible";

describe("revealInvisible", () => {
  it("shows bidi controls as placeholders (Trojan Source)", () => {
    const line = "if (isAdmin) { \u202e } \u2066// Check\u2069 { begin admin only";
    expect(revealInvisible(line)).toBe(
      "if (isAdmin) { <U+202E> } <U+2066>// Check<U+2069> { begin admin only",
    );
  });

  it("covers zero-width characters, marks, BOM and C0 controls but keeps tab", () => {
    expect(revealInvisible("a\u200bb\u200fc\u061cd\ufeffe")).toBe(
      "a<U+200B>b<U+200F>c<U+061C>d<U+FEFF>e",
    );
    expect(revealInvisible("x\u0000y\u001bz\u007f")).toBe("x<U+0000>y<U+001B>z<U+007F>");
    expect(revealInvisible("w\u2060j\u2064k\u2028l\u2029m\u0085n\u009bo")).toBe(
      "w<U+2060>j<U+2064>k<U+2028>l<U+2029>m<U+0085>n<U+009B>o",
    );
    expect(revealInvisible("tab\there")).toBe("tab\there");
    expect(revealInvisible("Grüße 日本")).toBe("Grüße 日本");
  });

  it("finds them and splits into marked segments", () => {
    expect(hasInvisible("plain")).toBe(false);
    expect(hasInvisible("a\u202eb")).toBe(true);
    expect(invisibleSegments("plain")).toEqual([{ text: "plain", hidden: false }]);
    expect(invisibleSegments("\u202eab\u200b")).toEqual([
      { text: "<U+202E>", hidden: true },
      { text: "ab", hidden: false },
      { text: "<U+200B>", hidden: true },
    ]);
    expect(placeholder("\u0007")).toBe("<U+0007>");
  });

  it("covers the soft hyphen, fillers and deprecated format characters", () => {
    for (const char of ["\u00ad", "\u115f", "\u1160", "\u180e", "\u3164", "\uffa0"]) {
      expect(hasInvisible(`a${char}b`)).toBe(true);
    }
    expect(revealInvisible("a\u206ab\u206fc\ufff9d\ufffbe")).toBe(
      "a<U+206A>b<U+206F>c<U+FFF9>d<U+FFFB>e",
    );
    expect(hasInvisible("plain text, ä ö ü")).toBe(false);
  });

  // Tag characters show nothing, but a language model reads them: "ASCII smuggling".
  it("finds tag characters as one code point each", () => {
    const hidden = "ok\u{e0069}\u{e0067}\u{e006e}";
    expect(hasInvisible(hidden)).toBe(true);
    expect(hasInvisible("\u{e0000}")).toBe(true);
    expect(hasInvisible("\u{e007f}")).toBe(true);
    expect(hasInvisible("\u{e0080}")).toBe(false);
    expect(revealInvisible(hidden)).toBe("ok<U+E0069><U+E0067><U+E006E>");
    expect(invisibleSegments("a\u{e0041}")).toEqual([
      { text: "a", hidden: false },
      { text: "<U+E0041>", hidden: true },
    ]);
  });
});
