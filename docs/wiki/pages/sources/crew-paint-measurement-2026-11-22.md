---
title: "Measurement: paints of the crew column at rest"
type: source
tags: [crew, sub-agents, performance, paint, power-saving, reduced-motion, measurement, windows]
sources: []
raw: raw/2026-11-22-crew-paint-measurement.md
url: https://chromedevtools.github.io/devtools-protocol/tot/Tracing/
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: paints of the crew column at rest

A test page with the real `CrewColumn`, `styles.css` and figures (one sub-agent per state, column
and strip) traced for 3 s in headless Edge 158.0.4370.33 on Windows 11, in the normal mode, with
reduced motion and with power saving (`raw/2026-11-22-crew-paint-measurement.md`).

## Key points
- Before and after the redesign of 2026-11-22: 0 `Paint` and 0 `Layout` events in every mode.
- After it, the normal mode shows about 13 `PrePaint` events per second while the typing dots and
  the waiting ring breathe; both animate opacity only, so nothing is repainted.
- Reduced motion and power saving: 0 `PrePaint`, the column stands completely still.

## Significance for Hangar
Opacity-only loops keep the crew within the rule of no repaint at rest; the stills for reduced
motion and power saving stop even the animation ticks ([[crew-column]]).
