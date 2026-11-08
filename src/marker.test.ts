import { describe, expect, it } from "vitest";

import {
  hasMarker,
  MARKER_COLORS,
  MARKER_EMOJIS,
  markerColorName,
  normalizeMarkerColor,
  parseEmoji,
  parseMarker,
  storedEmoji,
} from "./marker";

describe("colors", () => {
  it("accept #rrggbb in any case and store it lowercase", () => {
    expect(normalizeMarkerColor("#1A2b3C")).toBe("#1a2b3c");
    for (const bad of ["", "red", "#123", "#12345g", "123456", null, 5]) {
      expect(normalizeMarkerColor(bad)).toBeNull();
    }
  });

  it("have names for the palette only", () => {
    expect(markerColorName("#E5534B")).toBe("marker.red");
    expect(markerColorName("#000000")).toBeNull();
    expect(markerColorName(null)).toBeNull();
  });

  it("form a palette of eight distinct valid colors", () => {
    expect(MARKER_COLORS).toHaveLength(8);
    expect(new Set(MARKER_COLORS.map((entry) => entry.color)).size).toBe(8);
    for (const entry of MARKER_COLORS) expect(normalizeMarkerColor(entry.color)).toBe(entry.color);
  });
});

describe("emoji", () => {
  it("accepts short text up to 16 bytes", () => {
    expect(parseEmoji("\u{1F525}")).toBe("\u{1F525}");
    expect(parseEmoji("  \u{2B50} ")).toBe("\u{2B50}");
    // An emoji sequence with a joiner (technologist): 11 bytes.
    expect(parseEmoji("\u{1F468}\u{200D}\u{1F4BB}")).toBe("\u{1F468}\u{200D}\u{1F4BB}");
    expect(parseEmoji("v2")).toBe("v2");
  });

  it("treats empty input as no emoji", () => {
    expect(parseEmoji("")).toBeNull();
    expect(parseEmoji("   ")).toBeNull();
    expect(parseEmoji(null)).toBeNull();
    expect(parseEmoji(undefined)).toBeNull();
  });

  it("rejects long text and control characters", () => {
    expect(parseEmoji("\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}")).toBeUndefined();
    expect(parseEmoji("a\nb")).toBeUndefined();
    expect(parseEmoji("\u0007")).toBeUndefined();
    expect(parseEmoji(42)).toBeUndefined();
    expect(storedEmoji("a\nb")).toBeNull();
  });

  it("offers only valid quick picks", () => {
    for (const emoji of MARKER_EMOJIS) expect(parseEmoji(emoji)).toBe(emoji);
  });
});

describe("parseMarker", () => {
  it("reads color and emoji, dropping invalid parts", () => {
    expect(parseMarker({ color: "#AABBCC", emoji: "\u{1F680}" })).toEqual({
      color: "#aabbcc",
      emoji: "\u{1F680}",
    });
    expect(parseMarker({ color: "blue", emoji: "" })).toEqual({ color: null, emoji: null });
    expect(parseMarker(null)).toEqual({ color: null, emoji: null });
  });

  it("knows when there is something to show", () => {
    expect(hasMarker(undefined)).toBe(false);
    expect(hasMarker({ color: null, emoji: null })).toBe(false);
    expect(hasMarker({ color: "#e5534b", emoji: null })).toBe(true);
  });
});
