# Measurement: editing Claude Code's own wrapped prompt through ConPTY

- Date: 2026-11-15
- Platform: Windows 11 Pro 10.0.26300, x64
- Program: Claude Code 2.1.325 (`claude.exe`, no arguments), a trusted working folder
- Terminal: Windows ConPTY 50 columns by 30 rows (run F: 80), driven by a Python ctypes script
  (`tests/.tmp/conpty_probe2.py`, not tracked). Its output is fed to `pyte` 0.8.2 (a VT emulator) and the
  screen with the real cursor position is printed after each step. Enter (CR alone) was never sent; the
  only newline key was ESC CR (run E). Every run ended with three Ctrl+C and a terminate; no `claude`
  process was left over (process list compared before and after).
- Text is typed one character every 25 ms, arrow keys are `ESC [ D` and `ESC [ C`, Backspace is `0x7f`.
- Gemini CLI is not installed on this machine, so it was not measured.

Rows are shown as `row |50 columns|`. The cursor is the terminal's cursor (row and column 1-based). Only
the input block is kept from each screen.


## Run A: words (128 characters), Right and Backspace across the boundary

Steps: Left 36 then 1; Right 1; Left 2; Right 1, 1, 1; Right 5; Backspace 7.

```
=== A0 typed  cursor row 9 col 39
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A1 left36 (expect row9 col3)  cursor row 9 col 3
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A2 left1 more  cursor row 8 col 45
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A3 right1  cursor row 9 col 3
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A4 left2 from A3  cursor row 8 col 44
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A5 right1  cursor row 8 col 45
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A6 right1  cursor row 9 col 3
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A7 right1  cursor row 9 col 4
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A8 right5 (after oscar?)  cursor row 9 col 9
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike november      |
 9 |  oscar papa quebec romeo sierra tango            |
10 |──────────────────────────────────────────────────|
=== A9 backspace x7  cursor row 8 col 45
 7 |❯ alpha bravo charlie delta echo foxtrot golf     |
 8 |  hotel india juliet kilo lima mike novemberpapa  |
 9 |  quebec romeo sierra tango                       |
10 |──────────────────────────────────────────────────|
```

## Run B1: two blanks at the wrap, text ends 3 columns before the edge

Steps: 43 x `a`, two blanks, `word`; Left 4, 1, 1, 1; Right 3, 1; Backspace 3.

```
=== B1 typed  cursor row 8 col 7
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 left4 (start of word)  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 left1  cursor row 7 col 47
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 left1  cursor row 7 col 46
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 left1  cursor row 7 col 45
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 right3  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 right1  cursor row 8 col 4
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa     |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B1 backspace3 at start of word  cursor row 7 col 46
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaord  |
 8 |──────────────────────────────────────────────────|
```

## Run B2: two blanks at the wrap, text ends 4 columns before the edge

Steps: 44 x `a`, two blanks, `word`; Left 4, 1, 1, 1; Right 3; Backspace 3.

```
=== B2 typed  cursor row 8 col 7
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 left4  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 left1  cursor row 7 col 48
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 left1  cursor row 7 col 47
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 left1  cursor row 7 col 46
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 right3  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa    |
 8 |  word                                            |
 9 |──────────────────────────────────────────────────|
=== B2 backspace3  cursor row 7 col 46
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaawor  |
 8 |  d                                               |
 9 |──────────────────────────────────────────────────|
```

## Run C1: a row filled exactly to the edge, then a blank and `next`

Steps: 10 x `a`, blank, 35 x `b`; then a blank; then `next`; Left 4, 1, 1, 1; Right 3; Backspace 3.

```
=== C1 46 chars  cursor row 7 col 49
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |──────────────────────────────────────────────────|
=== C1 + blank  cursor row 8 col 3
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |                                                  |
 9 |──────────────────────────────────────────────────|
=== C1 + next  cursor row 8 col 7
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 left4  cursor row 8 col 3
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 left1  cursor row 8 col 3
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 left1  cursor row 7 col 48
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 left1  cursor row 7 col 47
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 right3  cursor row 8 col 3
 7 |❯ aaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  |
 8 |  next                                            |
 9 |──────────────────────────────────────────────────|
=== C1 backspace3  cursor row 8 col 36
 7 |❯ aaaaaaaaaa                                      |
 8 |  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbnext           |
 9 |──────────────────────────────────────────────────|
```

## Run C2: one word of 50 characters (split inside the word)

Steps: 50 x `a`; Left 4, 1; Right 1; Backspace 2.

```
=== C2 50 a  cursor row 8 col 7
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaa                                            |
 9 |──────────────────────────────────────────────────|
=== C2 left4  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaa                                            |
 9 |──────────────────────────────────────────────────|
=== C2 left1  cursor row 7 col 48
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaa                                            |
 9 |──────────────────────────────────────────────────|
=== C2 right1  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaa                                            |
 9 |──────────────────────────────────────────────────|
=== C2 backspace2  cursor row 7 col 47
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aa                                              |
 9 |──────────────────────────────────────────────────|
```

## Run D: a wide character that does not fit at the end of the row

Steps: 45 x `a`, a wide character, `bc`; Left 2, 1, 1, 1; Right 2, 1; Backspace 2.

```
=== D typed  cursor row 8 col 7
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D left2 (before b)  cursor row 8 col 5
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D left1 (before wide)  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D left1  cursor row 7 col 47
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D left1  cursor row 7 col 46
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D right2  cursor row 8 col 3
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D right1  cursor row 8 col 5
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |
 8 |  中bc                                            |
 9 |──────────────────────────────────────────────────|
=== D backspace2  cursor row 7 col 47
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabc  |
 8 |──────────────────────────────────────────────────|
```

## Run E: Shift+Enter (ESC CR) between `hello` and `world`

Steps: `hello`, ESC CR, `world`; Left 5, 1; Right 1; Backspace 1.

```
=== E typed  cursor row 8 col 8
 7 |❯ hello                                           |
 8 |  world                                           |
 9 |──────────────────────────────────────────────────|
=== E left5  cursor row 8 col 3
 7 |❯ hello                                           |
 8 |  world                                           |
 9 |──────────────────────────────────────────────────|
=== E left1  cursor row 7 col 8
 7 |❯ hello                                           |
 8 |  world                                           |
 9 |──────────────────────────────────────────────────|
=== E right1  cursor row 8 col 3
 7 |❯ hello                                           |
 8 |  world                                           |
 9 |──────────────────────────────────────────────────|
=== E backspace1 at line start  cursor row 7 col 8
 7 |❯ helloworld                                      |
 8 |──────────────────────────────────────────────────|
```

## Run F: 80 columns, 100 x `a`

Steps: Left 54, 1.

```
=== F typed 100 a  cursor row 8 col 27
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaaaaaaaaaaaaaaaaaaaaaa                                                      |
 9 |────────────────────────────────────────────────────────────────────────────────|
=== F left54  cursor row 7 col 49
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaaaaaaaaaaaaaaaaaaaaaa                                                      |
 9 |────────────────────────────────────────────────────────────────────────────────|
=== F left1  cursor row 7 col 48
 7 |❯ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  |
 8 |  aaaaaaaaaaaaaaaaaaaaaaaa                                                      |
 9 |────────────────────────────────────────────────────────────────────────────────|
```
