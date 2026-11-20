---
title: "ADR 0018: Same look on every platform"
type: decision
status: accepted
superseded_by:
tags: [fonts, webkit, webkitgtk, wkwebview, form-controls, scrollbars, ci, linux, macos, platform]
sources: ["[[app-fonts-and-controls-measurement]]", "[[app-fonts-and-controls-measurement-2]]", "[[terminal-font-coverage-measurement]]"]
created: 2026-11-20
updated: 2026-11-20
---

# ADR 0018: Same look on every platform

## Context
Hangar renders in WebView2 (Chromium) on Windows, WKWebView on macOS and WebKitGTK on Linux
([[0013-linux-release]], [[0017-macos-release]]). Two things made macOS and Linux look different
from Windows:

- **Fonts.** The UI font stack named Segoe UI and the monospace stack Cascadia Mono. Neither exists
  on macOS or Linux, so text fell back to SF Pro or Cantarell and DejaVu Sans Mono, with other
  widths: labels wrapped and columns moved.
- **Controls.** Checkboxes, radios, sliders, selects, progress bars and scrollbars are drawn by
  the engine. WebKit draws them its own way (a gray select with a triangle and a taller box, a
  white check mark, a slider with a gray thumb, overlay scrollbars that take no room).

## Decision

### Fonts
- **Selawik 1.01** (Microsoft, `github.com/microsoft/Selawik`, release `1.01`, OFL 1.1 with the
  Reserved Font Name "Selawik"), the open fallback for Segoe UI with matching metrics. Shipped as
  the upstream woff2 files, unmodified: Regular, Semibold, Bold (about 14.5 KB each, 44 KB in all;
  Light and Semilight are unused). In the stack directly after Segoe UI, so Windows never loads it.
  Measured: a 13px sample line is 461.1px wide in Segoe UI and 463.4px in Selawik, same line
  height ([[app-fonts-and-controls-measurement]]). Selawik has no weights 500 and 650, which
  Segoe UI Variable draws on Windows 11; on macOS and Linux they resolve to Regular and Bold,
  the Windows 10 look (Segoe UI without the variable font does the same).
- **Cascadia Mono 2407.24** (Microsoft, `github.com/microsoft/cascadia-code`, tag `v2407.24`, OFL
  1.1 with the Reserved Font Name "Cascadia Code"): the variable woff2 (weights 200 to 700, 210 KB),
  unmodified, cheaper than two static weights (144 KB + 146 KB) and closer to the variable font
  Windows 11 has. Registered under its own family name `Hangar Cascadia Mono`, so the terminal
  font picker, which detects an installed "Cascadia Mono" by measuring, is not confused by a web
  font of that name.
- **Stack per engine.** Chromium (Windows) keeps the old monospace stack byte for byte:
  `"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace`. Only WebKit gets
  `Hangar Cascadia Mono` after Consolas: `--mono` is overridden in `src/webkit.css`, and
  `fontStack` (terminal, file viewer, the previews in Settings) adds it once `main.tsx` has
  called `setBundledMonoFont` for WebKit. In Chromium the bundled face would not stay unused
  behind an installed Cascadia: it draws every glyph the installed Cascadia (2102) or Consolas
  lacks, in place of the system fallback, with other widths
  ([[app-fonts-and-controls-measurement-2]]: Arabic at 16px 69.14px with the old stack, 121.88px
  with the bundled face in it; on the Windows 10 stack Braille 96.44px against 75.00px and U+276F
  8.20px against 9.38px).
- Files and licenses in `src/assets/fonts/selawik/` and `src/assets/fonts/cascadia-mono/`, faces
  in `src/fonts.css` with `font-display: block` (local files, no flash of the fallback).
- **Coverage** (own `cmap` check against the strings of the 12 languages in `src/i18n.ts`):
  Selawik covers Latin only (348 code points: Latin-1, Latin Extended-A, so de, en, es, fr, it,
  pl, pt, tr are complete apart from the arrows U+2191..U+2193 and U+2265). Greek (el), Cyrillic
  (ru) and CJK (ja, zh) come from the system font on macOS and Linux, as before. A
  `unicode-range` on the Selawik faces keeps the browser from loading them for those scripts.
  Cascadia Mono covers el and ru fully; CJK falls back.
- **Terminal default** (`src/terminalLook.ts`): in WebKit the default stack ends at
  `Hangar Cascadia Mono` as well. xterm measures the cell size once, so the font loader waits for
  the bundled face before the default counts as ready, but only where it is in the stack and none
  of Cascadia Mono, Cascadia Code and Consolas is installed (measured once on a canvas); in
  Chromium the default is ready at once, as before. Until then a terminal draws in the stack
  without the bundled face (`waitingStack`), which differs from the stack of every font that has
  to load, so the switch makes xterm measure anew.

### Controls
- `src/engine.ts` sets `data-engine="webkit"` on the root when the user agent names AppleWebKit
  without a Chromium token, before the first render. `src/webkit.css` redraws the controls for
  that engine only, the way Chromium draws them on Windows: checkbox and radio (13px default,
  Chromium's colors, dark check mark on the light dark-theme accent, white on the light-theme
  accent), slider (bordered 8px track filled to the thumb from `--range-ratio`, set by
  `src/rangeFill.ts`, 15px thumb), select (author box plus Chromium's extra 1px above and below,
  room and chevron at the end), progress bar, search field, button box, text fields and text
  areas with Chromium's user agent box (Arial 13.33px or a monospace font, square corners, the
  outer pixel of a 2px border painted), date and time fields with Chromium's extra line height
  and a calendar or clock icon at the end, color fields with Chromium's gray button, and the classic
  scrollbar (15px with arrow buttons, 10px where `scrollbar-width: thin` is set), plus grayscale
  font smoothing on macOS.
- Every selector sits inside `:where()`, so styles.css always wins; a test checks that every rule
  is gated and that the thin scrollbar list matches styles.css.
- Chromium keeps its native controls, so nothing changes on Windows.

### CI
- The `linux-bundle` job starts the unpacked AppImage on Xvfb with a session bus; it must still
  run after 30 s. Screenshot (`import -window root`) and log go into the artifact
  `linux-start-x64`, not under `linux-bundles*`, which `tools/release.py` downloads by name.
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` keeps WebKitGTK off its DMA-BUF renderer, as Xvfb has no GPU.
  > [!question] Unverified
  > That WebKitGTK needs this under Xvfb is a precaution, not measured on the runner.

## Options
- **Gate by engine (chosen)** versus redrawing the controls for all engines: a redraw in Chromium
  would have to match its native controls pixel for pixel at every scale, or Windows changes.
- **Selawik** versus shipping Segoe UI: Segoe UI may not be redistributed. Versus Noto Sans or
  Inter: other metrics, so layouts would differ from Windows.
- **Bundled Cascadia in every engine's stack** versus in WebKit's only: even behind the installed
  fonts it changes Windows, because Chromium takes the glyphs they lack from it (measured above).

## Consequences
- 254 KB more in the app bundle (44 KB Selawik, 210 KB Cascadia Mono), loaded only on macOS and
  Linux.
- On macOS and Linux the first terminal waits for the bundled Cascadia (210 KB from the app
  bundle) before it measures its cells.
- Open: WebKit draws the inside of date, time and color fields itself (its own picker, the
  order of day, month and year of its locale); only the box, font and icon follow Chromium.
  Playwright WebKit is not WebKitGTK; the CI screenshot is the check on real Linux.

## Evidence
- [[app-fonts-and-controls-measurement]]: font files, versions and sizes, coverage per language,
  and box sizes in Edge against Playwright WebKit on the same fixture with the real classes. Box
  sizes of checkbox, radio, select (fixed and auto width), slider, button, progress bar and both
  scrollbar widths match to the pixel; the select text starts 4px further left than in Chromium,
  and WebKit sizes scrollbar thumbs slightly shorter. Unstyled text fields, text areas, date,
  time and color fields match Chromium's colors, font and height; Playwright WebKit on Windows
  has no date or color picker, so their insides were not comparable.
- [[app-fonts-and-controls-measurement-2]]: glyph widths in Edge with and without the bundled
  Cascadia in the stack.
- [[terminal-font-coverage-measurement]]: why bundled fonts are the unmodified upstream files.

> [!question] Unverified
> Playwright WebKit on Windows stands in for WKWebView and WebKitGTK; that both honor
> `::-webkit-scrollbar` and the other pseudo-elements the same way is not measured on a Mac or a
> Linux machine yet. The CI screenshot (`linux-start-x64`) is the first check on WebKitGTK.
