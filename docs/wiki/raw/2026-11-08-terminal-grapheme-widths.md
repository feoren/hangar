# Measurement record: terminal cell widths, Unicode 11 vs 15-graphemes (2026-11-08)

Raw record. Immutable. Summary: [[terminal-grapheme-widths]].

## Trigger

Claude Code's custom status line looked broken in Hangar: emoji with VS16 (U+FE0F) such as
"🏷️", "✏️", "⏱️" were clipped to half a glyph, the next character overlapped them, and the line
wrapped or was cut early. Example line (personal values shortened):

```
█████░░░░░ 52% (515k/1M) | ⚡ 98% 54m | ✏️ +23039 -2428 | 💰 $141.74 | ⏱️ 5h 07m | 🔋 5h 37% ↻ 3h17m
```

## Platform and versions

- Windows 11 Pro, build 10.0.26300; Node v26.13.0; Vitest 5.1.0
- @xterm/xterm 6.0.0; @xterm/addon-unicode11 0.9.0 (before); @xterm/addon-unicode-graphemes 0.4.0 (after)
- npm registry, `npm view <pkg>@<version> gitHead`:
  - @xterm/xterm 6.0.0: `f447274f430fd22513f6adbf9862d19524471c04`
  - @xterm/addon-unicode-graphemes 0.4.0: `f447274f430fd22513f6adbf9862d19524471c04`
- npm publish times (`npm view <pkg> time`): xterm 6.0.0 2025-12-22T13:50:12Z, addon-unicode11
  0.9.0 2025-12-22T13:50:48Z, addon-unicode-graphemes 0.4.0 2025-12-22T13:50:52Z. Dist tags of
  addon-unicode-graphemes: `latest` 0.4.0, `beta` 0.5.0-beta.330; 0.4.0 is the only stable
  version. License MIT, no dependencies or peer dependencies, unpacked size 618063 bytes.
- `node_modules/@xterm/addon-unicode-graphemes/README.md`, line 3:
  `⚠️ **This addon is currently experimental and may introduce unexpected and non-standard behavior**`

## Method

A `Terminal` from @xterm/xterm, not opened (the buffer and width rules run without a DOM),
`allowProposedApi: true`, 80 to 200 columns. For each text: write `ESC[H ESC[2K <text>` and read
`term.buffer.active.cursorX` in the write callback. Before: `Unicode11Addon`, `activeVersion =
"11"`. After: `UnicodeGraphemesAddon`, `activeVersion = "15-graphemes"` (the addon registers the
versions `6`, `15`, `15-graphemes` and activates `15-graphemes` on load). The after values are the
assertions of `src/terminalUnicode.test.ts`.

## Results (cells)

| Text | Unicode 11 | 15-graphemes |
|---|---|---|
| 🏷️ (U+1F3F7 U+FE0F) | 1 | 2 |
| ✏️ (U+270F U+FE0F) | 1 | 2 |
| ⏱️ (U+23F1 U+FE0F) | 1 | 2 |
| 🔋 (U+1F50B) | 2 | 2 |
| 📁 (U+1F4C1) | 2 | 2 |
| ⚡ (U+26A1) | 2 | 2 |
| 中 | not measured | 2 |
| 日本語 | not measured | 6 |
| e + U+0301 (combining) | not measured | 1 |
| 👨‍👩‍👧‍👦 (ZWJ family) | not measured | 2 |
| 👩🏽‍💻 (skin tone and ZWJ) | not measured | 2 |
| 🇩🇪 (flag) | not measured | 2 |
| 🇩🇪🇺🇸 (two flags) | not measured | 4 |
| #️⃣ (keycap) | not measured | 2 |
| █ ░ and the other blocks, box drawing ─│┌…╰, braille ⠋⠙⠹, powerline U+E0A0, U+E0B0 to U+E0B6 | not measured | 1 per character |
| agent glyphs without VS16: ⏺ ✻ ✽ ✢ · ⎿ ❯ ✓ ✔ ● … → ↻ | not measured | 1 per character |
| `🏷️ x` | 3 | 4 |
| `⏱️ 5h 07m \| 🔋 5h 37%` | not measured | 21 |

string-width (used by Ink, which Claude Code renders with) counts the last line as 21: two cells
per emoji, one per other character.

## Fonts

Visual check in headless Edge (Chromium, the engine of WebView2) with the example line after the
change: the VS16 emoji render in color, two cells wide, the next character no longer overlaps.

During the acceptance review the font stack was briefly extended with `"Segoe UI Emoji",
"Apple Color Emoji", "Noto Color Emoji"` after `monospace`. Reported by the review (not measured
in this record): Claude Code's text symbols ⏺ ✔ ⚠ ℹ then rendered as wide color emoji, because
those fonts have color glyphs for them. The names were removed again; the width fix needs only
the grapheme rules.
