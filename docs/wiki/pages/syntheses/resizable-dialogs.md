---
title: Resizable dialogs
type: synthesis
tags: [ui, dialogs, settings, pointer-events, dpi]
sources: []
created: 2026-11-15
updated: 2026-11-15
---

# Resizable dialogs

The settings dialog can be resized by its edges and corners like a window. Code: `src/dialogSize.ts`
(pure rules, tests in `src/dialogSize.test.ts`), `src/DialogResize.tsx` (`useDialogSize`,
`DialogResizeHandles`), used in `src/SettingsDialog.tsx`; styles `.dialog.resizable` and
`.dialog-resize` in `src/styles.css`.

## Behaviour

- **Grips** on all four edges and corners, 8 px wide, straddling the border so the hit area reaches
  4 px outside the dialog. Cursors `ns-resize`, `ew-resize`, `nesw-resize`, `nwse-resize`. The lower
  right corner shows two faint diagonal strokes as the visible hint.
- **Symmetric growth:** the overlay centers the dialog, so every edge changes the size by twice the
  pointer's way and the dragged edge stays under the pointer (`dragDialogSize`). Always computed from
  the size at the press, so after a limit the edge meets the pointer again where it left it.
- **Limits:** standard 936 by 672 (matches `--large-dialog-width` and `--large-dialog-height`), minimum
  640 by 420 (section list and a field row side by side), maximum the window less 16 px on each side.
  A window smaller than the minimum wins, so the dialog still fits (`clampDialogSize`).
- **Window resize:** the chosen size is kept as preferred; the shown size is that clamped to the
  current window, so a window made smaller and then larger gets the chosen size back.
- **Double click** on any grip: standard size, the remembered size is forgotten.
- **Keyboard:** only the lower right corner grip is in the Tab order (role `separator`, labelled);
  Left and Right change the width, Up and Down the height, 16 px per press. The edges are labelled
  separators outside the Tab order, so the dialog does not get eight extra stops.
- **Remembered** per computer in `localStorage` (`hangar.settingsDialogSize`, JSON, validated as
  `unknown`, every access in try/catch), like other comfort values (`notesSide.ts`, `layout.ts`).

## Performance and rendering

- Pointer Events with `setPointerCapture`; the size is applied at most once per animation frame
  straight to the dialog's inline style. React state and storage get it once, on pointer up, cancel or
  lost capture, so the settings content does not render on every pointer move.
- Pointer coordinates and CSS sizes are both CSS pixels, so no scaling factor enters the drag. The shown
  size is snapped so that the centered dialog's left and top edge fall on whole device pixels at any
  `devicePixelRatio` (`snapDialogSize`); a box between device pixels blurs its text at 125 % or 150 %.

> [!question] Unverified
> That the resize cursor stays while the pointer is captured and leaves the grip during a fast drag
> (Chromium and WebView2 take the cursor from the capturing element), and the blur avoided by snapping,
> were not checked in the running window.

## Other dialogs

The Team window (`.dialog.team-window`) shares the large dialog size and could take the same hook and
grips with its own storage key; it is not changed yet. Small dialogs (confirmations, the launcher) gain
nothing from resizing.
