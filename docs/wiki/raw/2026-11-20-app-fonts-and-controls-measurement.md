# Measurement: app fonts and form controls, Chromium against WebKit

Date: 2026-11-20. Platform: Windows 11 Pro 10.0.26300. Browsers: Microsoft Edge (Playwright
channel `msedge`, the WebView2 engine) and Playwright 1.65.1 WebKit (user agent
`AppleWebKit/605.1.15 ... Version/27.2 Safari/605.1.15`). Fonts read with fontTools 4.66.0.

## Font files
- Selawik: GitHub release `1.01` of microsoft/Selawik, `Selawik_Release.zip`. Files used:
  `selawk.woff2` (14632 bytes), `selawksb.woff2` (14896), `selawkb.woff2` (14424). Name table:
  "Selawik", "Version 1.01". License: repository `LICENSE.txt` (OFL 1.1, Reserved Font Name
  Selawik).
- Cascadia Mono: GitHub release `v2407.24` of microsoft/cascadia-code,
  `CascadiaCode-2407.24.zip`, file `woff2/CascadiaMono.woff2` (210484 bytes), variable, axis
  `wght` 200..700, default 400. Name table: "Version 2407.024". Static alternatives:
  `CascadiaMono-Regular.woff2` 143932, `CascadiaMono-Bold.woff2` 145920. License: repository
  `LICENSE` (OFL 1.1, Reserved Font Name Cascadia Code).

## Coverage against src/i18n.ts
Characters above U+0020 in each language's strings, missing from the font's cmap (same cmap in
the .ttf of the same release):

| Language | Selawik missing | Cascadia Mono missing |
| --- | --- | --- |
| de, en, es, fr, it, pl, pt, tr | 4: U+2191 U+2192 U+2193 U+2265 | 0 |
| el | 69 (65 Greek) | 0 |
| ru | 63 (59 Cyrillic) | 0 |
| ja | 602 (CJK, kana) | 598 |
| zh | 777 (CJK) | 760 |

Selawik cmap: 348 code points, ranges U+0020..U+017E (with gaps), U+0192, U+0218..U+021B,
U+02C6..U+02DD, U+0300..U+0336, U+1E80..U+1EF3, general punctuation U+2013..U+203A, U+20AC,
U+2122.

## Metrics (fixture page with the real classes of src/styles.css, 13px)
- Sample line "Neue Session starten · Arbeitsordner wählen · Größe ändern · ÄÖÜ ß 0123456789":
  Segoe UI 461.1 x 17.0 px, Selawik 463.4 x 17.0 px (both engines).
- Edge native (CSS px): checkbox and radio 16 x 16 as styled; `.select` 220 x 31; select with
  `.text-input` and auto width 97 x 29; range 166 x 16; plain button 39.6 x 19; progress 130 x 13;
  scrollbar 15 px wide (thumb 9 px, inset 3 px, arrow buttons), `scrollbar-width: thin` 10 px.
- Native Chromium colors read from screenshots: dark scheme control background #3b3b3b, border
  #858585, check mark #3b3b3b on accent #5aa9ff, progress #99c8ff, scrollbar track #2c2c2c,
  thumb #9f9f9f; light scheme background #ffffff, border #767676, range track #efefef, check mark
  #ffffff on accent #0969da, progress #0075ff, scrollbar track #fcfcfc, thumb #8b8b8b.
- WebKit before `src/webkit.css`: select 220 x 39 and 103 x 37, range 166 x 20, button 39.6 x 22,
  progress 130 x 6, no scrollbar room (overlay). After: all the Edge values above.
- Edge native text fields (dark): Arial 13.3333px, padding 1px 2px, layout border 2px (style
  inset, #858585) of which only the outer device-independent pixel is painted, background
  #3b3b3b, square corners, 177 x 21. Text area: monospace (Consolas) 13.3333px, padding 2px,
  border 1px #858585. Date and time without a class: monospace; with `.text-input` 132 x 29 and
  84.6 x 29.6 including the picker icon. Color field: 50 x 27, padding 1px 2px, background
  #6b6b6b (light #efefef).
- Playwright WebKit on Windows before: font "MS Shell Dlg", radius 5px, background #1e1e1e,
  date, time and color drawn as plain text fields (no picker in this build). After: text
  fields 21px high with Chromium's colors and font, text area 36px like Edge, date and time
  with `.text-input` 29px high.
- WebKit rebuilds a custom scrollbar only when a scrollbar property changes, so the engine
  attribute has to be set before the elements render (main.tsx does it before React renders).
