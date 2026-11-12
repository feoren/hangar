---
title: "Measurement: glyph coverage of monospace web fonts for agent CLIs"
type: source
tags: [fonts, xterm, box-drawing, measurement, settings]
sources: []
raw: raw/2026-11-12-terminal-font-coverage-measurement.md
url:
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Measurement: glyph coverage of monospace web fonts for agent CLIs

Own measurement of the `cmap` of candidate terminal fonts, done before bundling more fonts with
Hangar. Record: raw/2026-11-12-terminal-font-coverage-measurement.md.

## Key points
- Fontsource packages of fonts that come from Google Fonts (`@fontsource/*` 5.3.x: Source Code
  Pro, IBM Plex Mono, Victor Mono, Ubuntu Mono, Inconsolata, Red Hat Mono, Space Mono, Martian
  Mono, Monaspace Neon) contain **no box drawing and no block elements** in any subset. Their
  subsets follow Google's unicode ranges, which leave U+2500..U+259F out.
- The upstream files of the same fonts do contain them: Source Code Pro 2.42.0, IBM Plex Mono
  2.5.0 (`@ibm/plex-mono`) and Victor Mono 1.5.6 (`victormono`) each cover all 128 box drawing
  and all 32 block element characters.
- Fontsource packages built from upstream files keep them: Commit Mono (127/128), Maple Mono,
  Iosevka (961 KB per weight), and Cascadia Code and Intel One Mono through their `symbols2`
  subset.
- Braille (spinners of some CLIs) is missing from every bundled font; it then comes from the
  fallback in the font stack.

## Significance for Hangar
- The xterm DOM renderer draws every glyph from the font stack, so a font without box drawing
  gets the frames of agent CLIs from a fallback font with other metrics. Bundled fonts therefore
  come from the upstream packages, not from Fontsource, when Fontsource strips these ranges
  (`src/assets/fonts/`, `src/fonts.css`).
- Bundled since 0.6.0: Source Code Pro, IBM Plex Mono, Victor Mono (OFL 1.1, regular and bold,
  about 390 KB together). A face loads on first use: the chosen terminal font, and the regular
  weight of every bundled font once the font picker in Settings is shown (own check in headless
  Edge: all seven regular faces loaded, about 0.5 MB read from the app bundle).
- The files are copied unmodified; subsetting them would make them Modified Versions under the
  OFL, which may not carry the Reserved Font Names "Source" and "Plex".
