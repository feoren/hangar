# Measurement: glyph fallback into the bundled Cascadia Mono in Chromium

Date: 2026-11-20. Platform: Windows 11 Pro 10.0.26300 with Cascadia Mono and Consolas installed.
Browser: Microsoft Edge (Playwright 1.65.1, channel `msedge`). Page with `src/fonts.css`,
`src/styles.css`, `src/webkit.css`; `Hangar Cascadia Mono` loaded before measuring. Width of a
span at 16px, `white-space: pre`.

Samples: Arabic "مرحبا بالعالم", Braille U+280B U+2819 U+2839 U+2838 U+283C U+2834 U+2826 U+2827,
U+276F.

| Stack | Arabic | Braille | U+276F |
| --- | --- | --- | --- |
| old: "Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace | 69.14 | 75.00 | 9.38 |
| old, Windows 10 without Cascadia: Consolas, "DejaVu Sans Mono", monospace | 68.56 | 96.44 | 8.20 |
| with "Hangar Cascadia Mono" after Consolas | 121.88 | 75.00 | 9.38 |
| Windows 10 stack with "Hangar Cascadia Mono" after Consolas | 121.30 | 75.00 | 9.38 |
| `var(--mono)` in Chromium after the change | 69.14 | 75.00 | 9.38 |
| `fontStack("default")` in Chromium after the change | 69.14 | 75.00 | 9.38 |

The page reported `data-engine="chromium"`; `fontStack("default")` returned
`"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace`.
