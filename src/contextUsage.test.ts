import { describe, expect, it } from "vitest";

import {
  compactTokens,
  contextHint,
  contextShare,
  formatOutputRate,
  formatShare,
  HIGH_SHARE,
  parseContextSnapshot,
  parseOutputRates,
} from "./contextUsage";
import { createTranslator } from "./i18n";

describe("parseContextSnapshot", () => {
  it("reads the backend's snapshot by session id", () => {
    const parsed = parseContextSnapshot({
      sessions: [
        { sessionId: 4, used: 123_000, window: 1_000_000 },
        { sessionId: 7, used: 20_020, window: 200_000 },
      ],
    });
    expect(parsed).toEqual(
      new Map([
        [4, { used: 123_000, window: 1_000_000 }],
        [7, { used: 20_020, window: 200_000 }],
      ]),
    );
  });

  it("drops broken entries and ignores other payloads", () => {
    const parsed = parseContextSnapshot({
      sessions: [
        null,
        { sessionId: "4", used: 1, window: 2 },
        { sessionId: 5, used: -1, window: 2 },
        { sessionId: 6, used: 1.5, window: 2 },
        { sessionId: 7, used: 1, window: 0 },
        { sessionId: 8, used: 1, window: 2 },
      ],
    });
    expect([...(parsed?.keys() ?? [])]).toEqual([8]);
    expect(parseContextSnapshot(null)).toBeNull();
    expect(parseContextSnapshot([])).toBeNull();
    expect(parseContextSnapshot({ sessions: 3 })).toBeNull();
    expect(parseContextSnapshot({ sessions: [] })).toEqual(new Map());
  });
});

describe("formatting", () => {
  it("computes the share, capped at full", () => {
    expect(contextShare({ used: 120_000, window: 1_000_000 })).toBeCloseTo(0.12);
    expect(contextShare({ used: 300_000, window: 200_000 })).toBe(1);
    expect(contextShare({ used: 5, window: 0 })).toBe(0);
    expect(HIGH_SHARE).toBe(0.8);
  });

  it("formats the share in the language's percent style", () => {
    expect(formatShare(0.12, "en")).toBe("12%");
    // German puts a (narrow) no-break space before the sign.
    expect(formatShare(0.12, "de")).toMatch(/^12\s%$/u);
    expect(formatShare(0.125, "en")).toBe("13%");
    expect(formatShare(1, "en")).toBe("100%");
  });

  it("writes compact token counts without a trailing .0", () => {
    expect(compactTokens(123_000)).toBe("123k");
    expect(compactTokens(96_300)).toBe("96.3k");
    expect(compactTokens(1_000_000)).toBe("1M");
    expect(compactTokens(1_250_000)).toBe("1.3M");
    expect(compactTokens(950)).toBe("950");
  });

  it("describes the use with compact and exact numbers", () => {
    const t = createTranslator("en");
    expect(contextHint({ used: 123_456, window: 1_000_000 }, "en", t)).toBe(
      "Context 12% full: 123.5k / 1M tokens\nExactly: 123,456 of 1,000,000 tokens",
    );
  });
});

describe("the output rate", () => {
  it("reads the rates by session id and leaves out sessions without one", () => {
    const rates = parseOutputRates({
      sessions: [
        { sessionId: 4, used: 123_000, window: 1_000_000, outputRate: 84 },
        { sessionId: 5, used: 1_000, window: 200_000, outputRate: null },
        { sessionId: 6, used: 1_000, window: 200_000 },
        { sessionId: 7, used: 1_000, window: 200_000, outputRate: 0 },
        { sessionId: 8, used: 1_000, window: 200_000, outputRate: -3 },
        { sessionId: 9, used: 1_000, window: 200_000, outputRate: 12.5 },
        { sessionId: "x", outputRate: 40 },
      ],
    });
    expect(rates).toEqual(new Map([[4, 84]]));
    expect(parseOutputRates({ sessions: "none" })).toBeNull();
    expect(parseOutputRates(null)).toBeNull();
  });

  it("keeps the context use as it was", () => {
    const parsed = parseContextSnapshot({
      sessions: [{ sessionId: 4, used: 123_000, window: 1_000_000, outputRate: 84 }],
    });
    expect(parsed?.get(4)).toEqual({ used: 123_000, window: 1_000_000 });
  });

  it("writes whole numbers in the UI language", () => {
    expect(formatOutputRate(84, "en")).toBe("84");
    expect(formatOutputRate(1234, "en")).toBe("1,234");
    expect(formatOutputRate(1234, "de")).toBe("1.234");
    const t = createTranslator("de");
    expect(t("speed.value", { value: formatOutputRate(42, "de") })).toBe("42 tok/s");
    expect(t("speed.hint", { value: "42" })).toBe(
      "Claude Code erzeugt gerade 42 Token pro Sekunde",
    );
  });
});
