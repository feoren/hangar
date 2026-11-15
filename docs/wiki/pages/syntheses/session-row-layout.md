---
title: Session row layout (row builder)
type: synthesis
tags: [sidebar, settings, session-row, accessibility, contract]
sources: []
created: 2026-11-15
updated: 2026-11-15
---

# Session row layout (row builder)

Settings > Appearance > "Session row" lets the user build the inside of a sidebar session row
instead of picking one of fourteen fixed styles. Groups, tree guides and sub-agent rows stay the
same for every layout; only the row's inside changes.

## Model (`src/rowLayout.ts`)

- A `RowLayout` has exactly four lines (`ROW_LINES`); each line has a `left` and a `right` part,
  each an ordered list of element ids. Empty lines take no room.
- Elements: `title`, `state`, `program`, `signs`, `folder`, `branch`, `load`, `runtime`,
  `context`, `tokens`. An element on no line is hidden; the title can never be hidden.
- Forms (`forms`, kept while an element is hidden):
  - state: `word`, `pill`, `symbol` (the status chip's shape), `edge` (row tinted), `band`
  - context: `bar`, `mini`, `ring`, `segments`, `number`
  - load: `text`, `pills`, `tiles`
  - program: `icon` (only in groups whose header does not name the program), `avatar` (with
    the state dot on its corner)
  - folder, branch: `text`, `tag`
- Row-wide: `frame` (`flat`, `card`) and `density` (`normal`, `compact`).
- `parseRowLayout(value, legacyStyle)` validates stored JSON strictly: unknown ids dropped, each
  element at most once (first place wins), at most 32 entries read per part, lines beyond four
  dropped, unknown forms, frame and density fall back to the default, a missing title is added
  to the end of line 1's left part.
- Moves are pure: `placeElement` (drag and drop; the index counts the part as it is now),
  `stepElement` (arrow buttons: left and right walk the line across both parts, up and down keep
  the part and the place), `hideElement`, `showElement` (back to where the standard row has it),
  `setFormByName`.
- `planRow` turns a layout into a `RowPlan` once per layout (Sidebar `useMemo`), not per row
  and render: the lines with content, an avatar or ring at line 1's start (`lead`) or end
  (`trail`) that spans the first two lines, and what the peek must spell out (`peekLoad`,
  `peekContext`).

## Presets and migration

The fourteen row styles of 0.8.0 development builds are presets (`ROW_PRESETS`,
`presetLayout`). A stored `rowStyle` setting becomes its preset in `parseSettings` when no
`rowLayout` is stored; the next save stores only `rowLayout`. Three styles cannot be built from
elements and map to the closest layout:

| Style | Was | Now |
| --- | --- | --- |
| accordion | one line, a chevron or the active row shows lines 2 to 4 | one line: title, small context bar, state; the peek shows the rest |
| counters | state shape before the title, run time at its end, state counts on the group header | the same row without the counts on the group header |
| columns | CPU, share and run time in columns named once above the list | share and run time at the title line's end, no heading; the peek shows the load |

`card` now shows the state dot on the avatar's corner, as the avatar form always does.

## Rendering (`src/SessionRow.tsx`, styles.css "Row layout")

- Each line is a flex row: left elements, a zero-width spacer (`.row-push`) that pushes the
  right part to the edge, the right elements. The existing shrink rules of title, folder and
  branch and the fixed cells of the facts keep working because every element is a direct item.
- On the title's line the DOM order is title, full status sentence, then what follows, then what
  stands before the title in a `.row-before-title` wrapper with `order: -2`, so screen readers
  hear the title first. The full status sentence and the context's numbers are always read once,
  whatever is hidden.
- A line shows only if one of its elements has content; a run time keeps its empty cell on a
  line that shows.
- Row-wide looks are attributes on `.session-groups` and on the settings preview:
  `data-row-state`, `data-row-frame`, `data-row-density`. Compact density changes only the lines
  below the title, so the tree's elbow (`--agent-elbow-y`) still meets the title line.

## Editor (`src/RowLayoutSettings.tsx`)

Live preview on top (the same markup as the sidebar), a template picker and "Reset to standard",
then the four lines with their two parts as drop zones and the hidden elements below. Chips are
dragged with pointer events (pointer capture, 4 px threshold, `elementFromPoint` for the target,
an accent bar marks the place). For the keyboard and screen readers a chip is chosen with Enter
and moved with its four arrow buttons or the arrow keys; focus follows the moved chip, a polite
live region says where it went. Each chip has its form picker and an "x"; hidden elements come
back with "+".

## Verification (2026-11-15, acceptance review)

- Tree guides: all fourteen presets rendered with the branch's and with main's `styles.css` in
  Edge 157 headless at 305 px. Row height, title center (15.5 px, 17.5 px with the band) and the
  guide's box matched main in every preset; only the title's x moved by the preview's badge.
- Editor: a production build of `RowLayoutField` driven over the DevTools protocol with real mouse
  and key input (Chromium, as in WebView2): a drag across parts lands before the chip under the
  pointer and shows one marker and one drop zone, a drag onto the hidden list hides the element,
  a press without travel chooses the chip, arrow keys and arrow buttons move it with focus kept
  and the live region updated, the "x" hands focus to the element's "+", Enter on "+" shows it
  back. Arrow buttons at an edge are `aria-disabled`.

> [!question] Unverified
> The editor and the presets in the app window itself (WebView2, light theme, 720 px window).
