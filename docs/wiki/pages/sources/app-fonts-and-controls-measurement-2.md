---
title: "Measurement: glyph fallback into the bundled Cascadia Mono in Chromium"
type: source
tags: [fonts, cascadia, chromium, fallback, measurement]
sources: []
raw: raw/2026-11-20-bundled-mono-fallback-measurement.md
url:
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Measurement: glyph fallback into the bundled Cascadia Mono in Chromium

Own measurement in Edge on Windows 11. Record: raw/2026-11-20-bundled-mono-fallback-measurement.md.

## Key points
- A web font in the stack behind installed fonts is not inert: Chromium takes every glyph the
  installed fonts lack from it. With `Hangar Cascadia Mono` after Consolas, Arabic at 16px grows
  from 69.14px to 121.88px.
- On a Windows 10 stack without Cascadia (Consolas first), Braille changes from 96.44px to
  75.00px and U+276F from 8.20px to 9.38px.
- With the bundled face in WebKit's stack only, `--mono` and `fontStack("default")` measure the
  same in Chromium as the old stack.

## Significance for Hangar
- Basis of the stack per engine in [[0018-same-look-on-every-platform]].
