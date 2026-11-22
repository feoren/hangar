---
title: "Measurement: size and cost of the figures' idle loops"
type: source
tags: [figures, sub-agents, performance, bundle, measurement, windows]
sources: []
raw: raw/2026-11-22-figure-motion-measurement.md
url: https://chromedevtools.github.io/devtools-protocol/tot/Page/
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: size and cost of the figures' idle loops

Three test loops (animated WebP, 160 x 240, 80 frames) measured for bundle size, place of the
figure against its still file, and CPU and memory with ten visible figures in headless Edge
158.0.4370.33 on Windows 11 (`raw/2026-11-22-figure-motion-measurement.md`).

## Key points
- `dist` grows by the files alone: 7.98 MB to 11.51 MB with three loops (mean 1.17 MB each); 41
  loops come to about 48 MB. The script chunks only gain the URLs.
- The figure in the first frame lies within 1.3 px of the still file scaled to 160 x 240, so
  swapping the two does not move it.
- Ten visible loops: about 2 to 8 % of one core in headless Edge with software rendering, ten
  stills about 0 to 1 % once settled. Memory differences stay inside the browser's own noise of
  tens of MB.

## Significance for Hangar
The loops cost little CPU while shown and nothing while still, which supports animating only
visible figures in a visible window ([[sub-agent-library]] "Idle loops"). The size is the real
cost: about 48 MB more in every installer and update. Not measured: WebView2 with GPU
compositing.
