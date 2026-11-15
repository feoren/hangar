# Measurement: how Claude Code wraps a long prompt through ConPTY

- Date: 2026-11-15
- Platform: Windows 11 Pro 10.0.26300, x64
- Program: Claude Code 2.1.325 (`claude.exe`, no arguments), working folder a trusted repository
- Terminal: Windows ConPTY (`CreatePseudoConsole`) 50 columns by 30 rows, driven by a small Python
  ctypes script that records every byte the pseudo console writes. No terminal emulator in between, no
  answer to any query.
- Input: typed one character every 30 ms, then Left arrows (`ESC [ D`) with pauses, then one marker
  character, then Ctrl+C three times. Enter was never sent, so no prompt reached a model.

Output excerpts below are Python `repr` of the recorded bytes; `<<<PROBE ...>>>` lines are markers the
script inserted between the steps.

## Run 1: words, `alpha bravo ... sierra tango` (128 characters)

Idle prompt row (row 7) before typing, the placeholder drawn faint:

```
\x1b[m\r\n❯\xa0\x1b[2mTry\x1b[1C"edit\x1b[1C<filepath>\x1b[1Cto..."\x1b[38;2;136;136;136m\x1b[22m\r\n──────────────────────────────────────────────────
```

When the typed text reached the row's end, Claude Code moved the whole last word to a new row: it blanked
the start of the word on row 7 and wrote the next row after a real line break, indented by two blanks:

```
...\x1b[1Cfoxtrot\x1b[1Cgolf\x1b[1Cho\x1b[?25l\x1b[7;47H    \r\n  hot\x1b[K\x1b[38;2;136;136;136m\r\n───...
```

After typing: row 7 `❯ alpha bravo charlie delta echo foxtrot golf `, row 8
`  hotel india juliet kilo lima mike november `, row 9 `  oscar papa quebec romeo sierra tango`, then the
horizontal rule. The cursor (the real terminal cursor, shown with `ESC [?25h`) stood after `tango`.

20 Left presses moved the cursor 20 columns left on row 9 (ConPTY wrote 20 `\x08`); the marker landed
there (`\x1b[?25lXc romeo sie\x1b[1Cra tango\x1b[9;20H`).

## Run 2: same text, Left 36, then 1, 1, 1

```
<<<PROBE left36>>>  36 x \x08                        cursor at row 9 column 3, before "oscar"
<<<PROBE left1>>>   \x1b[?25l\x1b[8;45H\x1b[?25h       cursor at row 8 column 45, the blank after "november"
<<<PROBE left1>>>   \x08                              row 8 column 44, before "r"
<<<PROBE left1>>>   \x08                              row 8 column 43, before "e"
<<<PROBE insertX>>> \x1b[?25lXer\x1b[8;44H\x1b[?25h    "novembXer"
```

## Run 3: `ab ` and 60 times `x` (one word longer than a row)

The word first moved to row 8 like in run 1, then, once longer than a row, was broken inside the word:
row 7 `❯ ab ` followed by 43 `x` in columns 6 to 48, row 8 two blanks and the remaining 17 `x`. Columns 49
and 50 stayed empty on both rows, so the text area is 46 columns wide (columns 3 to 48) at 50 columns.
