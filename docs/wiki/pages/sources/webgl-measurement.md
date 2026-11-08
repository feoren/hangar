---
title: WebGL addon vs DOM renderer measurement
type: source
tags: [xterm, webgl, performance, webview2]
sources: []
created: 2026-11-08
updated: 2026-11-08
raw: raw/2026-11-08-webgl-measurement.md
url: https://www.npmjs.com/package/@xterm/addon-webgl
retrieved: 2026-11-08
---

# WebGL addon vs DOM renderer

Own measurement, record in [[2026-11-08-webgl-measurement]] (`raw/2026-11-08-webgl-measurement.md`). xterm.js 6.0.0, `@xterm/addon-webgl` 0.19.0, Edge 157.0.4341.42 (same Chromium as the installed WebView2 runtime 157.0.4341.42), RTX 4070, 60 Hz, DPR 1. Terminal 200x50, scrollback 10000, 3 runs per cell. Time to the last write callback, median of 3 (ms):

| Scenario | DOM | WebGL | Result |
|---|---|---|---|
| a: 21 MB plain text | 266 | 321 | no difference (noise 208 to 437) |
| b: 21 MB truecolor, random RGB per word | 432 | 1681 | WebGL about 4x slower, 15 to 17 long tasks, max 152 ms |
| c: 2000 full-screen redraws, random RGB | 117 | 185 | DOM faster, no long tasks |
| d: 21 MB truecolor, 16-color palette | 338 | 253 | WebGL about 25 percent faster at best, run 1 DOM 651 vs 254 |
| e: 2000 redraws, 16-color palette | 106 | 111 | no difference |

## Key points

- Parsing, not drawing, dominates: xterm renders once per animation frame, so the write callback (parse) is nearly independent of the renderer. Both renderers kept 60 Hz frame cadence and produced no long tasks except WebGL in scenario b.
- WebGL is not clearly faster in any scenario. Its only gain was scenario d (palette-based truecolor), and it was small and within the DOM variance of its first run.
- WebGL is clearly worse for many unique colors: the glyph texture atlas has to be rebuilt per glyph and color combination, which produced long tasks of up to 152 ms.
- Not measured: paced-stream frame intervals (jank under a steady stream), DPR other than 1, a real WebView2 host, memory in MB (JS heap numbers are GC noise; GPU memory not recorded), context loss under GPU reset or sleep. The run was stopped on request, so the data is a single machine, one GPU and 3 runs.

## Recommendation

Skip WebGL (keep the DOM renderer as default); at most offer it later as an off-by-default experimental option once paced-frame, DPR 1.25/1.5 and WebView2-host data exist. Reason: no measurable speed-up on this hardware, and a 4x slowdown on color-heavy output.

## Risks if enabled (not measured here unless stated)

- Context loss (GPU reset, sleep, driver update): the addon fires `onContextLoss`; the terminal must be rebuilt with the DOM renderer. No loss occurred in the runs. > [!question] Unverified recovery behavior in WebView2.
- Memory: each WebGL terminal holds a canvas and glyph atlas on the GPU; with many sessions this multiplies. > [!question] Unverified, not measured.
- Blurry text at fractional DPI (125 percent, 150 percent common on Windows): canvas text can look softer than DOM text. > [!question] Unverified, DPR 1 only.
- A browser limits the number of live WebGL contexts (around 16 in Chromium); a session count above that would lose contexts. > [!question] Unverified for WebView2.
