---
title: "Measurement: app fonts and form controls, Chromium against WebKit"
type: source
tags: [fonts, selawik, cascadia, webkit, form-controls, scrollbars, measurement]
sources: []
raw: raw/2026-11-20-app-fonts-and-controls-measurement.md
url:
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Measurement: app fonts and form controls, Chromium against WebKit

Own measurement before bundling UI fonts and restyling controls for WebKit. Record:
raw/2026-11-20-app-fonts-and-controls-measurement.md.

## Key points
- Selawik 1.01 (upstream woff2, about 14.5 KB per weight) covers Latin only: everything in de,
  en, es, fr, it, pl, pt, tr except four arrows and U+2265; no Greek, Cyrillic or CJK.
- Selawik keeps Segoe UI's metrics: a 13px sample line is 461.1px in Segoe UI and 463.4px in
  Selawik, line height 17px in both.
- Cascadia Mono 2407.24 as one variable woff2 (210 KB, `wght` 200..700) is smaller than its
  regular and bold static files together (290 KB); it covers Greek and Cyrillic, not CJK.
- Playwright WebKit draws selects 8px taller, sliders and buttons taller, progress bars thinner
  and scrollbars as overlays; with `src/webkit.css` every measured box matches Edge.
- WebKit applies a custom scrollbar only when the element renders with the rule in place or a
  scrollbar property changes.

## Significance for Hangar
- Basis of [[0018-same-look-on-every-platform]]: Selawik and Cascadia Mono ship in
  `src/assets/fonts/`, and WebKit gets controls drawn like WebView2's.
- Greek, Russian, Japanese and Chinese UI text uses the system font on macOS and Linux.
