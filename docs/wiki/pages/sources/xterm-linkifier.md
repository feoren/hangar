---
title: xterm.js 6 linkifier behavior
type: source
tags: [xterm, links, terminal]
sources: []
created: 2026-11-13
updated: 2026-11-13
raw: raw/2026-11-13-xterm-linkifier-reading.md
url: https://github.com/xtermjs/xterm.js/blob/6.0.0/src/browser/Linkifier.ts
retrieved: 2026-11-13
---

# xterm.js 6 linkifier behavior

Reading notes of xterm.js 6.0.0's `Linkifier.ts` and `RenderService.ts` in
[[2026-11-13-xterm-linkifier-reading]] (`raw/2026-11-13-xterm-linkifier-reading.md`).

## Key points

- Link providers are asked per hovered **line**, not per mouse move: on a new line, after a
  resize, and when a rendered viewport change touches the current link. Within a line xterm
  searches its cached replies (`useLineCache`).
- A fresh query replaces `_activeProviderReplies` with a new map, but a late callback of an
  earlier query still writes into the new map. An asynchronous provider must drop stale answers
  itself.
- Overlapping links of one reply are pruned (`_removeIntersectingLinks`), so overlapping
  alternatives (whole path and its segments) cannot be offered side by side.
- `Terminal.refresh(0, rows - 1)` makes xterm clear the hovered link and ask all providers again
  (`onRenderedViewportChange`, not a redraw-only render). This is the only way to change the
  hovered link without a mouse move, e.g. when Shift is pressed.
- Activation happens on `mouseup` over the same link as `mousedown`, with no selection check.

## Significance for Hangar

Shapes `src/terminalLinkProvider.ts`: a request counter drops stale answers, segments are offered
only while Shift is held (no overlap), Shift changes on a hovered link call `term.refresh`, and
`activate` returns early while text is selected. See [[terminal-links]].
