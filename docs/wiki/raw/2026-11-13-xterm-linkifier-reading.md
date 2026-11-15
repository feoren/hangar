# xterm.js 6.0.0 Linkifier: reading notes

Source: `node_modules/@xterm/xterm/src/browser/Linkifier.ts` and
`node_modules/@xterm/xterm/src/browser/services/RenderService.ts`, package `@xterm/xterm` 6.0.0
(as installed in Hangar on 2026-11-13). Read in full; quotes verbatim.

## Hover and line cache

- `mousemove` on the terminal element calls `_handleHover` only when the cell changed:
  `if (!this._lastBufferCell || (position.x !== this._lastBufferCell.x || position.y !== this._lastBufferCell.y))`.
  `_lastBufferCell` is not reset on `mouseleave`.
- `_handleHover`: a new line (`this._activeLine !== position.y`) or a resize clears the current
  link and calls `_askForLink(position, false)`; on the same line, only when the current link is
  not under the mouse, `_askForLink(position, true)` (line cache).
- `_askForLink(..., false)` disposes the old replies and sets
  `this._activeProviderReplies = new Map()`, then calls every provider's
  `provideLinks(position.y, callback)`. The callback does
  `this._activeProviderReplies?.set(i, linksWithState)`: a late callback from an earlier request
  writes into the map of the current line.
- With `useLineCache` the providers are not asked again; the cached reply is searched with
  `_linkAtPosition`, which reads `link.range` at that moment.
- Once all providers answered, `_removeIntersectingLinks` drops every link that shares a cell of
  the hovered row with an earlier one, also within the same provider's reply.

## Re-query on render

- `_handleNewLink` registers `this._renderService.onRenderedViewportChange(e => ...)`. When the
  rendered rows contain the current link it calls `_clearCurrentLink(start, end)` and then
  `this._askForLink(position, false)`: a fresh query of all providers.
  "When start is 0 a scroll most likely occurred": `const start = e.start === 0 ? 0 : e.start + 1 + ydisp`.
- `RenderService._renderRows` fires `_onRenderedViewportChange` unless the render was
  redraw-only; `Terminal.refresh(start, end)` calls `refreshRows(start, end)` with
  `isRedrawOnly = false`.

## Activation

- `mousedown` stores the current link, `mouseup` activates it when the same link is still under
  the mouse: `this._currentLink.link.activate(event, this._currentLink.link.text)`. There is no
  check for a selection or a drag.
- Ranges: `x` and `y` are 1 based, `y` a buffer line; the underline event uses
  `range.start.x - 1` and `range.start.y - scrollOffset - 1`.
