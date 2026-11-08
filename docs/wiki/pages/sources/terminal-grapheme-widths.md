---
title: "Measurement: terminal cell widths, Unicode 11 vs 15-graphemes"
type: source
tags: [xterm, unicode, emoji, measurement, claude-code, windows]
sources: []
raw: raw/2026-11-08-terminal-grapheme-widths.md
url:
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Measurement: terminal cell widths, Unicode 11 vs 15-graphemes

Own measurement with @xterm/xterm 6.0.0 in Node (buffer only, no DOM), triggered by Claude Code's
status line breaking in Hangar. Record: raw/2026-11-08-terminal-grapheme-widths.md.

## Key points
- With @xterm/addon-unicode11 0.9.0 (`activeVersion = "11"`), a text-default symbol plus VS16
  (U+FE0F), such as 🏷️, ✏️, ⏱️, is **1 cell**. Emoji that are emoji by default (🔋, 📁, ⚡) are 2.
- With @xterm/addon-unicode-graphemes 0.4.0 (`activeVersion = "15-graphemes"`), the same sequences
  are **2 cells**, as string-width (Ink, Claude Code) counts them. Claude Code computes its
  layout with 2, so with Unicode 11 every cursor position after such an emoji was off by one:
  clipped glyphs, overlapping characters, early wraps.
- Measured with 15-graphemes: CJK 2; combining accent 1; ZWJ family, skin tone with ZWJ, flag,
  keycap 2 each (whole grapheme cluster); box drawing, blocks, braille spinner, powerline (private
  use) and agent glyphs without VS16 (⏺ ✻ ✽ ✢ · ⎿ ❯ ✓ ✔ ● … → ↻) 1 per character. The example
  status line `⏱️ 5h 07m | 🔋 5h 37%` is 21 cells, the same as string-width.
- The addon's README calls it experimental ("may introduce unexpected and non-standard behavior").
- @xterm/xterm 6.0.0 and @xterm/addon-unicode-graphemes 0.4.0 come from the same release commit
  `f447274f` (npm `gitHead`) and were published within a minute; 0.4.0 is the only stable version
  (the rest are 0.5.0 betas). MIT, no dependencies.
- Naming color emoji fonts (Segoe UI Emoji, Apple Color Emoji, Noto Color Emoji) in the terminal
  font stack turns text symbols such as ⏺ ✔ ⚠ ℹ into wide color emoji (reported by the acceptance
  review, not measured in the record). The width fix needs no font change.

## Significance for Hangar
- `src/terminalUnicode.ts` loads the grapheme addon and activates `15-graphemes`;
  `src/terminalUnicode.test.ts` holds the table above as regression tests. @xterm/addon-unicode11
  was removed.
- The terminal font stack (`src/terminalLook.ts`) names no emoji font, so agent CLIs' text
  symbols keep one cell and their text look.

> [!question] Unverified
> Rendering inside the real WebView2 window and on macOS and Linux fonts; only headless Edge on
> Windows was looked at. The "experimental" label means behavior may change in later addon
> versions; the tests catch a width change.
