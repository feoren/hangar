import { describe, expect, it } from "vitest";

import { attentionEdges, attentionTint, textOnColor } from "./attentionLook";
import { CREAM, DEFAULT_LOOKS, LOOK_STATES } from "./stateLook";
import { lookVariables } from "./stateLookCss";

describe("lookVariables", () => {
  it("sets the general attention variables from done, as before", () => {
    for (const theme of ["dark", "light"] as const) {
      const vars = lookVariables(DEFAULT_LOOKS, theme);
      const edges = attentionEdges(CREAM, theme);
      expect(vars["--attention"]).toBe(CREAM);
      expect(vars["--on-attention"]).toBe(textOnColor(CREAM));
      expect(vars["--attention-strong"]).toBe(edges.strong);
      expect(vars["--attention-chip-bg"]).toBe(edges.chipBg);
    }
  });

  it("breathes waiting rows as before and leaves working and pause still", () => {
    const vars = lookVariables(DEFAULT_LOOKS, "dark");
    const tint = attentionTint(CREAM, "dark");
    expect(vars["--look-done-low"]).toBe(`${String(tint.low)}%`);
    expect(vars["--look-done-high"]).toBe(`${String(tint.high)}%`);
    expect(vars["--look-done-seconds"]).toBe("2.8s");
    expect(vars["--look-done-pulse"]).toBe("row-breathe");
    expect(vars["--look-asks-pulse"]).toBe("row-breathe");
    expect(vars["--look-working-pulse"]).toBe("none");
    expect(vars["--look-working-still"]).toBe("0%");
    expect(vars["--look-idle-pulse"]).toBe("none");
  });

  it("keeps the theme's green for working and pause until a color is chosen", () => {
    const vars = lookVariables(DEFAULT_LOOKS, "light");
    expect(vars["--look-working-strong"]).toBe("var(--ok)");
    expect(vars["--look-idle-color"]).toBe("var(--ok)");
    const custom = lookVariables(
      { ...DEFAULT_LOOKS, working: { color: "#8cc8ff", pulse: "strong", seconds: 1 } },
      "light",
    );
    expect(custom["--look-working-strong"]).toBe(attentionEdges("#8cc8ff", "light").strong);
    // The chip label is text: it needs 4.5 : 1, more than the 3 : 1 of dot and edge.
    expect(vars["--look-working-text"]).toBe("var(--ok)");
    expect(custom["--look-working-text"]).toBe(attentionEdges("#8cc8ff", "light").text);
    expect(custom["--look-working-pulse"]).toBe("row-breathe");
    expect(custom["--look-working-seconds"]).toBe("1s");
  });

  it("keeps the theme's red for an error until a color is chosen", () => {
    const vars = lookVariables(DEFAULT_LOOKS, "light");
    expect(vars["--look-failed-color"]).toBe("var(--error)");
    expect(vars["--look-failed-strong"]).toBe("var(--error)");
    // Red text needs the darker shade in the light theme.
    expect(vars["--look-failed-text"]).toBe("var(--chip-failed-fg)");
    expect(vars["--look-failed-pulse"]).toBe("row-breathe");
    const custom = lookVariables(
      { ...DEFAULT_LOOKS, failed: { color: "#f0b429", pulse: "off", seconds: 2.8 } },
      "dark",
    );
    expect(custom["--look-failed-strong"]).toBe(attentionEdges("#f0b429", "dark").strong);
    expect(custom["--look-failed-text"]).toBe(attentionEdges("#f0b429", "dark").text);
    expect(custom["--look-failed-pulse"]).toBe("none");
  });

  it("defines every variable for every state", () => {
    const vars = lookVariables(DEFAULT_LOOKS, "dark");
    for (const state of LOOK_STATES) {
      for (const part of ["color", "strong", "ring", "chip-bg", "chip-fg", "still", "ring-pulse"]) {
        expect(vars[`--look-${state}-${part}`]).toBeTruthy();
      }
    }
  });
});
